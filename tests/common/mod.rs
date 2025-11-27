//! Common test utilities and fixtures

use chrono::Utc;
use gmail_automation::models::{Classification, EmailCategory, FilterRule, MessageMetadata};
use gmail_automation::client::GmailClient;
use gmail_automation::error::Result;
use mockall::predicate::*;
use mockall::mock;
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

/// Create a personal message
pub fn create_personal_message(id: &str) -> MessageMetadata {
    create_test_message(id, "friend@personal.com", "Hey, how are you?")
}

/// Create a shipping notification message
pub fn create_shipping_message(id: &str) -> MessageMetadata {
    create_automated_message(id, "shipping@logistics.com", "Your package is on its way!")
}

/// Create a financial message
pub fn create_financial_message(id: &str) -> MessageMetadata {
    create_automated_message(id, "alerts@bank.com", "Account Statement Available")
}

/// Create a test classification
pub fn create_test_classification(
    message_id: &str,
    category: EmailCategory,
    confidence: f32,
) -> Classification {
    Classification {
        message_id: message_id.to_string(),
        category: category.clone(),
        confidence,
        suggested_label: format!("AutoSort/{}", category_to_label(&category)),
        should_archive: confidence > 0.8,
        reasoning: Some(format!("Classified as {:?} with high confidence", category)),
    }
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

/// Create a test filter rule
pub fn create_test_filter(name: &str, from_pattern: &str, label_id: &str) -> FilterRule {
    FilterRule {
        id: None,
        name: name.to_string(),
        from_pattern: Some(from_pattern.to_string()),
        subject_keywords: vec![],
        target_label_id: label_id.to_string(),
        should_archive: true,
        estimated_matches: 10,
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

/// Create mock Gmail label response (JSON)
pub fn mock_gmail_label_response(id: &str, name: &str) -> serde_json::Value {
    json!({
        "id": id,
        "name": name,
        "messageListVisibility": "show",
        "labelListVisibility": "labelShow",
        "type": "user"
    })
}

/// Create mock Gmail filter response (JSON)
pub fn mock_gmail_filter_response(id: &str, from: &str, label_id: &str) -> serde_json::Value {
    json!({
        "id": id,
        "criteria": {
            "from": from
        },
        "action": {
            "addLabelIds": [label_id],
            "removeLabelIds": ["INBOX"]
        }
    })
}

/// Create a mock Gmail error response (JSON)
pub fn mock_gmail_error_response(code: u16, message: &str) -> serde_json::Value {
    json!({
        "error": {
            "code": code,
            "message": message,
            "status": match code {
                400 => "INVALID_ARGUMENT",
                401 => "UNAUTHENTICATED",
                403 => "PERMISSION_DENIED",
                404 => "NOT_FOUND",
                429 => "RESOURCE_EXHAUSTED",
                500 => "INTERNAL",
                _ => "UNKNOWN"
            }
        }
    })
}

/// Mock implementation of GmailClient for testing
mock! {
    pub GmailClient {}

    #[async_trait::async_trait]
    impl GmailClient for GmailClient {
        async fn list_message_ids(&self, query: &str) -> Result<Vec<String>>;
        async fn get_message(&self, id: &str) -> Result<MessageMetadata>;
        async fn create_label(&self, name: &str) -> Result<String>;
        async fn create_filter(&self, filter: &FilterRule) -> Result<String>;
        async fn apply_label(&self, message_id: &str, label_id: &str) -> Result<()>;
        async fn fetch_messages_batch(&self, message_ids: Vec<String>) -> Result<Vec<MessageMetadata>>;
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
        let response = mock_gmail_message_response(
            "msg1",
            "thread1",
            "test@example.com",
            "Test Subject",
        );
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
