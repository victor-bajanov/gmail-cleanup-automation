//! Common test utilities and fixtures

use chrono::Utc;
use gmail_automation::client::GmailClient;
use gmail_automation::error::Result;
use gmail_automation::models::{EmailCategory, FilterRule, MessageMetadata};
use mockall::mock;
use mockall::predicate::*;
use serde_json::json;

/// Create a test message with default values
pub fn create_test_message(id: &str, sender: &str, subject: &str) -> MessageMetadata {
    let domain = sender
        .split('@')
        .nth(1)
        .unwrap_or("example.com")
        .to_string();

    MessageMetadata {
        id: id.to_string(),
        thread_id: format!("thread_{}", id),
        sender_email: sender.to_string(),
        sender_domain: domain,
        sender_name: "Test Sender".to_string(),
        subject: subject.to_string(),
        recipients: vec!["me@example.com".to_string()],
        date_received: Utc::now(),
        labels: vec!["INBOX".to_string()],
        has_unsubscribe: false,
        is_automated: false,
    }
}

/// Create a test message with automated sender
pub fn create_automated_message(id: &str, sender: &str, subject: &str) -> MessageMetadata {
    let mut message = create_test_message(id, sender, subject);
    message.is_automated = true;
    message.has_unsubscribe = true;
    message
}

/// Create a newsletter message
pub fn create_newsletter_message(id: &str, sender: &str) -> MessageMetadata {
    create_automated_message(id, sender, "Weekly Newsletter - Tech Updates")
}

/// Create a receipt message
pub fn create_receipt_message(id: &str) -> MessageMetadata {
    create_automated_message(id, "noreply@payments.com", "Your Receipt #12345")
}

/// Convert category to label name
pub fn category_to_label(category: &EmailCategory) -> String {
    match category {
        EmailCategory::Newsletter => "Newsletters".to_string(),
        EmailCategory::Receipt => "Receipts".to_string(),
        EmailCategory::Notification => "Notifications".to_string(),
        EmailCategory::Marketing => "Marketing".to_string(),
        EmailCategory::Shipping => "Shipping".to_string(),
        EmailCategory::Financial => "Financial".to_string(),
        EmailCategory::Personal => "Personal".to_string(),
        EmailCategory::Other => "Other".to_string(),
    }
}

/// Create mock Gmail API message response (JSON)
pub fn mock_gmail_message_response(
    id: &str,
    thread_id: &str,
    from: &str,
    subject: &str,
) -> serde_json::Value {
    json!({
        "id": id,
        "threadId": thread_id,
        "labelIds": ["INBOX", "UNREAD"],
        "snippet": "Email snippet...",
        "payload": {
            "mimeType": "multipart/alternative",
            "headers": [
                {"name": "From", "value": from},
                {"name": "Subject", "value": subject},
                {"name": "Date", "value": "Mon, 1 Jan 2024 10:00:00 -0800"},
                {"name": "To", "value": "me@example.com"}
            ]
        },
        "internalDate": "1704124800000",
        "sizeEstimate": 1234
    })
}

/// Create mock Gmail list messages response (JSON)
pub fn mock_gmail_list_response(
    message_ids: Vec<&str>,
    next_page_token: Option<&str>,
) -> serde_json::Value {
    let messages: Vec<serde_json::Value> = message_ids
        .iter()
        .map(|id| {
            json!({
                "id": id,
                "threadId": format!("thread_{}", id)
            })
        })
        .collect();

    let mut response = json!({
        "messages": messages,
        "resultSizeEstimate": messages.len()
    });

    if let Some(token) = next_page_token {
        response["nextPageToken"] = json!(token);
    }

    response
}

/// Create a test LabelInfo
pub fn create_test_label_info(id: &str, name: &str) -> gmail_automation::client::LabelInfo {
    gmail_automation::client::LabelInfo {
        id: id.to_string(),
        name: name.to_string(),
    }
}

