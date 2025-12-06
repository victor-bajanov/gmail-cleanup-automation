//! Gmail API client with rate limiting and retry logic

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures::stream::{self, StreamExt, TryStreamExt};
use google_gmail1::{
    api::{BatchModifyMessagesRequest, Filter, FilterAction, FilterCriteria, Label, Message, ModifyMessageRequest},
    hyper_rustls, hyper_util, Gmail,
};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;
use tracing::{debug, warn};

use crate::error::{GmailError, Result};
use crate::models::{FilterRule, MessageMetadata};

/// Progress callback type for batch operations
pub type ProgressCallback = Arc<dyn Fn() + Send + Sync>;

/// Label info returned from Gmail API
#[derive(Debug, Clone)]
pub struct LabelInfo {
    pub id: String,
    pub name: String,
}

/// Existing Gmail filter info for comparison
#[derive(Debug, Clone)]
pub struct ExistingFilterInfo {
    pub id: String,
    pub query: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub subject: Option<String>,
    pub add_label_ids: Vec<String>,
    pub remove_label_ids: Vec<String>,
}

impl ExistingFilterInfo {
    /// Check if this existing filter matches the new filter rule
    /// Returns true if they are functionally equivalent
    pub fn matches_filter_rule(&self, new_filter: &FilterRule) -> bool {
        // Compare the from pattern / query
        let query_matches = match (&self.query, &new_filter.from_pattern) {
            (Some(existing_query), Some(new_pattern)) => {
                // Normalize for comparison
                let existing_normalized = existing_query.to_lowercase().trim().to_string();
                let new_normalized = new_pattern.to_lowercase().trim().to_string();
                // Check if the from pattern is contained in the query
                existing_normalized.contains(&format!("from:({})", new_normalized)) ||
                existing_normalized == format!("from:({})", new_normalized)
            }
            (None, None) => true,
            _ => false,
        };

        // Compare the from field directly as well
        let from_matches = match (&self.from, &new_filter.from_pattern) {
            (Some(existing_from), Some(new_pattern)) => {
                let existing_normalized = existing_from.to_lowercase().trim().to_string();
                let new_normalized = new_pattern.to_lowercase().trim().to_string();
                existing_normalized == new_normalized
            }
            _ => false, // from field is alternative to query
        };

        // If neither query nor from matches, filters are different
        if !query_matches && !from_matches {
            return false;
        }

        // Compare subject keywords
        // Extract subject from existing query if present
        let existing_query_lower = self.query.as_deref().unwrap_or("").to_lowercase();
        let existing_has_subject = existing_query_lower.contains("subject:(");

        let subject_matches = if new_filter.subject_keywords.is_empty() {
            // New filter has no subject - existing should also have no subject
            !existing_has_subject
        } else {
            // New filter has subject keywords - check if existing query contains them
            new_filter.subject_keywords.iter().all(|keyword| {
                let keyword_lower = keyword.to_lowercase();
                existing_query_lower.contains(&format!("subject:({})", keyword_lower)) ||
                existing_query_lower.contains(&format!("subject:(\"{}\")", keyword_lower))
            })
        };

        if !subject_matches {
            return false;
        }

        // Compare add_label_ids
        let label_matches = self.add_label_ids.contains(&new_filter.target_label_id);

        // Compare archive behavior (remove INBOX)
        let archive_matches = if new_filter.should_archive {
            self.remove_label_ids.iter().any(|l| l == "INBOX")
        } else {
            !self.remove_label_ids.iter().any(|l| l == "INBOX")
        };

        label_matches && archive_matches
    }
}

/// Trait defining Gmail client operations for easier testing
#[async_trait]
pub trait GmailClient: Send + Sync {
    /// List all message IDs matching a query
    async fn list_message_ids(&self, query: &str) -> Result<Vec<String>>;

    /// Get detailed message metadata
    async fn get_message(&self, id: &str) -> Result<MessageMetadata>;

    /// List all labels in the account
    async fn list_labels(&self) -> Result<Vec<LabelInfo>>;

    /// Create a new label
    async fn create_label(&self, name: &str) -> Result<String>;

    /// Delete a label by ID
    async fn delete_label(&self, label_id: &str) -> Result<()>;

    /// Create a new filter rule
    async fn create_filter(&self, filter: &FilterRule) -> Result<String>;

    /// List all existing filters
    async fn list_filters(&self) -> Result<Vec<ExistingFilterInfo>>;

    /// Delete an existing filter by ID
    async fn delete_filter(&self, filter_id: &str) -> Result<()>;

