//! Email scanner for retrieving historical messages with concurrent fetching and checkpointing

use crate::client::GmailClient;
use crate::error::{GmailError, Result};
use crate::models::MessageMetadata;
use async_stream::stream;
use chrono::{DateTime, Duration, Utc};
use futures::stream::{Stream, StreamExt};
use google_gmail1::api::Message;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::pin::Pin;
use tracing::{debug, info, warn};

/// Message format options for Gmail API
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageFormat {
    /// Only message ID and thread ID
    Minimal,
    /// ID, thread ID, label IDs, snippet, history ID, internal date, size estimate
    Metadata,
    /// Full message data including headers and body
    Full,
}

impl MessageFormat {
    /// Get format string for API call (lines 348-380)
    pub fn as_str(&self) -> &'static str {
        match self {
            MessageFormat::Minimal => "minimal",
            MessageFormat::Metadata => "metadata",
            MessageFormat::Full => "full",
        }
    }

    /// Get fields to include for this format
    pub fn fields(&self) -> Vec<&'static str> {
        match self {
            MessageFormat::Minimal => vec!["id", "threadId"],
            MessageFormat::Metadata => vec![
                "id",
                "threadId",
                "labelIds",
                "snippet",
                "historyId",
                "internalDate",
                "sizeEstimate",
            ],
            MessageFormat::Full => vec!["id", "threadId", "labelIds", "snippet", "payload", "raw"],
        }
    }

    /// Get partial response field selector for metadata
    pub fn partial_fields(&self) -> String {
        match self {
            MessageFormat::Minimal => "id,threadId".to_string(),
            MessageFormat::Metadata => {
                "id,threadId,labelIds,snippet,historyId,internalDate,sizeEstimate,payload/headers"
                    .to_string()
            }
            MessageFormat::Full => "".to_string(), // Full doesn't use partial
        }
    }
}

/// Checkpoint for resuming email scanning
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanCheckpoint {
    pub page_token: Option<String>,
    pub messages_processed: usize,
    pub last_message_id: Option<String>,
    pub timestamp: DateTime<Utc>,
}

impl ScanCheckpoint {
    pub fn new() -> Self {
        Self {
            page_token: None,
            messages_processed: 0,
            last_message_id: None,
            timestamp: Utc::now(),
        }
    }

    pub fn update(&mut self, page_token: Option<String>, message_id: Option<String>) {
        self.page_token = page_token;
        if let Some(id) = message_id {
            self.last_message_id = Some(id);
            self.messages_processed += 1;
        }
        self.timestamp = Utc::now();
    }

    /// Load checkpoint from file
    pub async fn load(path: &str) -> Result<Option<Self>> {
        match tokio::fs::read_to_string(path).await {
            Ok(content) => {
                let checkpoint = serde_json::from_str(&content)
                    .map_err(|e| GmailError::ConfigError(format!("Invalid checkpoint: {}", e)))?;
                Ok(Some(checkpoint))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Save checkpoint to file
    pub async fn save(&self, path: &str) -> Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        tokio::fs::write(path, json).await?;
        Ok(())
    }
}

impl Default for ScanCheckpoint {
    fn default() -> Self {
        Self::new()
    }
}

/// Configuration for email scanning
#[derive(Debug, Clone)]
pub struct ScanConfig {
    pub query: Option<String>,
    pub max_results: Option<i32>,
    pub label_ids: Vec<String>,
    pub include_spam_trash: bool,
    pub format: MessageFormat,
    pub concurrent_fetches: usize,
    pub page_size: u32,
}

impl Default for ScanConfig {
    fn default() -> Self {
        Self {
            query: None,
            max_results: Some(500),
            label_ids: Vec::new(),
            include_spam_trash: false,
            format: MessageFormat::Metadata,
            concurrent_fetches: 10,
            page_size: 100,
        }
    }
}

impl ScanConfig {
    /// Create config for scanning date range
    pub fn for_period(days: u32) -> Self {
        let date = Utc::now() - Duration::days(days as i64);
        let date_str = date.format("%Y/%m/%d").to_string();
        let query = format!("after:{}", date_str);

        Self {
            query: Some(query),
            ..Default::default()
        }
    }
}

/// Email scanner for fetching messages from Gmail
pub struct EmailScanner {
    client: Box<dyn GmailClient>,
    #[allow(dead_code)]
    hub: Option<
        google_gmail1::Gmail<
            hyper_rustls::HttpsConnector<hyper_util::client::legacy::connect::HttpConnector>,
        >,
    >,
    #[allow(dead_code)]
    user_id: String,
}

impl EmailScanner {
    pub fn new(client: Box<dyn GmailClient>) -> Self {
        Self {
            client,
            hub: None,
            user_id: "me".to_string(),
        }
    }

    /// Create scanner with direct Gmail hub access for advanced features
    pub fn with_hub(
        client: Box<dyn GmailClient>,
        hub: google_gmail1::Gmail<
            hyper_rustls::HttpsConnector<hyper_util::client::legacy::connect::HttpConnector>,
        >,
    ) -> Self {
        Self {
            client,
            hub: Some(hub),
            user_id: "me".to_string(),
        }
    }

    /// Get message format selection for API call (lines 348-380)
    pub fn get_format_param(&self, format: MessageFormat) -> &'static str {
        format.as_str()
    }

    /// Apply format to message retrieval
    pub fn select_message_format(&self, format: MessageFormat) -> Vec<&'static str> {
        format.fields()
    }

