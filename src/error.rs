use thiserror::Error;

/// Type alias for Result with GmailError
pub type Result<T> = std::result::Result<T, GmailError>;

/// Comprehensive error types for Gmail automation system
#[derive(Error, Debug)]
pub enum GmailError {
    /// Gmail API returned an error
    #[error("Gmail API error: {0}")]
    ApiError(String),

    /// Authentication failed
    #[error("Authentication failed: {0}")]
    AuthError(String),

    /// Rate limit exceeded - should retry after specified seconds
    #[error("Rate limit exceeded, retry after {retry_after} seconds")]
    RateLimitExceeded { retry_after: u64 },

    /// Rate limit error (alias for backwards compatibility)
    #[error("Rate limit error: {0}")]
    RateLimitError(String),

    /// User cancelled operation
    #[error("Operation cancelled: {0}")]
    OperationCancelled(String),

    /// Network-related error (connection issues, timeouts, etc.)
    #[error("Network error: {0}")]
    NetworkError(String),

    /// Server returned 5xx error
    #[error("Server error (HTTP {status}): {message}")]
    ServerError { status: u16, message: String },

    /// Resource not found (404)
    #[error("Message not found: {0}")]
    MessageNotFound(String),

    /// Bad request (400)
    #[error("Bad request: {0}")]
    BadRequest(String),

    /// Forbidden (403)
    #[error("Access forbidden: {0}")]
    Forbidden(String),

    /// Invalid message format or parsing error
    #[error("Invalid message format: {0}")]
    InvalidMessageFormat(String),

    /// Label-related errors
    #[error("Label error: {0}")]
    LabelError(String),

    /// Filter-related errors
    #[error("Filter error: {0}")]
    FilterError(String),

    /// Classification errors
    #[error("Classification error: {0}")]
    ClassificationError(String),

    /// IO error (file operations, etc.)
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    /// JSON serialization/deserialization error
    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    /// Configuration error
    #[error("Configuration error: {0}")]
    ConfigError(String),

    /// State management errors
    #[error("State error: {0}")]
    StateError(String),

    /// Quota exceeded
    #[error("Quota exceeded: {0}")]
    QuotaExceeded(String),

    /// Circuit breaker is open (rejecting requests)
    #[error("Circuit breaker open: {message}. Will retry after {retry_after_secs} seconds")]
    CircuitBreakerOpen {
        message: String,
        retry_after_secs: u64,
    },

    /// Generic catch-all error
    #[error("Unknown error: {0}")]
    Unknown(String),
}

impl GmailError {
    /// Check if the error is transient and should be retried
    pub fn is_transient(&self) -> bool {
        matches!(
            self,
            GmailError::RateLimitExceeded { .. }
                | GmailError::RateLimitError(_)
                | GmailError::ServerError { .. }
                | GmailError::NetworkError(_)
                | GmailError::CircuitBreakerOpen { .. }
        )
    }

    /// Check if the error is permanent and should not be retried
    pub fn is_permanent(&self) -> bool {
        !self.is_transient()
    }
}

/// Parse the Retry-After header from an HTTP response
///
/// The Retry-After header can be specified in two formats:
/// 1. Delay-seconds: An integer indicating seconds to wait (e.g., "120")
/// 2. HTTP-date: An HTTP date format (e.g., "Wed, 21 Oct 2015 07:28:00 GMT")
///
/// Returns the number of seconds to wait. If the header is missing or invalid,
/// returns a default of 5 seconds.
fn parse_retry_after_header<B>(response: &hyper::Response<B>) -> u64 {
    const DEFAULT_RETRY_AFTER: u64 = 5;

    if let Some(retry_after_value) = response.headers().get("retry-after") {
        if let Ok(retry_after_str) = retry_after_value.to_str() {
            // Try to parse as integer (delay-seconds format)
            if let Ok(seconds) = retry_after_str.parse::<u64>() {
                return seconds;
            }

            // Try to parse as HTTP date format
            if let Ok(http_date) = httpdate::parse_http_date(retry_after_str) {
                // Calculate seconds until that time
                let now = std::time::SystemTime::now();
                if let Ok(duration) = http_date.duration_since(now) {
                    return duration.as_secs();
                }
            }
        }
    }

    DEFAULT_RETRY_AFTER
}