    /// Update an existing filter (delete and recreate with new settings)
    async fn update_filter(&self, filter_id: &str, filter: &FilterRule) -> Result<String>;

    /// Apply a label to a message
    async fn apply_label(&self, message_id: &str, label_id: &str) -> Result<()>;

    /// Remove a label from a message (used for archiving - removing INBOX)
    async fn remove_label(&self, message_id: &str, label_id: &str) -> Result<()>;

    /// Remove a label from multiple messages in batch (up to 1000 per call)
    /// Returns the number of messages successfully modified
    async fn batch_remove_label(&self, message_ids: &[String], label_id: &str) -> Result<usize>;

    /// Add a label to multiple messages in batch (up to 1000 per call)
    /// Returns the number of messages successfully modified
    async fn batch_add_label(&self, message_ids: &[String], label_id: &str) -> Result<usize>;

    /// Batch modify labels on multiple messages (up to 1000 per call)
    /// Can add and remove labels in a single API call
    /// Returns the number of messages successfully modified
    async fn batch_modify_labels(
        &self,
        message_ids: &[String],
        add_label_ids: &[String],
        remove_label_ids: &[String],
    ) -> Result<usize>;

    /// Fetch multiple messages concurrently
    async fn fetch_messages_batch(&self, message_ids: Vec<String>) -> Result<Vec<MessageMetadata>>;

    /// Fetch multiple messages with progress callback
    async fn fetch_messages_with_progress(
        &self,
        message_ids: Vec<String>,
        on_progress: ProgressCallback,
    ) -> Result<Vec<MessageMetadata>>;
}

/// Rate-limited Gmail client using semaphores
///
/// This struct wraps the Gmail hub and enforces rate limits using a semaphore.
/// Based on implementation spec lines 254-278.
pub struct RateLimitedGmailClient<T> {
    hub: Gmail<T>,
    semaphore: Arc<Semaphore>,
}

impl<T> RateLimitedGmailClient<T>
where
    T: Send + Sync + 'static,
{
    /// Create a new rate-limited client
    ///
    /// # Arguments
    /// * `hub` - Gmail API hub instance
    /// * `max_concurrent` - Maximum concurrent requests (typically 40-50 for 5-unit operations)
    pub fn new(hub: Gmail<T>, max_concurrent: usize) -> Self {
        Self {
            hub,
            semaphore: Arc::new(Semaphore::new(max_concurrent)),
        }
    }

    /// Get the inner hub reference
    pub fn hub(&self) -> &Gmail<T> {
        &self.hub
    }

    /// Acquire a semaphore permit
    pub async fn acquire_permit(&self) -> Result<tokio::sync::SemaphorePermit<'_>> {
        self.semaphore
            .acquire()
            .await
            .map_err(|e| GmailError::Unknown(format!("Failed to acquire permit: {}", e)))
    }
}

/// Production Gmail client with rate limiting and retry logic
///
/// This implementation includes:
/// - Semaphore-based rate limiting
/// - Exponential backoff retry logic
/// - Concurrent message fetching with buffered streams
///
/// Based on implementation spec lines 922-1011.
pub struct ProductionGmailClient {
    hub: Gmail<hyper_rustls::HttpsConnector<hyper_util::client::legacy::connect::HttpConnector>>,
    rate_limiter: Arc<Semaphore>,
}

impl ProductionGmailClient {
    /// Create a new production Gmail client
    ///
    /// # Arguments
    /// * `hub` - Gmail API hub instance
    /// * `max_concurrent` - Maximum concurrent requests (typically 40-50)
    pub fn new(
        hub: Gmail<
            hyper_rustls::HttpsConnector<hyper_util::client::legacy::connect::HttpConnector>,
        >,
        max_concurrent: usize,
    ) -> Self {
        Self {
            hub,
            rate_limiter: Arc::new(Semaphore::new(max_concurrent)),
        }
    }