    /// Get metadata fields for partial response
    pub fn get_metadata_fields(&self) -> String {
        MessageFormat::Metadata.partial_fields()
    }

    /// Fetch messages concurrently using buffer_unordered (lines 216-237)
    pub async fn fetch_messages_concurrent(
        &self,
        message_ids: Vec<String>,
        _format: MessageFormat,
    ) -> Result<Vec<MessageMetadata>> {
        use futures::stream;

        info!(
            "Fetching {} messages with {} concurrent workers",
            message_ids.len(),
            10
        );

        let messages_stream = stream::iter(message_ids)
            .map(|msg_id| {
                let client = &self.client;
                async move {
                    debug!("Fetching message: {}", msg_id);
                    match client.get_message(&msg_id).await {
                        Ok(message) => {
                            debug!("Successfully fetched message: {}", msg_id);
                            Ok(message)
                        }
                        Err(e) => {
                            warn!("Failed to fetch message {}: {}", msg_id, e);
                            Err(e)
                        }
                    }
                }
            })
            .buffer_unordered(10);

        let results: Vec<Result<MessageMetadata>> = messages_stream.collect().await;

        // Separate successes from failures
        let mut messages = Vec::new();
        let mut errors = Vec::new();

        for result in results {
            match result {
                Ok(msg) => messages.push(msg),
                Err(e) => errors.push(e),
            }
        }

        if !errors.is_empty() {
            warn!("Failed to fetch {} messages", errors.len());
        }

        info!("Successfully fetched {} messages", messages.len());
        Ok(messages)
    }

    /// List message IDs with pagination
    async fn list_message_ids_page(
        &self,
        config: &ScanConfig,
        _page_token: Option<String>,
    ) -> Result<(Vec<String>, Option<String>)> {
        // Build query string
        let mut query_parts = vec![];

        if let Some(q) = &config.query {
            query_parts.push(q.clone());
        }

        for label_id in &config.label_ids {
            query_parts.push(format!("label:{}", label_id));
        }

        let query = if query_parts.is_empty() {
            "in:anywhere".to_string()
        } else {
            query_parts.join(" ")
        };

        // Use client to list message IDs
        // In a real implementation, this would handle pagination
        let ids = self.client.list_message_ids(&query).await?;

        // For now, return all IDs without pagination
        // A full implementation would handle page_token properly
        Ok((ids, None))
    }

    /// Scan all messages with pagination
    pub async fn scan_all_messages(&self, config: &ScanConfig) -> Result<Vec<MessageMetadata>> {
        let mut all_messages = Vec::new();
        let mut page_token: Option<String> = None;

        loop {
            let (message_ids, next_token) =
                self.list_message_ids_page(config, page_token).await?;

            if message_ids.is_empty() {
                break;
            }

            info!("Processing page with {} message IDs", message_ids.len());

            // Fetch messages concurrently
            let messages = self
                .fetch_messages_concurrent(message_ids, config.format)
                .await?;

            all_messages.extend(messages);

            page_token = next_token;
            if page_token.is_none() {
                break;
            }
        }

        info!("Scan complete: {} total messages", all_messages.len());
        Ok(all_messages)
    }

