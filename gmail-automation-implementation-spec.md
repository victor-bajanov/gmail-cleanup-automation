# Gmail Automation System - Implementation Specification
## Rust Implementation Guide

**Document Version:** 1.0  
**Date:** 24 November 2025  
**Author:** Victor Bajanov  
**Status:** Draft

---

## 1. Executive Summary

This implementation specification provides detailed technical guidance for building the Gmail automation system defined in the functional design specification. It covers the specific Rust crates, API patterns, testing strategies, and production-ready code examples needed to implement a robust email management system.

**Key Technologies:**
- Rust 1.70+ (2021 edition)
- google-gmail1 v6.0.0 (Gmail API client)
- yup-oauth2 v12.1.0 (OAuth2 authentication)
- tokio v1.47.1 (Async runtime)
- Complete testing stack (mockall, wiremock, proptest)

**Critical Implementation Decisions:**
- Use concurrent individual API requests, NOT native batch requests
- Implement rate limiting with semaphores (250 quota units/second limit)
- Use exponential backoff for all transient failures
- Checkpoint processing state every 100 messages
- Full unit test coverage with mocks for all external dependencies

---

## 2. Technology Stack and Dependencies

### 2.1 Core Dependencies

```toml
[package]
name = "gmail-automation"
version = "0.1.0"
edition = "2021"
rust-version = "1.70"

[dependencies]
# Gmail API
google-gmail1 = "6.0.0"
yup-oauth2 = "12.1.0"

# Async runtime
tokio = { version = "1.47", features = ["full"] }
futures = "0.3"
futures-buffered = "0.2"
async-stream = "0.3"

# HTTP client (required by google-gmail1)
hyper = "1"
hyper-rustls = "0.27"
hyper-util = "0.1"

# Serialization
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"

# Date/time
chrono = { version = "0.4", features = ["serde"] }

# Error handling
anyhow = "1"
thiserror = "1"
backoff = { version = "0.4", features = ["tokio"] }

# Logging
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }

# CLI
clap = { version = "4.4", features = ["derive"] }
indicatif = "0.17"

# Regex and patterns
regex = "1.10"
once_cell = "1.19"

# Caching
lru = "0.12"

# Database (optional)
rusqlite = { version = "0.30", features = ["bundled"], optional = true }

# ML providers (optional)
async-openai = { version = "0.17", optional = true }

# Python bridge for Claude Agents SDK (optional)
pyo3 = { version = "0.20", features = ["auto-initialize"], optional = true }

# Base64 encoding
base64 = "0.22"

# Async trait support
async-trait = "0.1"

[dev-dependencies]
tokio-test = "0.4"
mockall = "0.13"
wiremock = "0.6"
proptest = "1.8"
tempfile = "3"

[features]
default = ["cli"]
cli = []
cache = ["rusqlite"]
ml = ["async-openai"]
claude-agents = ["pyo3", "ml"]
```

### 2.2 Crate Version Justifications

**google-gmail1 6.0.0+20240624**
- Generated from Google's June 2024 API schema
- Provides complete access to Gmail API v1
- Type-safe Rust interface with builder patterns
- Automatic serde integration for JSON serialization

**yup-oauth2 12.1.0**
- Mature OAuth2 implementation with automatic token refresh
- Built-in token persistence to disk
- Supports InstalledFlow for desktop applications
- Checks token expiry 1 minute before actual expiration

**tokio 1.47.1**
- LTS release (supported until September 2026)
- Multi-threaded runtime with work-stealing
- Cooperative scheduling model
- Mature ecosystem with extensive documentation

---

## 3. Gmail API Integration Architecture

### 3.1 Hub Initialization Pattern

The Gmail hub serves as the central entry point for all API operations:

```rust
use google_gmail1::{Gmail, hyper_rustls, hyper_util, yup_oauth2};
use std::error::Error;

pub async fn initialize_gmail_hub(
    credentials_path: &str,
    token_cache_path: &str,
) -> Result<Gmail<hyper_rustls::HttpsConnector<hyper_util::client::legacy::connect::HttpConnector>>, Box<dyn Error>> {
    // Read OAuth2 credentials
    let secret = yup_oauth2::read_application_secret(credentials_path).await?;
    
    // Build authenticator with token persistence
    let auth = yup_oauth2::InstalledFlowAuthenticator::builder(
        secret,
        yup_oauth2::InstalledFlowReturnMethod::HTTPRedirect,
    )
    .persist_tokens_to_disk(token_cache_path)
    .build()
    .await?;
    
    // Configure HTTP client with TLS
    let client = hyper_util::client::legacy::Client::builder(
        hyper_util::rt::TokioExecutor::new()
    )
    .build(
        hyper_rustls::HttpsConnectorBuilder::new()
            .with_native_roots()?
            .https_or_http()
            .enable_http1()
            .build()
    );
    
    Ok(Gmail::new(client, auth))
}
```