    /// Fetch a single message with retry logic
    async fn fetch_single_with_retry(&self, id: &str) -> Result<MessageMetadata> {
        let _permit = self.rate_limiter.acquire().await.map_err(|e| {
            GmailError::Unknown(format!("Failed to acquire rate limit permit: {}", e))
        })?;

        let mut attempts = 0;
        let max_attempts = 4; // Initial + 3 retries
        let mut delay = std::time::Duration::from_millis(100);

        loop {
            attempts += 1;

            let result = self
                .hub
                .users()
                .messages_get("me", id)
                .format("metadata")
                .add_metadata_headers("From")
                .add_metadata_headers("Subject")
                .add_metadata_headers("Date")
                .add_metadata_headers("List-Unsubscribe")
                .add_scope("https://www.googleapis.com/auth/gmail.modify")
                .doit()
                .await;

            match result {
                Ok((_, msg)) => {
                    match parse_message_metadata(msg) {
                        Ok(metadata) => return Ok(metadata),
                        Err(_e) if attempts < max_attempts => {
                            tokio::time::sleep(delay).await;
                            delay *= 2; // Exponential backoff
                            continue;
                        }
                        Err(e) => return Err(e),
                    }
                }
                Err(e) => {
                    let gmail_error = GmailError::from(e);
                    if gmail_error.is_transient() && attempts < max_attempts {
                        tokio::time::sleep(delay).await;
                        delay *= 2;
                        continue;
                    }
                    return Err(gmail_error);
                }
            }
        }
    }

    /// Check if an error is retryable
    fn should_retry(error: &GmailError) -> bool {
        matches!(
            error,
            GmailError::ServerError { .. }
                | GmailError::RateLimitExceeded { .. }
                | GmailError::NetworkError(_)
        )
    }

    /// Execute an async operation with exponential backoff retry
    async fn with_retry<T, F, Fut>(
        operation_name: &str,
        max_retries: u32,
        mut operation: F,
    ) -> Result<T>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        let mut delay = Duration::from_secs(1);
        let mut attempts = 0;

        loop {
            attempts += 1;
            match operation().await {
                Ok(result) => return Ok(result),
                Err(e) if Self::should_retry(&e) && attempts <= max_retries => {
                    warn!(
                        "{} failed (attempt {}/{}): {}. Retrying in {:?}...",
                        operation_name, attempts, max_retries + 1, e, delay
                    );
                    tokio::time::sleep(delay).await;
                    delay = std::cmp::min(delay * 2, Duration::from_secs(30));
                }
                Err(e) => return Err(e),
            }
        }
    }
}

/// Parse Gmail API Message into our MessageMetadata structure
fn parse_message_metadata(msg: Message) -> Result<MessageMetadata> {
    let id = msg
        .id
        .ok_or_else(|| GmailError::InvalidMessageFormat("Missing message ID".to_string()))?;

    let thread_id = msg
        .thread_id
        .ok_or_else(|| GmailError::InvalidMessageFormat("Missing thread ID".to_string()))?;

    let labels = msg.label_ids.unwrap_or_default();

    // Parse headers
    let headers = msg
        .payload
        .as_ref()
        .and_then(|p| p.headers.as_ref())
        .ok_or_else(|| GmailError::InvalidMessageFormat("Missing headers".to_string()))?;

    let mut sender_email = String::new();
    let mut sender_name = String::new();
    let mut subject = String::new();
    let mut recipients = Vec::new();
    let mut date_str = String::new();
    let mut has_unsubscribe = false;

    for header in headers {
        if let (Some(name), Some(value)) = (&header.name, &header.value) {
            match name.to_lowercase().as_str() {
                "from" => {
                    // Parse "Name <email@example.com>" format
                    if let Some((name_part, email_part)) = parse_email_header(value) {
                        sender_name = name_part;
                        sender_email = email_part;
                    } else {
                        sender_email = value.clone();
                    }
                }
                "subject" => {
                    subject = value.clone();
                }
                "to" | "cc" => {
                    recipients.push(value.clone());
                }
                "date" => {
                    date_str = value.clone();
                }
                "list-unsubscribe" => {
                    has_unsubscribe = true;
                }
                _ => {}
            }
        }
    }

    // Extract sender domain
    let sender_domain = sender_email
        .split('@')
        .nth(1)
        .unwrap_or("")
        .to_string();

    // Parse date
    let date_received = parse_date(&date_str).unwrap_or_else(|_| Utc::now());

    // Check if automated
    let is_automated = check_if_automated(&sender_email, &sender_name, has_unsubscribe);

    Ok(MessageMetadata {
        id,
        thread_id,
        sender_email,
        sender_domain,
        sender_name,
        subject,
        recipients,
        date_received,
        labels,
        has_unsubscribe,
        is_automated,
    })
}

/// Parse email header in "Name <email@example.com>" format
fn parse_email_header(header: &str) -> Option<(String, String)> {
    if let Some(start) = header.find('<') {
        if let Some(end) = header.find('>') {
            let name = header[..start].trim().trim_matches('"').to_string();
            let email = header[start + 1..end].trim().to_string();
            return Some((name, email));
        }
    }
    None
}