    /// Scan messages with checkpoints - streaming function (lines 1020-1100)
    /// Returns a stream that yields messages and periodically emits checkpoints
    pub fn scan_messages_with_checkpoints<'a>(
        &'a self,
        config: ScanConfig,
        mut checkpoint: ScanCheckpoint,
        checkpoint_interval: usize,
    ) -> Pin<Box<dyn Stream<Item = Result<ScanResult>> + Send + 'a>> {
        Box::pin(stream! {
            let mut page_token = checkpoint.page_token.clone();
            let mut messages_since_checkpoint = 0;

            info!(
                "Starting scan with checkpoint. Already processed: {}, page_token: {:?}",
                checkpoint.messages_processed, page_token
            );

            loop {
                // Fetch the next page of message IDs
                let page_result = self.list_message_ids_page(&config, page_token.clone()).await;

                match page_result {
                    Ok((message_ids, next_token)) => {
                        if message_ids.is_empty() {
                            info!("No more messages to process");

                            // Emit final checkpoint
                            checkpoint.update(None, None);
                            yield Ok(ScanResult::Checkpoint(checkpoint.clone()));
                            break;
                        }

                        info!("Processing page with {} message IDs", message_ids.len());

                        // Fetch messages concurrently
                        let messages_result = self
                            .fetch_messages_concurrent(message_ids, config.format)
                            .await;

                        match messages_result {
                            Ok(messages) => {
                                // Process each message
                                for message in messages {
                                    let message_id = message.id.clone();

                                    // Yield the message
                                    yield Ok(ScanResult::Message(message));

                                    // Update checkpoint tracking
                                    messages_since_checkpoint += 1;

                                    // Emit checkpoint if interval reached
                                    if messages_since_checkpoint >= checkpoint_interval {
                                        checkpoint.update(next_token.clone(), Some(message_id.clone()));

                                        debug!(
                                            "Emitting checkpoint: processed={}, page_token={:?}",
                                            checkpoint.messages_processed, checkpoint.page_token
                                        );

                                        yield Ok(ScanResult::Checkpoint(checkpoint.clone()));
                                        messages_since_checkpoint = 0;
                                    }
                                }

                                // Move to next page
                                page_token = next_token;

                                // If no more pages, break
                                if page_token.is_none() {
                                    info!("Reached end of messages");

                                    // Emit final checkpoint
                                    checkpoint.update(None, None);
                                    yield Ok(ScanResult::Checkpoint(checkpoint.clone()));
                                    break;
                                }
                            }
                            Err(e) => {
                                warn!("Error fetching messages: {}", e);
                                yield Err(e);
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        warn!("Error listing messages: {}", e);
                        yield Err(e);
                        break;
                    }
                }
            }

            info!(
                "Scan complete. Total messages processed: {}",
                checkpoint.messages_processed
            );
        })
    }

    /// Scan messages with checkpoint resumption
    pub async fn scan_with_resume(
        &self,
        config: ScanConfig,
        checkpoint_file: Option<&str>,
        checkpoint_interval: usize,
    ) -> Result<Vec<MessageMetadata>> {
        let checkpoint = if let Some(path) = checkpoint_file {
            ScanCheckpoint::load(path).await?.unwrap_or_default()
        } else {
            ScanCheckpoint::new()
        };

        let mut stream =
            self.scan_messages_with_checkpoints(config, checkpoint, checkpoint_interval);
        let mut messages = Vec::new();

        while let Some(result) = stream.next().await {
            match result? {
                ScanResult::Message(msg) => {
                    messages.push(msg);
                }
                ScanResult::Checkpoint(cp) => {
                    // Save checkpoint to file if specified
                    if let Some(path) = checkpoint_file {
                        cp.save(path).await?;
                        debug!("Saved checkpoint to {}", path);
                    }
                }
            }
        }

        info!("Scan completed with {} messages", messages.len());
        Ok(messages)
    }

    /// Scan emails from the last N days
    pub async fn scan_period(&self, days: u32) -> Result<Vec<MessageMetadata>> {
        let config = ScanConfig::for_period(days);
        self.scan_all_messages(&config).await
    }

    /// Get a single message by ID
    pub async fn get_message(&self, id: &str) -> Result<MessageMetadata> {
        self.client.get_message(id).await
    }
}

/// Result type for streaming scan
#[derive(Debug)]
pub enum ScanResult {
    Message(MessageMetadata),
    Checkpoint(ScanCheckpoint),
}

/// Parse Gmail API Message to MessageMetadata
pub fn parse_message_metadata(message: &Message) -> Result<MessageMetadata> {
    let id = message
        .id
        .clone()
        .ok_or_else(|| GmailError::InvalidMessageFormat("Missing message ID".to_string()))?;

    let thread_id = message
        .thread_id
        .clone()
        .ok_or_else(|| GmailError::InvalidMessageFormat("Missing thread ID".to_string()))?;

    let headers = get_headers_map(message);

    let sender_email = headers
        .get("From")
        .and_then(|from| extract_sender_email(from))
        .unwrap_or_default();

    let sender_domain = extract_domain(&sender_email).unwrap_or_default();

    let sender_name = headers
        .get("From")
        .and_then(|from| extract_sender_name(from))
        .unwrap_or_else(|| sender_email.clone());

    let subject = headers.get("Subject").cloned().unwrap_or_default();

    let recipients = headers
        .get("To")
        .map(|to| extract_recipients(to))
        .unwrap_or_default();

    let date_received = headers
        .get("Date")
        .and_then(|date_str| parse_email_date(date_str))
        .unwrap_or_else(Utc::now);

    let labels = message.label_ids.clone().unwrap_or_default();

    let has_unsubscribe = headers.contains_key("List-Unsubscribe")
        || headers.contains_key("List-Unsubscribe-Post");

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
        is_automated: false, // Will be determined by classifier
    })
}