**Key Design Decisions:**
- Use `HTTPRedirect` flow for desktop applications (opens browser)
- Persist tokens to disk for automatic refresh between runs
- Enable HTTP/1 for compatibility (HTTP/2 is default for google-gmail1)
- Use native TLS roots for maximum compatibility

### 3.2 Request Patterns - Concurrent Individual vs Batch

**IMPORTANT: Do NOT implement native batch requests.** The google-gmail1 crate uses concurrent individual requests, which is the recommended production pattern.

#### Why Concurrent Individual Requests Are Better

1. **Quota consumption is identical**: Gmail counts batched requests as n individual requests
2. **Rate limiting is the real bottleneck**: 250 quota units/second limit dominates performance
3. **HTTP/2 connection reuse**: Automatic connection pooling eliminates most HTTP overhead
4. **Superior error handling**: Easy to identify which specific request failed
5. **Simpler retry logic**: Retry only the failed requests, not entire batches
6. **Better progress tracking**: Monitor each operation independently

#### The Numbers

For fetching 1,000 messages (5 quota units each = 5,000 total units):

**Native Batch Approach:**
- HTTP overhead saved: ~400ms (optimistic)
- Quota consumed: 5,000 units
- **Total time: ~20 seconds** (limited by 250 units/sec rate limit)

**Concurrent Individual Approach:**
- HTTP overhead: ~400ms additional (via connection reuse)
- Quota consumed: 5,000 units
- **Total time: ~20 seconds** (limited by 250 units/sec rate limit)

**Conclusion:** The rate limit dominates; HTTP overhead is negligible.

#### Production Pattern: Buffered Streams

```rust
use futures::stream::{self, StreamExt, TryStreamExt};
use google_gmail1::api::Message;

pub async fn fetch_messages_concurrent(
    hub: &Gmail,
    message_ids: Vec<String>,
) -> Result<Vec<Message>, Box<dyn std::error::Error>> {
    stream::iter(message_ids)
        .map(|id| async move {
            hub.users()
                .messages_get("me", &id)
                .format("metadata")
                .doit()
                .await
                .map(|(_, msg)| msg)
        })
        .buffer_unordered(40)  // Limit concurrent requests to ~200 units/sec
        .try_collect()
        .await
}
```

**Concurrency Calculation:**
- Each `messages_get` = 5 quota units
- Target rate: 200 units/second (buffer below 250 limit)
- 200 / 5 = 40 concurrent requests
- Adjust based on operation mix and desired safety margin

### 3.3 Rate Limiting Implementation

Use semaphores to enforce rate limits across all API operations:

```rust
use tokio::sync::Semaphore;
use std::sync::Arc;

pub struct RateLimitedGmailClient {
    hub: Gmail,
    semaphore: Arc<Semaphore>,
}

impl RateLimitedGmailClient {
    pub fn new(hub: Gmail, max_concurrent: usize) -> Self {
        Self {
            hub,
            semaphore: Arc::new(Semaphore::new(max_concurrent)),
        }
    }
    
    pub async fn get_message(&self, message_id: &str) -> Result<Message, Box<dyn std::error::Error>> {
        let _permit = self.semaphore.acquire().await?;
        
        let (_, message) = self.hub
            .users()
            .messages_get("me", message_id)
            .doit()
            .await?;
        
        Ok(message)
    }
}
```

**Rate Limit Guidelines:**
- `messages.get` (5 units): 40-50 concurrent (200-250 units/sec)
- `messages.list` (1 unit): N/A (single sequential operation)
- `messages.modify` (5 units): 40-50 concurrent
- `labels.create` (5 units): Sequential recommended (low volume)
- `filters.create` (10 units): Sequential recommended (low volume)

### 3.4 Error Handling and Retry Logic

Implement exponential backoff for all transient failures:

```rust
use backoff::{ExponentialBackoff, retry, Error as BackoffError};
use google_gmail1::Error as GmailError;

pub async fn fetch_with_retry(
    hub: &Gmail,
    message_id: &str,
) -> Result<Message, Box<dyn std::error::Error>> {
    let operation = || async {
        hub.users()
            .messages_get("me", message_id)
            .doit()
            .await
            .map(|(_, msg)| msg)
            .map_err(|e| {
                match e {
                    GmailError::HttpError(ref err) if err.status() == 429 => {
                        // Rate limit exceeded - transient
                        BackoffError::transient(e)
                    }
                    GmailError::HttpError(ref err) if err.status() >= 500 => {
                        // Server error - transient
                        BackoffError::transient(e)
                    }
                    GmailError::Io(_) => {
                        // Network error - transient
                        BackoffError::transient(e)
                    }
                    _ => {
                        // All other errors - permanent
                        BackoffError::permanent(e)
                    }
                }
            })
    };
    
    let backoff = ExponentialBackoff {
        initial_interval: std::time::Duration::from_millis(100),
        max_interval: std::time::Duration::from_secs(30),
        max_elapsed_time: Some(std::time::Duration::from_secs(300)),
        ..Default::default()
    };
    
    retry(backoff, operation).await
}
```

**Error Classification:**
- **Transient (retry):** 429 (rate limit), 5xx (server errors), network errors
- **Permanent (fail fast):** 400 (bad request), 401 (unauthorized), 403 (forbidden), 404 (not found)

### 3.5 Message Format Selection

Choose format based on data requirements (all cost 5 quota units):

```rust
// Minimal: Only ID and labels (fastest, smallest)
pub async fn fetch_minimal(hub: &Gmail, id: &str) -> Result<Message, Box<dyn std::error::Error>> {
    let (_, msg) = hub.users()
        .messages_get("me", id)
        .format("minimal")
        .doit()
        .await?;
    Ok(msg)
}

// Metadata: Headers only (recommended for classification)
pub async fn fetch_metadata(hub: &Gmail, id: &str) -> Result<Message, Box<dyn std::error::Error>> {
    let (_, msg) = hub.users()
        .messages_get("me", id)
        .format("metadata")
        .add_metadata_headers("From")
        .add_metadata_headers("Subject")
        .add_metadata_headers("Date")
        .add_metadata_headers("List-Unsubscribe")
        .doit()
        .await?;
    Ok(msg)
}

// Full: Complete MIME structure (use only when needed)
pub async fn fetch_full(hub: &Gmail, id: &str) -> Result<Message, Box<dyn std::error::Error>> {
    let (_, msg) = hub.users()
        .messages_get("me", id)
        .format("full")
        .doit()
        .await?;
    Ok(msg)
}
```

**Format Selection Guidelines:**
- Use `minimal` for: Checking labels, counting messages
- Use `metadata` for: Classification, filter generation (RECOMMENDED)
- Use `full` for: Body content analysis, MIME parsing
- Use `raw` for: Message archival, RFC 2822 processing

---

## 4. OAuth2 Authentication Implementation

### 4.1 Credential Management

```rust
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Serialize, Deserialize)]
pub struct Credentials {
    pub installed: InstalledApp,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct InstalledApp {
    pub client_id: String,
    pub project_id: String,
    pub auth_uri: String,
    pub token_uri: String,
    pub client_secret: String,
    pub redirect_uris: Vec<String>,
}

pub async fn load_credentials(path: &Path) -> Result<Credentials, Box<dyn std::error::Error>> {
    let content = tokio::fs::read_to_string(path).await?;
    Ok(serde_json::from_str(&content)?)
}
```

### 4.2 Scope Selection

```rust
pub const REQUIRED_SCOPES: &[&str] = &[
    "https://www.googleapis.com/auth/gmail.modify",
    "https://www.googleapis.com/auth/gmail.labels",
    "https://www.googleapis.com/auth/gmail.settings.basic",
];

pub const READONLY_SCOPES: &[&str] = &[
    "https://www.googleapis.com/auth/gmail.readonly",
];

pub const METADATA_SCOPES: &[&str] = &[
    "https://www.googleapis.com/auth/gmail.metadata",
];
```

**Scope Hierarchy (most to least permissive):**
1. `https://mail.google.com/` - Full access including permanent deletion
2. `gmail.modify` - Read/write except permanent deletion (RECOMMENDED)
3. `gmail.readonly` - Read-only access
4. `gmail.metadata` - Headers only, no body content
5. `gmail.labels` - Label management only
6. `gmail.send` - Send only, no read access

### 4.3 Token Storage Security