/// Parse RFC 2822 date string
fn parse_date(date_str: &str) -> Result<DateTime<Utc>> {
    // Try parsing as RFC 2822 format
    DateTime::parse_from_rfc2822(date_str)
        .map(|dt| dt.with_timezone(&Utc))
        .or_else(|_| {
            // Fallback to RFC 3339 format
            DateTime::parse_from_rfc3339(date_str).map(|dt| dt.with_timezone(&Utc))
        })
        .map_err(|e| GmailError::InvalidMessageFormat(format!("Invalid date format: {}", e)))
}

/// Check if sender appears to be automated
fn check_if_automated(sender_email: &str, sender_name: &str, has_unsubscribe: bool) -> bool {
    let automated_keywords = [
        "noreply",
        "no-reply",
        "notification",
        "automated",
        "donotreply",
        "do-not-reply",
        "mailer",
        "robot",
    ];

    let email_lower = sender_email.to_lowercase();
    let name_lower = sender_name.to_lowercase();

    has_unsubscribe
        || automated_keywords
            .iter()
            .any(|&keyword| email_lower.contains(keyword) || name_lower.contains(keyword))
}

#[async_trait]
impl GmailClient for ProductionGmailClient {
    async fn list_message_ids(&self, query: &str) -> Result<Vec<String>> {
        let mut all_ids = Vec::new();
        let mut page_token: Option<String> = None;

        loop {
            let mut call = self
                .hub
                .users()
                .messages_list("me")
                .q(query)
                .max_results(100);

            if let Some(token) = page_token.as_ref() {
                call = call.page_token(token);
            }

            let (_, response) = call
                .add_scope("https://www.googleapis.com/auth/gmail.modify")
                .doit()
                .await?;

            if let Some(messages) = response.messages {
                for msg_ref in messages {
                    if let Some(id) = msg_ref.id {
                        all_ids.push(id);
                    }
                }
            }

            page_token = response.next_page_token;
            if page_token.is_none() {
                break;
            }
        }

        Ok(all_ids)
    }

    async fn get_message(&self, id: &str) -> Result<MessageMetadata> {
        self.fetch_single_with_retry(id).await
    }

    async fn list_labels(&self) -> Result<Vec<LabelInfo>> {
        Self::with_retry("list_labels", 3, || async {
            // Wrap API call in timeout to prevent indefinite hangs
            let timeout_duration = Duration::from_secs(30);
            let api_call = async {
                debug!("Calling Gmail API to list labels...");
                let result = self
                    .hub
                    .users()
                    .labels_list("me")
                    .add_scope("https://www.googleapis.com/auth/gmail.labels")
                    .doit()
                    .await;
                debug!("Gmail API list labels call completed");
                result
            };

            let (_, response) = match tokio::time::timeout(timeout_duration, api_call).await {
                Ok(result) => result?,
                Err(_) => {
                    warn!("Gmail API list_labels call timed out after {:?}", timeout_duration);
                    return Err(GmailError::NetworkError(
                        format!("API call timed out after {:?}", timeout_duration)
                    ));
                }
            };

            let labels: Vec<LabelInfo> = response
                .labels
                .unwrap_or_default()
                .into_iter()
                .filter_map(|label| {
                    match (label.id, label.name) {
                        (Some(id), Some(name)) => Some(LabelInfo { id, name }),
                        _ => None,
                    }
                })
                .collect();

            debug!("Successfully parsed {} labels", labels.len());
            Ok(labels)
        }).await
    }

    async fn create_label(&self, name: &str) -> Result<String> {
        let name = name.to_string();
        Self::with_retry("create_label", 3, || async {
            let label = Label {
                name: Some(name.clone()),
                message_list_visibility: Some("show".to_string()),
                label_list_visibility: Some("labelShow".to_string()),
                ..Default::default()
            };

            let (_, created_label) = self.hub.users().labels_create(label, "me")
                .add_scope("https://www.googleapis.com/auth/gmail.labels")
                .doit().await?;

            created_label
                .id
                .ok_or_else(|| GmailError::LabelError("Created label has no ID".to_string()))
        }).await
    }

    async fn delete_label(&self, label_id: &str) -> Result<()> {
        self.hub
            .users()
            .labels_delete("me", label_id)
            .add_scope("https://www.googleapis.com/auth/gmail.labels")
            .doit()
            .await?;

        Ok(())
    }