/// Extract header value from message
pub fn get_header_value(message: &Message, header_name: &str) -> Option<String> {
    message
        .payload
        .as_ref()?
        .headers
        .as_ref()?
        .iter()
        .find(|h| h.name.as_deref() == Some(header_name))
        .and_then(|h| h.value.clone())
}

/// Get all headers as a map
pub fn get_headers_map(message: &Message) -> HashMap<String, String> {
    let mut headers = HashMap::new();

    if let Some(payload) = &message.payload {
        if let Some(header_list) = &payload.headers {
            for header in header_list {
                if let (Some(name), Some(value)) = (&header.name, &header.value) {
                    headers.insert(name.clone(), value.clone());
                }
            }
        }
    }

    headers
}

/// Extract email address from From header
pub fn extract_sender_email(from_header: &str) -> Option<String> {
    let re = Regex::new(r"<?([a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,})>?").ok()?;
    re.captures(from_header)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str().to_lowercase())
}

/// Extract sender name from From header
pub fn extract_sender_name(from_header: &str) -> Option<String> {
    // Try to extract name before <email>
    if let Some(pos) = from_header.find('<') {
        let name = from_header[..pos].trim();
        if !name.is_empty() {
            return Some(name.trim_matches('"').to_string());
        }
    }
    None
}

/// Extract domain from email address
pub fn extract_domain(email: &str) -> Option<String> {
    email.split('@').nth(1).map(|d| d.to_lowercase())
}

/// Extract recipient emails from To header
pub fn extract_recipients(to_header: &str) -> Vec<String> {
    let re = Regex::new(r"([a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,})").unwrap();
    re.captures_iter(to_header)
        .filter_map(|cap| cap.get(1))
        .map(|m| m.as_str().to_lowercase())
        .collect()
}

/// Parse email date header
pub fn parse_email_date(date_str: &str) -> Option<DateTime<Utc>> {
    // Try RFC 2822 format first
    if let Ok(dt) = chrono::DateTime::parse_from_rfc2822(date_str) {
        return Some(dt.with_timezone(&Utc));
    }

    // Try RFC 3339 format
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(date_str) {
        return Some(dt.with_timezone(&Utc));
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_format() {
        assert_eq!(MessageFormat::Minimal.as_str(), "minimal");
        assert_eq!(MessageFormat::Metadata.as_str(), "metadata");
        assert_eq!(MessageFormat::Full.as_str(), "full");
    }

    #[test]
    fn test_extract_sender_email() {
        assert_eq!(
            extract_sender_email("John Doe <john@example.com>"),
            Some("john@example.com".to_string())
        );
        assert_eq!(
            extract_sender_email("john@example.com"),
            Some("john@example.com".to_string())
        );
        assert_eq!(
            extract_sender_email("<admin@test.org>"),
            Some("admin@test.org".to_string())
        );
    }

    #[test]
    fn test_extract_sender_name() {
        assert_eq!(
            extract_sender_name("John Doe <john@example.com>"),
            Some("John Doe".to_string())
        );
        assert_eq!(
            extract_sender_name("\"Jane Smith\" <jane@example.com>"),
            Some("Jane Smith".to_string())
        );
        assert_eq!(extract_sender_name("admin@test.org"), None);
    }

    #[test]
    fn test_extract_domain() {
        assert_eq!(
            extract_domain("user@example.com"),
            Some("example.com".to_string())
        );
        assert_eq!(
            extract_domain("admin@mail.google.com"),
            Some("mail.google.com".to_string())
        );
        assert_eq!(extract_domain("invalid"), None);
    }

    #[test]
    fn test_extract_recipients() {
        let recipients = extract_recipients("user1@example.com, user2@test.org");
        assert_eq!(recipients.len(), 2);
        assert!(recipients.contains(&"user1@example.com".to_string()));
        assert!(recipients.contains(&"user2@test.org".to_string()));
    }

    #[test]
    fn test_checkpoint_update() {
        let mut checkpoint = ScanCheckpoint::new();
        assert_eq!(checkpoint.messages_processed, 0);

        checkpoint.update(Some("token123".to_string()), Some("msg1".to_string()));
        assert_eq!(checkpoint.messages_processed, 1);
        assert_eq!(checkpoint.page_token, Some("token123".to_string()));
        assert_eq!(checkpoint.last_message_id, Some("msg1".to_string()));
    }

    #[test]
    fn test_scan_config_for_period() {
        let config = ScanConfig::for_period(7);
        assert!(config.query.is_some());
        assert!(config.query.unwrap().starts_with("after:"));
    }
}
