//! Email scanner for retrieving historical messages with concurrent fetching and checkpointing

use crate::error::{GmailError, Result};
use crate::models::MessageMetadata;
use chrono::{DateTime, Duration, Utc};
use google_gmail1::api::Message;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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

    let has_unsubscribe =
        headers.contains_key("List-Unsubscribe") || headers.contains_key("List-Unsubscribe-Post");

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
