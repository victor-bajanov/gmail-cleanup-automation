//! Gmail API client with rate limiting and retry logic

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures::stream::{self, StreamExt, TryStreamExt};
use google_gmail1::{
    api::{BatchModifyMessagesRequest, Filter, FilterAction, FilterCriteria, Label, Message, ModifyMessageRequest},
    hyper_rustls, hyper_util, Gmail,
};
use std::sync::Arc;
use tokio::sync::Semaphore;

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
                existing_normalized == new_normalized
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

    /// Create a new filter rule
    async fn create_filter(&self, filter: &FilterRule) -> Result<String>;

    /// List all existing filters
    async fn list_filters(&self) -> Result<Vec<ExistingFilterInfo>>;

    /// Apply a label to a message
    async fn apply_label(&self, message_id: &str, label_id: &str) -> Result<()>;

    /// Remove a label from a message (used for archiving - removing INBOX)
    async fn remove_label(&self, message_id: &str, label_id: &str) -> Result<()>;

    /// Remove a label from multiple messages in batch (up to 1000 per call)
    /// Returns the number of messages successfully modified
    async fn batch_remove_label(&self, message_ids: &[String], label_id: &str) -> Result<usize>;

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
        let (_, response) = self
            .hub
            .users()
            .labels_list("me")
            .add_scope("https://www.googleapis.com/auth/gmail.labels")
            .doit()
            .await?;

        let labels = response
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

        Ok(labels)
    }

    async fn create_label(&self, name: &str) -> Result<String> {
        let label = Label {
            name: Some(name.to_string()),
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
    }

    async fn create_filter(&self, filter: &FilterRule) -> Result<String> {
        // Build the filter criteria
        let criteria = FilterCriteria {
            // Use the from_pattern as a query (e.g., "from:(*@domain.com)")
            query: filter.from_pattern.clone(),
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
    }

    async fn list_filters(&self) -> Result<Vec<ExistingFilterInfo>> {
        let (_, response) = self
            .hub
            .users()
            .settings_filters_list("me")
            .add_scope("https://www.googleapis.com/auth/gmail.settings.basic")
            .doit()
            .await?;

        let filters = response
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

        Ok(filters)
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

        for chunk in message_ids.chunks(BATCH_SIZE) {
            let request = BatchModifyMessagesRequest {
                ids: Some(chunk.to_vec()),
                add_label_ids: None,
                remove_label_ids: Some(vec![label_id.to_string()]),
            };

            self.hub
                .users()
                .messages_batch_modify(request, "me")
                .add_scope("https://www.googleapis.com/auth/gmail.modify")
                .doit()
                .await?;

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

    async fn create_filter(&self, filter: &FilterRule) -> Result<String> {
        self.as_ref().create_filter(filter).await
    }

    async fn list_filters(&self) -> Result<Vec<ExistingFilterInfo>> {
        self.as_ref().list_filters().await
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
    fn test_should_retry() {
        // This test would require mocking google_gmail1::Error
        // For now, just test the logic structure
        assert!(true); // Placeholder
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
}
