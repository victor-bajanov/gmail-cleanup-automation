use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageMetadata {
    pub id: String,
    pub thread_id: String,
    pub sender_email: String,
    pub sender_domain: String,
    pub sender_name: String,
    pub subject: String,
    pub recipients: Vec<String>,
    pub date_received: DateTime<Utc>,
    pub labels: Vec<String>,
    pub has_unsubscribe: bool,
    pub is_automated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Classification {
    pub message_id: String,
    pub category: EmailCategory,
    pub confidence: f32,
    pub suggested_label: String,
    pub should_archive: bool,
    pub reasoning: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum EmailCategory {
    Newsletter,
    Receipt,
    Notification,
    Marketing,
    Shipping,
    Financial,
    Personal,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterRule {
    pub id: Option<String>,
    pub name: String,
    pub from_pattern: Option<String>,
    pub subject_keywords: Vec<String>,
    pub target_label_id: String,
    pub should_archive: bool,
    pub estimated_matches: usize,
}

/// Custom deserializers for Gmail API types
pub mod deserializers {
    use serde::{de::{self, Deserializer}, Deserialize};
    use chrono::{DateTime, Utc};
    use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};

    /// Deserialize Gmail timestamp (milliseconds since epoch as string)
    pub fn deserialize_gmail_timestamp<'de, D>(
        deserializer: D,
    ) -> Result<Option<DateTime<Utc>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let opt: Option<String> = Option::deserialize(deserializer)?;
        match opt {
            Some(s) => {
                let millis = s.parse::<i64>().map_err(de::Error::custom)?;
                let dt = DateTime::from_timestamp_millis(millis)
                    .ok_or_else(|| de::Error::custom("Invalid timestamp"))?;
                Ok(Some(dt))
            }
            None => Ok(None),
        }
    }

    /// Deserialize base64url encoded data
    pub fn deserialize_base64url<'de, D>(
        deserializer: D,
    ) -> Result<Option<Vec<u8>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let opt: Option<String> = Option::deserialize(deserializer)?;
        match opt {
            Some(s) => {
                let decoded = URL_SAFE_NO_PAD.decode(s).map_err(de::Error::custom)?;
                Ok(Some(decoded))
            }
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_metadata_serialization() {
        let metadata = MessageMetadata {
            id: "123".to_string(),
            thread_id: "456".to_string(),
            sender_email: "test@example.com".to_string(),
            sender_domain: "example.com".to_string(),
            sender_name: "Test User".to_string(),
            subject: "Test Subject".to_string(),
            recipients: vec!["recipient@example.com".to_string()],
            date_received: Utc::now(),
            labels: vec!["INBOX".to_string()],
            has_unsubscribe: false,
            is_automated: false,
        };

        let json = serde_json::to_string(&metadata).unwrap();
        let deserialized: MessageMetadata = serde_json::from_str(&json).unwrap();

        assert_eq!(metadata.id, deserialized.id);
        assert_eq!(metadata.sender_email, deserialized.sender_email);
    }

    #[test]
    fn test_email_category_equality() {
        assert_eq!(EmailCategory::Newsletter, EmailCategory::Newsletter);
        assert_ne!(EmailCategory::Newsletter, EmailCategory::Receipt);
    }
}