/// Create a test ExistingFilterInfo
pub fn create_test_existing_filter(
    id: &str,
    query: Option<&str>,
    add_label_ids: Vec<&str>,
) -> gmail_automation::client::ExistingFilterInfo {
    gmail_automation::client::ExistingFilterInfo {
        id: id.to_string(),
        query: query.map(|s| s.to_string()),
        from: None,
        to: None,
        subject: None,
        add_label_ids: add_label_ids.into_iter().map(|s| s.to_string()).collect(),
        remove_label_ids: vec![],
    }
}

// Mock implementation of GmailClient for testing
mock! {
    pub GmailClient {}

    #[async_trait::async_trait]
    impl GmailClient for GmailClient {
        async fn list_message_ids(&self, query: &str) -> Result<Vec<String>>;
        async fn get_message(&self, id: &str) -> Result<MessageMetadata>;
        async fn list_labels(&self) -> Result<Vec<gmail_automation::client::LabelInfo>>;
        async fn create_label(&self, name: &str) -> Result<String>;
        async fn delete_label(&self, label_id: &str) -> Result<()>;
        async fn create_filter(&self, filter: &FilterRule) -> Result<String>;
        async fn list_filters(&self) -> Result<Vec<gmail_automation::client::ExistingFilterInfo>>;
        async fn delete_filter(&self, filter_id: &str) -> Result<()>;
        async fn update_filter(&self, filter_id: &str, filter: &FilterRule) -> Result<String>;
        async fn apply_label(&self, message_id: &str, label_id: &str) -> Result<()>;
        async fn remove_label(&self, message_id: &str, label_id: &str) -> Result<()>;
        async fn batch_remove_label(&self, message_ids: &[String], label_id: &str) -> Result<usize>;
        async fn batch_add_label(&self, message_ids: &[String], label_id: &str) -> Result<usize>;
        async fn batch_modify_labels(
            &self,
            message_ids: &[String],
            add_label_ids: &[String],
            remove_label_ids: &[String],
        ) -> Result<usize>;
        async fn fetch_messages_batch(&self, message_ids: Vec<String>) -> Result<Vec<MessageMetadata>>;
        async fn fetch_messages_with_progress(
            &self,
            message_ids: Vec<String>,
            on_progress: gmail_automation::client::ProgressCallback,
        ) -> Result<Vec<MessageMetadata>>;
        async fn quota_stats(&self) -> gmail_automation::rate_limiter::QuotaStats;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_test_message() {
        let msg = create_test_message("msg1", "test@example.com", "Test Subject");
        assert_eq!(msg.id, "msg1");
        assert_eq!(msg.sender_email, "test@example.com");
        assert_eq!(msg.sender_domain, "example.com");
        assert_eq!(msg.subject, "Test Subject");
    }

    #[test]
    fn test_create_newsletter_message() {
        let msg = create_newsletter_message("msg1", "newsletter@example.com");
        assert!(msg.has_unsubscribe);
        assert!(msg.is_automated);
    }

    #[test]
    fn test_create_receipt_message() {
        let msg = create_receipt_message("msg1");
        assert!(msg.is_automated);
        assert!(msg.subject.contains("Receipt"));
    }

    #[test]
    fn test_mock_gmail_message_response() {
        let response =
            mock_gmail_message_response("msg1", "thread1", "test@example.com", "Test Subject");
        assert_eq!(response["id"], "msg1");
        assert_eq!(response["threadId"], "thread1");
    }

    #[test]
    fn test_mock_gmail_list_response() {
        let response = mock_gmail_list_response(vec!["msg1", "msg2"], Some("token123"));
        assert_eq!(response["messages"].as_array().unwrap().len(), 2);
        assert_eq!(response["nextPageToken"], "token123");
    }

    #[test]
    fn test_category_to_label() {
        assert_eq!(category_to_label(&EmailCategory::Newsletter), "Newsletters");
        assert_eq!(category_to_label(&EmailCategory::Receipt), "Receipts");
    }
}