    async fn create_filter(&self, filter: &FilterRule) -> Result<String> {
        let filter = filter.clone();
        Self::with_retry("create_filter", 3, || async {
            // Build the full Gmail query including from pattern, exclusions, and subject keywords
            let full_query = crate::filter_manager::FilterManager::build_gmail_query_static(&filter);

            // Build the filter criteria
            let criteria = FilterCriteria {
                // Use the full query with all criteria (from, exclusions, subjects)
                query: Some(full_query),
                exclude_chats: Some(true),
                ..Default::default()
            };

            // Build the filter action
            let mut action = FilterAction {
                add_label_ids: Some(vec![filter.target_label_id.clone()]),
                ..Default::default()
            };

            // If should_archive, remove the INBOX label
            if filter.should_archive {
                action.remove_label_ids = Some(vec!["INBOX".to_string()]);
            }

            let gmail_filter = Filter {
                criteria: Some(criteria),
                action: Some(action),
                ..Default::default()
            };

            let (_, created_filter) = self
                .hub
                .users()
                .settings_filters_create(gmail_filter, "me")
                .add_scope("https://www.googleapis.com/auth/gmail.settings.basic")
                .doit()
                .await?;

            created_filter
                .id
                .ok_or_else(|| GmailError::FilterError("Created filter has no ID".to_string()))
        }).await
    }

    async fn list_filters(&self) -> Result<Vec<ExistingFilterInfo>> {
        Self::with_retry("list_filters", 3, || async {
            // Wrap API call in timeout to prevent indefinite hangs
            let timeout_duration = Duration::from_secs(30);
            let api_call = async {
                debug!("Calling Gmail API to list filters...");
                let result = self
                    .hub
                    .users()
                    .settings_filters_list("me")
                    .add_scope("https://www.googleapis.com/auth/gmail.settings.basic")
                    .doit()
                    .await;
                debug!("Gmail API list filters call completed");
                result
            };

            let (_, response) = match tokio::time::timeout(timeout_duration, api_call).await {
                Ok(result) => result?,
                Err(_) => {
                    warn!("Gmail API list_filters call timed out after {:?}", timeout_duration);
                    return Err(GmailError::NetworkError(
                        format!("API call timed out after {:?}", timeout_duration)
                    ));
                }
            };

            let filters: Vec<ExistingFilterInfo> = response
                .filter
                .unwrap_or_default()
                .into_iter()
                .filter_map(|f| {
                    let id = f.id?;
                    let criteria = f.criteria.unwrap_or_default();
                    let action = f.action.unwrap_or_default();

                    Some(ExistingFilterInfo {
                        id,
                        query: criteria.query,
                        from: criteria.from,
                        to: criteria.to,
                        subject: criteria.subject,
                        add_label_ids: action.add_label_ids.unwrap_or_default(),
                        remove_label_ids: action.remove_label_ids.unwrap_or_default(),
                    })
                })
                .collect();

            debug!("Successfully parsed {} filters", filters.len());
            Ok(filters)
        }).await
    }

    async fn delete_filter(&self, filter_id: &str) -> Result<()> {
        let filter_id = filter_id.to_string();
        Self::with_retry("delete_filter", 3, || async {
            self.hub
                .users()
                .settings_filters_delete("me", &filter_id)
                .add_scope("https://www.googleapis.com/auth/gmail.settings.basic")
                .doit()
                .await?;

            Ok(())
        }).await
    }

    async fn update_filter(&self, filter_id: &str, filter: &FilterRule) -> Result<String> {
        let filter_id = filter_id.to_string();
        let filter = filter.clone();
        Self::with_retry("update_filter", 3, || async {
            // Delete the old filter
            self.delete_filter(&filter_id).await?;

            // Create a new one with updated settings
            self.create_filter(&filter).await
        }).await
    }

    async fn apply_label(&self, message_id: &str, label_id: &str) -> Result<()> {
        let modify_request = ModifyMessageRequest {
            add_label_ids: Some(vec![label_id.to_string()]),
            remove_label_ids: None,
        };

        self.hub
            .users()
            .messages_modify(modify_request, "me", message_id)
            .add_scope("https://www.googleapis.com/auth/gmail.modify")
            .doit()
            .await?;

        Ok(())
    }

    async fn remove_label(&self, message_id: &str, label_id: &str) -> Result<()> {
        let modify_request = ModifyMessageRequest {
            add_label_ids: None,
            remove_label_ids: Some(vec![label_id.to_string()]),
        };

        self.hub
            .users()
            .messages_modify(modify_request, "me", message_id)
            .add_scope("https://www.googleapis.com/auth/gmail.modify")
            .doit()
            .await?;

        Ok(())
    }