```rust
#[cfg(unix)]
pub async fn secure_token_file(path: &Path) -> Result<(), std::io::Error> {
    use std::os::unix::fs::PermissionsExt;
    
    let mut perms = tokio::fs::metadata(path).await?.permissions();
    perms.set_mode(0o600);  // Read/write for owner only
    tokio::fs::set_permissions(path, perms).await?;
    Ok(())
}

#[cfg(windows)]
pub async fn secure_token_file(_path: &Path) -> Result<(), std::io::Error> {
    // Windows uses ACLs, file permissions are different
    // In production, use win32 APIs to set appropriate ACLs
    Ok(())
}
```

### 4.4 Environment-Based Credentials (Production)

```rust
use std::env;

pub fn load_credentials_from_env() -> Result<ApplicationSecret, Box<dyn std::error::Error>> {
    let client_id = env::var("GMAIL_CLIENT_ID")?;
    let client_secret = env::var("GMAIL_CLIENT_SECRET")?;
    let redirect_uri = env::var("GMAIL_REDIRECT_URI")
        .unwrap_or_else(|_| "http://localhost:8080".to_string());
    
    Ok(ApplicationSecret {
        client_id,
        client_secret,
        auth_uri: "https://accounts.google.com/o/oauth2/auth".to_string(),
        token_uri: "https://oauth2.googleapis.com/token".to_string(),
        redirect_uris: vec![redirect_uri],
        ..Default::default()
    })
}
```

---

## 5. Data Structures

### 5.1 Core Domain Models

```rust
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
```

### 5.2 Custom Deserializers for Gmail API

```rust
use serde::de::{self, Deserializer};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};

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
```