impl From<google_gmail1::Error> for GmailError {
    fn from(error: google_gmail1::Error) -> Self {
        match error {
            // HTTP response with status code (non-success responses)
            google_gmail1::Error::Failure(ref response) => {
                let status = response.status();
                let status_code = status.as_u16();
                let message = format!(
                    "HTTP {}: {}",
                    status_code,
                    status.canonical_reason().unwrap_or("Unknown")
                );

                match status_code {
                    // Rate limiting - transient
                    429 => {
                        let retry_after = parse_retry_after_header(response);
                        GmailError::RateLimitExceeded { retry_after }
                    }
                    // Not found
                    404 => GmailError::MessageNotFound("Resource not found".to_string()),
                    // Bad request
                    400 => GmailError::BadRequest(message),
                    // Forbidden
                    403 => GmailError::Forbidden(message),
                    // Server errors - transient
                    500..=599 => GmailError::ServerError {
                        status: status_code,
                        message,
                    },
                    // Other non-success status codes
                    _ => GmailError::ApiError(message),
                }
            }
            // BadRequest variant (request not understood by server)
            google_gmail1::Error::BadRequest(ref err) => GmailError::BadRequest(format!("{}", err)),
            // Network/connection errors - transient
            google_gmail1::Error::HttpError(ref err) => {
                GmailError::NetworkError(format!("Connection error: {}", err))
            }
            // IO errors - transient
            google_gmail1::Error::Io(err) => GmailError::NetworkError(err.to_string()),
            // All other errors
            _ => GmailError::ApiError(error.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transient_errors() {
        let rate_limit = GmailError::RateLimitExceeded { retry_after: 5 };
        assert!(rate_limit.is_transient());
        assert!(!rate_limit.is_permanent());

        let server_error = GmailError::ServerError {
            status: 503,
            message: "Service unavailable".to_string(),
        };
        assert!(server_error.is_transient());

        let network_error = GmailError::NetworkError("Connection timeout".to_string());
        assert!(network_error.is_transient());
    }

    #[test]
    fn test_permanent_errors() {
        let bad_request = GmailError::BadRequest("Invalid query".to_string());
        assert!(bad_request.is_permanent());
        assert!(!bad_request.is_transient());

        let not_found = GmailError::MessageNotFound("msg123".to_string());
        assert!(not_found.is_permanent());

        let forbidden = GmailError::Forbidden("Access denied".to_string());
        assert!(forbidden.is_permanent());
    }

    #[test]
    fn test_error_display() {
        let error = GmailError::RateLimitExceeded { retry_after: 10 };
        let display = format!("{}", error);
        assert!(display.contains("Rate limit exceeded"));
        assert!(display.contains("10 seconds"));

        let auth_error = GmailError::AuthError("Invalid token".to_string());
        let display = format!("{}", auth_error);
        assert!(display.contains("Authentication failed"));
    }

    #[test]
    fn test_parse_retry_after_header_integer() {
        // Test parsing integer seconds format
        let mut response = hyper::Response::builder().status(429).body(()).unwrap();
        response.headers_mut().insert(
            "retry-after",
            hyper::header::HeaderValue::from_static("120"),
        );

        let retry_after = parse_retry_after_header(&response);
        assert_eq!(retry_after, 120);
    }

    #[test]
    fn test_parse_retry_after_header_missing() {
        // Test default value when header is missing
        let response = hyper::Response::builder().status(429).body(()).unwrap();

        let retry_after = parse_retry_after_header(&response);
        assert_eq!(retry_after, 5); // Default value
    }

    #[test]
    fn test_parse_retry_after_header_invalid() {
        // Test default value when header is invalid
        let mut response = hyper::Response::builder().status(429).body(()).unwrap();
        response.headers_mut().insert(
            "retry-after",
            hyper::header::HeaderValue::from_static("invalid"),
        );

        let retry_after = parse_retry_after_header(&response);
        assert_eq!(retry_after, 5); // Default value
    }

    #[test]
    fn test_parse_retry_after_header_http_date() {
        // Test parsing HTTP date format
        // Note: This test uses a date in the future
        let mut response = hyper::Response::builder().status(429).body(()).unwrap();

        // Create a date 60 seconds in the future
        let future_time = std::time::SystemTime::now() + std::time::Duration::from_secs(60);
        let http_date = httpdate::fmt_http_date(future_time);

        response.headers_mut().insert(
            "retry-after",
            hyper::header::HeaderValue::from_str(&http_date).unwrap(),
        );

        let retry_after = parse_retry_after_header(&response);
        // Should be close to 60 seconds (allowing for some test execution time)
        assert!(
            retry_after >= 59 && retry_after <= 61,
            "Expected ~60, got {}",
            retry_after
        );
    }

    #[test]
    fn test_parse_retry_after_header_past_http_date() {
        // Test HTTP date in the past (should fall back to default)
        let mut response = hyper::Response::builder().status(429).body(()).unwrap();

        // Create a date in the past
        let past_time = std::time::SystemTime::now() - std::time::Duration::from_secs(60);
        let http_date = httpdate::fmt_http_date(past_time);

        response.headers_mut().insert(
            "retry-after",
            hyper::header::HeaderValue::from_str(&http_date).unwrap(),
        );

        let retry_after = parse_retry_after_header(&response);
        // Should fall back to default since past dates don't make sense
        assert_eq!(retry_after, 5);
    }

    #[test]
    fn test_parse_retry_after_header_zero() {
        // Test zero seconds (edge case)
        let mut response = hyper::Response::builder().status(429).body(()).unwrap();
        response
            .headers_mut()
            .insert("retry-after", hyper::header::HeaderValue::from_static("0"));

        let retry_after = parse_retry_after_header(&response);
        assert_eq!(retry_after, 0);
    }

    #[test]
    fn test_parse_retry_after_header_large_value() {
        // Test large retry-after value
        let mut response = hyper::Response::builder().status(429).body(()).unwrap();
        response.headers_mut().insert(
            "retry-after",
            hyper::header::HeaderValue::from_static("3600"),
        );

        let retry_after = parse_retry_after_header(&response);
        assert_eq!(retry_after, 3600);
    }
}