    async fn batch_remove_label(&self, message_ids: &[String], label_id: &str) -> Result<usize> {
        if message_ids.is_empty() {
            return Ok(0);
        }

        // Gmail API allows up to 1000 messages per batch request
        const BATCH_SIZE: usize = 1000;
        let mut total_modified = 0;
        let label_id = label_id.to_string();

        for chunk in message_ids.chunks(BATCH_SIZE) {
            let chunk_vec = chunk.to_vec();
            let label_id_clone = label_id.clone();

            Self::with_retry("batch_remove_label", 3, || async {
                let request = BatchModifyMessagesRequest {
                    ids: Some(chunk_vec.clone()),
                    add_label_ids: None,
                    remove_label_ids: Some(vec![label_id_clone.clone()]),
                };

                self.hub
                    .users()
                    .messages_batch_modify(request, "me")
                    .add_scope("https://www.googleapis.com/auth/gmail.modify")
                    .doit()
                    .await?;

                Ok(())
            }).await?;

            total_modified += chunk.len();
        }

        Ok(total_modified)
    }

    async fn batch_add_label(&self, message_ids: &[String], label_id: &str) -> Result<usize> {
        if message_ids.is_empty() {
            return Ok(0);
        }

        const BATCH_SIZE: usize = 1000;
        let mut total_modified = 0;
        let label_id = label_id.to_string();

        for chunk in message_ids.chunks(BATCH_SIZE) {
            let chunk_vec = chunk.to_vec();
            let label_id_clone = label_id.clone();

            Self::with_retry("batch_add_label", 3, || async {
                let request = BatchModifyMessagesRequest {
                    ids: Some(chunk_vec.clone()),
                    add_label_ids: Some(vec![label_id_clone.clone()]),
                    remove_label_ids: None,
                };

                self.hub
                    .users()
                    .messages_batch_modify(request, "me")
                    .add_scope("https://www.googleapis.com/auth/gmail.modify")
                    .doit()
                    .await?;

                Ok(())
            }).await?;

            total_modified += chunk.len();
        }

        Ok(total_modified)
    }

    async fn batch_modify_labels(
        &self,
        message_ids: &[String],
        add_label_ids: &[String],
        remove_label_ids: &[String],
    ) -> Result<usize> {
        if message_ids.is_empty() {
            return Ok(0);
        }

        const BATCH_SIZE: usize = 1000;
        let mut total_modified = 0;

        let add_labels = if add_label_ids.is_empty() {
            None
        } else {
            Some(add_label_ids.to_vec())
        };

        let remove_labels = if remove_label_ids.is_empty() {
            None
        } else {
            Some(remove_label_ids.to_vec())
        };

        for chunk in message_ids.chunks(BATCH_SIZE) {
            let chunk_vec = chunk.to_vec();
            let add_labels_clone = add_labels.clone();
            let remove_labels_clone = remove_labels.clone();

            Self::with_retry("batch_modify_labels", 3, || async {
                let request = BatchModifyMessagesRequest {
                    ids: Some(chunk_vec.clone()),
                    add_label_ids: add_labels_clone.clone(),
                    remove_label_ids: remove_labels_clone.clone(),
                };

                self.hub
                    .users()
                    .messages_batch_modify(request, "me")
                    .add_scope("https://www.googleapis.com/auth/gmail.modify")
                    .doit()
                    .await?;

                Ok(())
            }).await?;

            total_modified += chunk.len();
        }

        Ok(total_modified)
    }

    async fn fetch_messages_batch(&self, message_ids: Vec<String>) -> Result<Vec<MessageMetadata>> {
        // Note: fetch_single_with_retry already handles rate limiting via semaphore
        // buffer_unordered limits concurrency to 40 parallel requests
        stream::iter(message_ids)
            .map(|id| {
                let client = self;
                async move {
                    client.fetch_single_with_retry(&id).await
                }
            })
            .buffer_unordered(40)
            .try_collect()
            .await
    }

    async fn fetch_messages_with_progress(
        &self,
        message_ids: Vec<String>,
        on_progress: ProgressCallback,
    ) -> Result<Vec<MessageMetadata>> {
        let results = tokio::sync::Mutex::new(Vec::with_capacity(message_ids.len()));

        stream::iter(message_ids)
            .map(|id| {
                let client = self;
                let on_progress = Arc::clone(&on_progress);
                async move {
                    let msg = client.fetch_single_with_retry(&id).await?;
                    on_progress();
                    Ok::<_, GmailError>(msg)
                }
            })
            .buffer_unordered(40)
            .try_for_each(|msg| {
                let results = &results;
                async move {
                    results.lock().await.push(msg);
                    Ok(())
                }
            })
            .await?;

        Ok(results.into_inner())
    }
}