### 5.3 State Management for Resumability

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessingState {
    pub run_id: String,
    pub started_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub phase: ProcessingPhase,
    pub messages_scanned: usize,
    pub messages_classified: usize,
    pub labels_created: Vec<String>,
    pub filters_created: Vec<String>,
    pub last_processed_message_id: Option<String>,
    pub failed_message_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProcessingPhase {
    Scanning,
    Classifying,
    CreatingLabels,
    CreatingFilters,
    ApplyingLabels,
    Complete,
}

impl ProcessingState {
    pub async fn save(&self, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let json = serde_json::to_string_pretty(self)?;
        tokio::fs::write(path, json).await?;
        Ok(())
    }
    
    pub async fn load(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let json = tokio::fs::read_to_string(path).await?;
        Ok(serde_json::from_str(&json)?)
    }
}
```

---

## 6. Comprehensive Testing Strategy

### 6.1 Trait-Based Mocking for Unit Tests

```rust
use async_trait::async_trait;

#[async_trait]
pub trait GmailClient: Send + Sync {
    async fn list_message_ids(&self, query: &str) -> Result<Vec<String>, GmailError>;
    async fn get_message(&self, id: &str) -> Result<MessageMetadata, GmailError>;
    async fn create_label(&self, name: &str) -> Result<String, GmailError>;
    async fn create_filter(&self, filter: &FilterRule) -> Result<String, GmailError>;
    async fn apply_label(&self, message_id: &str, label_id: &str) -> Result<(), GmailError>;
}

pub struct RealGmailClient {
    hub: Gmail,
}

#[async_trait]
impl GmailClient for RealGmailClient {
    async fn get_message(&self, id: &str) -> Result<MessageMetadata, GmailError> {
        let (_, msg) = self.hub
            .users()
            .messages_get("me", id)
            .format("metadata")
            .doit()
            .await
            .map_err(|e| GmailError::ApiError(e.to_string()))?;
        
        // Parse gmail1::api::Message into MessageMetadata
        parse_message_metadata(msg)
    }
    
    // ... other implementations
}
```

### 6.2 Mockall-Based Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use mockall::mock;
    use mockall::predicate::*;

    mock! {
        pub GmailClient {}
        
        #[async_trait]
        impl GmailClient for GmailClient {
            async fn list_message_ids(&self, query: &str) -> Result<Vec<String>, GmailError>;
            async fn get_message(&self, id: &str) -> Result<MessageMetadata, GmailError>;
            async fn create_label(&self, name: &str) -> Result<String, GmailError>;
            async fn create_filter(&self, filter: &FilterRule) -> Result<String, GmailError>;
            async fn apply_label(&self, message_id: &str, label_id: &str) -> Result<(), GmailError>;
        }
    }

    #[tokio::test]
    async fn test_email_scanner_retrieves_all_messages() {
        let mut mock = MockGmailClient::new();
        
        // Setup expectations
        mock.expect_list_message_ids()
            .with(eq("after:2025/08/24"))
            .times(1)
            .returning(|_| Ok(vec!["msg1".to_string(), "msg2".to_string()]));
        
        mock.expect_get_message()
            .with(eq("msg1"))
            .times(1)
            .returning(|_| Ok(MessageMetadata {
                id: "msg1".to_string(),
                sender_email: "test@example.com".to_string(),
                // ... other fields
            }));
        
        mock.expect_get_message()
            .with(eq("msg2"))
            .times(1)
            .returning(|_| Ok(MessageMetadata {
                id: "msg2".to_string(),
                sender_email: "test2@example.com".to_string(),
                // ... other fields
            }));
        
        // Test
        let scanner = EmailScanner::new(Box::new(mock));
        let messages = scanner.scan_period(90).await.unwrap();
        
        assert_eq!(messages.len(), 2);
    }

    #[tokio::test]
    async fn test_email_scanner_handles_rate_limit_errors() {
        let mut mock = MockGmailClient::new();
        
        mock.expect_get_message()
            .times(1)
            .returning(|_| Err(GmailError::RateLimitExceeded { retry_after: 1 }));
        
        let scanner = EmailScanner::new(Box::new(mock));
        let result = scanner.get_message("msg1").await;
        
        assert!(matches!(result, Err(GmailError::RateLimitExceeded { .. })));
    }
}
```

### 6.3 HTTP-Level Integration Tests with Wiremock

```rust
#[cfg(test)]
mod integration_tests {
    use super::*;
    use wiremock::{MockServer, Mock, ResponseTemplate};
    use wiremock::matchers::{method, path, header, query_param};

    #[tokio::test]
    async fn test_list_messages_with_pagination() {
        let mock_server = MockServer::start().await;
        
        // First page
        Mock::given(method("GET"))
            .and(path("/gmail/v1/users/me/messages"))
            .and(query_param("q", "after:2025/08/24"))
            .and(query_param("maxResults", "100"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "messages": [
                    {"id": "msg1", "threadId": "thread1"},
                    {"id": "msg2", "threadId": "thread2"}
                ],
                "nextPageToken": "token123"
            })))
            .expect(1)
            .mount(&mock_server)
            .await;
        
        // Second page
        Mock::given(method("GET"))
            .and(path("/gmail/v1/users/me/messages"))
            .and(query_param("pageToken", "token123"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "messages": [
                    {"id": "msg3", "threadId": "thread3"}
                ]
            })))
            .expect(1)
            .mount(&mock_server)
            .await;
        
        // Test with mock server
        let client = build_test_client(mock_server.uri());
        let messages = client.list_all_messages("after:2025/08/24").await.unwrap();
        
        assert_eq!(messages.len(), 3);
    }

    #[tokio::test]
    async fn test_rate_limit_handling() {
        let mock_server = MockServer::start().await;
        
        // First call: rate limited
        Mock::given(method("GET"))
            .and(path("/gmail/v1/users/me/messages/msg1"))
            .respond_with(ResponseTemplate::new(429).set_body_json(serde_json::json!({
                "error": {
                    "code": 429,
                    "message": "Rate Limit Exceeded"
                }
            })))
            .up_to_n_times(1)
            .mount(&mock_server)
            .await;
        
        // Second call: success
        Mock::given(method("GET"))
            .and(path("/gmail/v1/users/me/messages/msg1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "msg1",
                "threadId": "thread1",
                "labelIds": ["INBOX"]
            })))
            .expect(1)
            .mount(&mock_server)
            .await;
        
        let client = build_test_client_with_retry(mock_server.uri());
        let message = client.get_message_with_retry("msg1").await.unwrap();
        
        assert_eq!(message.id, "msg1");
    }
}
```

### 6.4 Property-Based Testing for Classification

```rust
#[cfg(test)]
mod property_tests {
    use super::*;
    use proptest::prelude::*;

    fn email_address_strategy() -> impl Strategy<Value = String> {
        r"[a-z0-9]{1,10}@[a-z]{3,10}\.(com|org|net)"
            .prop_map(|s| s.to_lowercase())
    }

    fn subject_strategy() -> impl Strategy<Value = String> {
        r"\PC{5,100}"
    }

    proptest! {
        #[test]
        fn test_classification_is_deterministic(
            sender in email_address_strategy(),
            subject in subject_strategy(),
        ) {
            let message = MessageMetadata {
                sender_email: sender,
                subject: subject,
                // ... other fields with defaults
            };
            
            let classifier = EmailClassifier::new();
            let result1 = classifier.classify(&message);
            let result2 = classifier.classify(&message);
            
            prop_assert_eq!(result1.category, result2.category);
            prop_assert_eq!(result1.confidence, result2.confidence);
        }

        #[test]
        fn test_automated_detection_consistent(
            sender in email_address_strategy(),
        ) {
            let message = MessageMetadata {
                sender_email: sender.clone(),
                // ... other fields
            };
            
            let is_automated1 = is_automated_sender(&message);
            let is_automated2 = is_automated_sender(&message);
            
            prop_assert_eq!(is_automated1, is_automated2);
        }

        #[test]
        fn test_label_name_sanitization_valid(
            raw_name in r"[\w\s-]{1,50}",
        ) {
            let sanitized = sanitize_label_name(&raw_name);
            
            // Properties that must hold
            prop_assert!(!sanitized.is_empty());
            prop_assert!(sanitized.len() <= 50);
            prop_assert!(!sanitized.starts_with('/'));
            prop_assert!(!sanitized.ends_with('/'));
        }
    }
}
```

### 6.5 Test Coverage Requirements

**Unit Test Coverage: 100%**
- All public functions must have unit tests
- All error paths must be tested
- Edge cases must be explicitly tested

**Integration Test Coverage: 80%**
- Critical paths (auth, list, get, create operations)
- Error handling flows
- Rate limiting behavior

**Property Test Coverage:**
- All classification logic
- All string manipulation functions
- All domain/email parsing logic

---

## 7. Production Architecture

### 7.1 Complete Client Implementation

```rust
use std::sync::Arc;
use tokio::sync::Semaphore;

pub struct ProductionGmailClient {
    hub: Gmail,
    rate_limiter: Arc<Semaphore>,
    backoff_policy: ExponentialBackoff,
}

impl ProductionGmailClient {
    pub async fn new(
        credentials_path: &str,
        token_cache_path: &str,
        max_concurrent: usize,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let hub = initialize_gmail_hub(credentials_path, token_cache_path).await?;
        
        let backoff = ExponentialBackoff {
            initial_interval: Duration::from_millis(100),
            max_interval: Duration::from_secs(30),
            max_elapsed_time: Some(Duration::from_secs(300)),
            ..Default::default()
        };
        
        Ok(Self {
            hub,
            rate_limiter: Arc::new(Semaphore::new(max_concurrent)),
            backoff_policy: backoff,
        })
    }
    
    pub async fn fetch_messages_batch(
        &self,
        message_ids: Vec<String>,
    ) -> Result<Vec<MessageMetadata>, Box<dyn std::error::Error>> {
        use futures::stream::{self, StreamExt, TryStreamExt};
        
        stream::iter(message_ids)
            .map(|id| {
                let rate_limiter = Arc::clone(&self.rate_limiter);
                async move {
                    let _permit = rate_limiter.acquire().await?;
                    self.fetch_single_with_retry(&id).await
                }
            })
            .buffer_unordered(40)
            .try_collect()
            .await
    }
    
    async fn fetch_single_with_retry(
        &self,
        id: &str,
    ) -> Result<MessageMetadata, Box<dyn std::error::Error>> {
        let operation = || async {
            let (_, msg) = self.hub
                .users()
                .messages_get("me", id)
                .format("metadata")
                .add_metadata_headers("From")
                .add_metadata_headers("Subject")
                .add_metadata_headers("Date")
                .add_metadata_headers("List-Unsubscribe")
                .doit()
                .await
                .map_err(|e| {
                    if should_retry(&e) {
                        BackoffError::transient(e)
                    } else {
                        BackoffError::permanent(e)
                    }
                })?;
            
            parse_message_metadata(msg)
                .map_err(|e| BackoffError::permanent(e))
        };
        
        retry(self.backoff_policy.clone(), operation).await
    }
}

fn should_retry(error: &google_gmail1::Error) -> bool {
    match error {
        google_gmail1::Error::HttpError(err) => {
            err.status() == 429 || err.status() >= 500
        }
        google_gmail1::Error::Io(_) => true,
        _ => false,
    }
}
```

### 7.2 Streaming with Checkpoints

```rust
use async_stream::stream;
use futures::stream::Stream;

pub fn scan_messages_with_checkpoints<'a>(
    hub: &'a Gmail,
    checkpoint_path: &'a Path,
) -> impl Stream<Item = Result<MessageMetadata, Box<dyn std::error::Error>>> + 'a {
    stream! {
        let mut state = ProcessingState::load(checkpoint_path)
            .await
            .unwrap_or_else(|_| ProcessingState::new());
        
        let mut page_token: Option<String> = None;
        let mut count = 0;
        
        loop {
            let mut call = hub.users()
                .messages_list("me")
                .q("after:2025/08/24")
                .max_results(100);
            
            if let Some(token) = page_token.as_ref() {
                call = call.page_token(token);
            }
            
            let (_, response) = match call.doit().await {
                Ok(r) => r,
                Err(e) => {
                    yield Err(Box::new(e) as Box<dyn std::error::Error>);
                    break;
                }
            };
            
            if let Some(messages) = response.messages {
                for msg_ref in messages {
                    if let Some(id) = msg_ref.id {
                        // Fetch full metadata
                        let (_, msg) = match hub.users()
                            .messages_get("me", &id)
                            .format("metadata")
                            .doit()
                            .await
                        {
                            Ok(m) => m,
                            Err(e) => {
                                yield Err(Box::new(e) as Box<dyn std::error::Error>);
                                continue;
                            }
                        };
                        
                        match parse_message_metadata(msg) {
                            Ok(metadata) => {
                                yield Ok(metadata);
                                count += 1;
                                
                                // Checkpoint every 100 messages
                                if count % 100 == 0 {
                                    state.messages_scanned = count;
                                    state.last_processed_message_id = Some(id);
                                    state.updated_at = Utc::now();
                                    let _ = state.save(checkpoint_path).await;
                                }
                            }
                            Err(e) => {
                                yield Err(e);
                            }
                        }
                    }
                }
            }
            
            page_token = response.next_page_token;
            if page_token.is_none() {
                break;
            }
        }
        
        // Final checkpoint
        state.messages_scanned = count;
        state.phase = ProcessingPhase::Complete;
        state.updated_at = Utc::now();
        let _ = state.save(checkpoint_path).await;
    }
}
```

---

## 8. Critical Implementation Patterns

### 8.1 DO Patterns

✅ **Use concurrent individual requests with rate limiting**
```rust
// Good: Concurrent with bounded parallelism
stream::iter(ids)
    .map(|id| fetch(id))
    .buffer_unordered(40)
    .try_collect().await
```

✅ **Implement exponential backoff for all API calls**
```rust
retry(ExponentialBackoff::default(), operation).await
```

✅ **Use semaphores to enforce rate limits**
```rust
let _permit = semaphore.acquire().await?;
api_call().await
```

✅ **Checkpoint state every 100 operations**
```rust
if count % 100 == 0 {
    state.save(checkpoint_path).await?;
}
```

✅ **Use format("metadata") for classification**
```rust
.format("metadata")
.add_metadata_headers("From")
.add_metadata_headers("Subject")
```

✅ **Test with comprehensive mocks**
```rust
mock.expect_get_message()
    .times(1)
    .returning(|_| Ok(test_message()));
```

### 8.2 DON'T Patterns

❌ **Do NOT implement native batch requests**
```rust
// Wrong: Manual multipart/mixed construction
let batch_body = format!("--boundary
{}", requests);
```

❌ **Do NOT use unbounded concurrency**
```rust
// Wrong: No limit on concurrent operations
for id in ids {
    spawn(fetch(id));
}
```

❌ **Do NOT ignore rate limit errors**
```rust
// Wrong: No retry logic
fetch().await.unwrap()
```

❌ **Do NOT hold locks across await points**
```rust
// Wrong: Lock held during async operation
let mut state = state.lock().unwrap();
let result = api_call().await?;
state.update(result);
```

❌ **Do NOT use blocking operations in async context**
```rust
// Wrong: Blocking file I/O
std::fs::read_to_string("file.txt")?
// Correct:
tokio::fs::read_to_string("file.txt").await?
```

❌ **Do NOT request broader scopes than necessary**
```rust
// Wrong: Full mailbox access
"https://mail.google.com/"
// Correct: Minimal required scope
"https://www.googleapis.com/auth/gmail.modify"
```

---

## 9. Performance Benchmarks and Optimization

### 9.1 Expected Performance Metrics

**Throughput (constrained by rate limits):**
- Message retrieval: ~40-50 messages/second (200-250 quota units/sec)
- Label creation: 2-3/second (sequential, 5 units each)
- Filter creation: 1-2/second (sequential, 10 units each)

**Latency:**
- Single message.get: ~100-300ms
- Single messages.list: ~200-500ms
- Token refresh: ~500-1000ms (amortized across many requests)

**Memory Usage:**
- Base client: ~10MB
- Per message (metadata): ~2KB
- Streaming: ~100MB for 10,000 messages
- LRU cache (1000 entries): ~2MB

### 9.2 Optimization Strategies

**1. Adjust concurrency based on operation mix:**
```rust
// For mixed operations, calculate weighted average
let message_gets = 1000;  // 5 units each = 5000 units
let label_creates = 50;   // 5 units each = 250 units
let total_units = 5250;
let target_rate = 200;    // units/sec (safety margin)

// If all operations were messages.get:
let max_concurrent = target_rate / 5;  // 40
```

**2. Use LRU caching for frequently accessed data:**
```rust
use lru::LruCache;

pub struct CachedGmailClient {
    client: ProductionGmailClient,
    cache: Arc<Mutex<LruCache<String, MessageMetadata>>>,
}

impl CachedGmailClient {
    pub async fn get_message(&self, id: &str) -> Result<MessageMetadata, Box<dyn std::error::Error>> {
        // Check cache first
        {
            let mut cache = self.cache.lock().await;
            if let Some(metadata) = cache.get(id) {
                return Ok(metadata.clone());
            }
        }
        
        // Fetch and cache
        let metadata = self.client.fetch_single_with_retry(id).await?;
        
        {
            let mut cache = self.cache.lock().await;
            cache.put(id.to_string(), metadata.clone());
        }
        
        Ok(metadata)
    }
}
```

**3. Use streaming to control memory usage:**
```rust
// Process in streaming fashion instead of collecting all
use futures::StreamExt;

let mut stream = scan_messages_with_checkpoints(&hub, checkpoint_path);
while let Some(result) = stream.next().await {
    let message = result?;
    process_message(message).await?;
    // Memory released after processing each message
}
```

---

## 10. Deployment Checklist

### 10.1 Pre-Deployment

- [ ] All unit tests pass with 100% coverage
- [ ] All integration tests pass
- [ ] Property tests run without failures
- [ ] OAuth2 credentials stored securely (environment variables)
- [ ] Token cache directory has correct permissions (0600)
- [ ] Rate limiting tested under load
- [ ] Checkpoint/resume tested with interruptions
- [ ] Error handling tested with mock failures
- [ ] Logging configured appropriately

### 10.2 Configuration

- [ ] Set appropriate `max_concurrent` based on operation mix
- [ ] Configure checkpoint frequency (default: every 100 messages)
- [ ] Set up structured logging with JSON output
- [ ] Configure retry backoff parameters
- [ ] Set OAuth2 scopes to minimum required
- [ ] Configure LRU cache size based on available memory

### 10.3 Monitoring

- [ ] Track API quota usage via Google Cloud Console
- [ ] Monitor rate limit errors (should be near zero)
- [ ] Track processing throughput (messages/second)
- [ ] Monitor checkpoint file size growth
- [ ] Alert on authentication failures
- [ ] Track classification accuracy (sample validation)

---

## 11. Troubleshooting Guide

### 11.1 Common Issues

**Issue: Rate limit errors (429)**
- Cause: Too many concurrent requests or burst too high
- Solution: Reduce `max_concurrent` parameter, increase backoff delays

**Issue: Authentication failures (401)**
- Cause: Token expired or invalid credentials
- Solution: Delete token cache, re-authenticate; verify credentials file

**Issue: Quota exceeded (403)**
- Cause: Daily quota limit reached
- Solution: Wait for quota reset (midnight Pacific time), request quota increase

**Issue: Slow processing**
- Cause: Sequential operations or network latency
- Solution: Increase concurrency (up to rate limits), use metadata format

**Issue: Memory exhaustion**
- Cause: Collecting too many messages in memory
- Solution: Use streaming with bounded buffers, process in smaller batches

### 11.2 Debugging Techniques

**Enable trace logging:**
```rust
tracing_subscriber::fmt()
    .with_max_level(tracing::Level::TRACE)
    .with_target(true)
    .init();
```

**Use request IDs for correlation:**
```rust
let request_id = Uuid::new_v4();
tracing::info!(request_id = %request_id, "Starting message fetch");
```

**Inspect API responses:**
```rust
let (response, message) = hub.users().messages_get("me", id).doit().await?;
tracing::debug!(?response, "API response received");
```

---

## 12. References

- Gmail API Documentation: https://developers.google.com/gmail/api
- google-gmail1 crate: https://docs.rs/google-gmail1/6.0.0
- yup-oauth2 crate: https://docs.rs/yup-oauth2/12.1.0
- Tokio documentation: https://tokio.rs
- Rust Async Book: https://rust-lang.github.io/async-book/

---

## 13. Document Change History

| Version | Date | Author | Changes |
|---------|------|--------|---------|
| 1.0 | 2025-11-24 | Victor Bajanov | Initial implementation specification |

---

**End of Implementation Specification**