// Implement GmailClient for Arc<ProductionGmailClient> to allow shared ownership
#[async_trait]
impl GmailClient for Arc<ProductionGmailClient> {
    async fn list_message_ids(&self, query: &str) -> Result<Vec<String>> {
        self.as_ref().list_message_ids(query).await
    }

    async fn get_message(&self, id: &str) -> Result<MessageMetadata> {
        self.as_ref().get_message(id).await
    }

    async fn list_labels(&self) -> Result<Vec<LabelInfo>> {
        self.as_ref().list_labels().await
    }

    async fn create_label(&self, name: &str) -> Result<String> {
        self.as_ref().create_label(name).await
    }

    async fn delete_label(&self, label_id: &str) -> Result<()> {
        self.as_ref().delete_label(label_id).await
    }

    async fn create_filter(&self, filter: &FilterRule) -> Result<String> {
        self.as_ref().create_filter(filter).await
    }

    async fn list_filters(&self) -> Result<Vec<ExistingFilterInfo>> {
        self.as_ref().list_filters().await
    }

    async fn delete_filter(&self, filter_id: &str) -> Result<()> {
        self.as_ref().delete_filter(filter_id).await
    }

    async fn update_filter(&self, filter_id: &str, filter: &FilterRule) -> Result<String> {
        self.as_ref().update_filter(filter_id, filter).await
    }

    async fn apply_label(&self, message_id: &str, label_id: &str) -> Result<()> {
        self.as_ref().apply_label(message_id, label_id).await
    }

    async fn remove_label(&self, message_id: &str, label_id: &str) -> Result<()> {
        self.as_ref().remove_label(message_id, label_id).await
    }

    async fn batch_remove_label(&self, message_ids: &[String], label_id: &str) -> Result<usize> {
        self.as_ref().batch_remove_label(message_ids, label_id).await
    }

    async fn batch_add_label(&self, message_ids: &[String], label_id: &str) -> Result<usize> {
        self.as_ref().batch_add_label(message_ids, label_id).await
    }

    async fn batch_modify_labels(
        &self,
        message_ids: &[String],
        add_label_ids: &[String],
        remove_label_ids: &[String],
    ) -> Result<usize> {
        self.as_ref().batch_modify_labels(message_ids, add_label_ids, remove_label_ids).await
    }

    async fn fetch_messages_batch(&self, message_ids: Vec<String>) -> Result<Vec<MessageMetadata>> {
        self.as_ref().fetch_messages_batch(message_ids).await
    }

    async fn fetch_messages_with_progress(
        &self,
        message_ids: Vec<String>,
        on_progress: ProgressCallback,
    ) -> Result<Vec<MessageMetadata>> {
        self.as_ref().fetch_messages_with_progress(message_ids, on_progress).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_retry_server_error() {
        let error = GmailError::ServerError {
            status: 500,
            message: "Internal error".to_string(),
        };
        assert!(ProductionGmailClient::should_retry(&error));
    }

    #[test]
    fn test_should_retry_rate_limit() {
        let error = GmailError::RateLimitExceeded { retry_after: 5 };
        assert!(ProductionGmailClient::should_retry(&error));
    }

    #[test]
    fn test_should_retry_network_error() {
        let error = GmailError::NetworkError("connection reset".to_string());
        assert!(ProductionGmailClient::should_retry(&error));
    }

    #[test]
    fn test_should_not_retry_auth_error() {
        let error = GmailError::AuthError("invalid token".to_string());
        assert!(!ProductionGmailClient::should_retry(&error));
    }

    #[test]
    fn test_should_not_retry_filter_error() {
        let error = GmailError::FilterError("invalid filter".to_string());
        assert!(!ProductionGmailClient::should_retry(&error));
    }

    #[test]
    fn test_parse_email_header() {
        let result = parse_email_header("John Doe <john@example.com>");
        assert_eq!(result, Some(("John Doe".to_string(), "john@example.com".to_string())));

        let result = parse_email_header("\"Jane Smith\" <jane@example.com>");
        assert_eq!(result, Some(("Jane Smith".to_string(), "jane@example.com".to_string())));

        let result = parse_email_header("plain@example.com");
        assert_eq!(result, None);
    }

    #[test]
    fn test_check_if_automated() {
        assert!(check_if_automated("noreply@example.com", "", false));
        assert!(check_if_automated("", "Automated System", false));
        assert!(check_if_automated("user@example.com", "John", true));
        assert!(!check_if_automated("user@example.com", "John", false));
    }

    #[test]
    fn test_parse_date() {
        let date_str = "Mon, 24 Nov 2025 10:30:00 +0000";
        let result = parse_date(date_str);
        assert!(result.is_ok());
    }

    // Test retry logic for list_filters
    #[tokio::test]
    async fn test_with_retry_succeeds_after_transient_error() {
        use std::sync::atomic::{AtomicU32, Ordering};
        use std::sync::Arc;

        let attempt_count = Arc::new(AtomicU32::new(0));
        let attempt_count_clone = Arc::clone(&attempt_count);

        let result = ProductionGmailClient::with_retry("test_op", 3, || {
            let count = Arc::clone(&attempt_count_clone);
            async move {
                let current = count.fetch_add(1, Ordering::SeqCst);
                if current < 2 {
                    // First two attempts fail with transient error
                    Err(GmailError::NetworkError("Connection timeout".to_string()))
                } else {
                    // Third attempt succeeds
                    Ok("success".to_string())
                }
            }
        })
        .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "success");
        assert_eq!(attempt_count.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn test_with_retry_fails_on_permanent_error() {
        use std::sync::atomic::{AtomicU32, Ordering};
        use std::sync::Arc;

        let attempt_count = Arc::new(AtomicU32::new(0));
        let attempt_count_clone = Arc::clone(&attempt_count);

        let result = ProductionGmailClient::with_retry("test_op", 3, || {
            let count = Arc::clone(&attempt_count_clone);
            async move {
                count.fetch_add(1, Ordering::SeqCst);
                // Permanent error - should not retry
                Err::<String, _>(GmailError::AuthError("Invalid credentials".to_string()))
            }
        })
        .await;

        assert!(result.is_err());
        // Should only attempt once, no retries for permanent errors
        assert_eq!(attempt_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_with_retry_exhausts_all_retries() {
        use std::sync::atomic::{AtomicU32, Ordering};
        use std::sync::Arc;

        let attempt_count = Arc::new(AtomicU32::new(0));
        let attempt_count_clone = Arc::clone(&attempt_count);

        let result = ProductionGmailClient::with_retry("test_op", 3, || {
            let count = Arc::clone(&attempt_count_clone);
            async move {
                count.fetch_add(1, Ordering::SeqCst);
                // Always fail with transient error
                Err::<String, _>(GmailError::RateLimitExceeded { retry_after: 1 })
            }
        })
        .await;

        assert!(result.is_err());
        // Should attempt 4 times: initial + 3 retries
        assert_eq!(attempt_count.load(Ordering::SeqCst), 4);
    }

    #[tokio::test]
    async fn test_with_retry_succeeds_immediately() {
        use std::sync::atomic::{AtomicU32, Ordering};
        use std::sync::Arc;

        let attempt_count = Arc::new(AtomicU32::new(0));
        let attempt_count_clone = Arc::clone(&attempt_count);

        let result = ProductionGmailClient::with_retry("test_op", 3, || {
            let count = Arc::clone(&attempt_count_clone);
            async move {
                count.fetch_add(1, Ordering::SeqCst);
                Ok("success".to_string())
            }
        })
        .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "success");
        // Should only attempt once
        assert_eq!(attempt_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_timeout_triggers_network_error() {
        use tokio::time::{sleep, Duration};

        // Simulate a slow operation that exceeds timeout
        let timeout_duration = Duration::from_millis(100);
        let slow_operation = async {
            sleep(Duration::from_millis(200)).await;
            Ok::<String, GmailError>("too slow".to_string())
        };

        let result = tokio::time::timeout(timeout_duration, slow_operation).await;

        // Should timeout
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_timeout_completes_within_limit() {
        use tokio::time::{sleep, Duration};

        // Simulate a fast operation that completes before timeout
        let timeout_duration = Duration::from_millis(100);
        let fast_operation = async {
            sleep(Duration::from_millis(10)).await;
            Ok::<String, GmailError>("fast enough".to_string())
        };

        let result = tokio::time::timeout(timeout_duration, fast_operation).await;

        // Should succeed
        assert!(result.is_ok());
        assert!(result.unwrap().is_ok());
    }
}
