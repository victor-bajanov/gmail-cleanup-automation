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
        )
    }

    /// Check if the error is permanent and should not be retried
    pub fn is_permanent(&self) -> bool {
        !self.is_transient()
    }
}

impl From<google_gmail1::Error> for GmailError {
    fn from(error: google_gmail1::Error) -> Self {
        match error {
            // HTTP response with status code (non-success responses)
            google_gmail1::Error::Failure(ref response) => {
                let status = response.status();
                let status_code = status.as_u16();
                let message = format!("HTTP {}: {}", status_code, status.canonical_reason().unwrap_or("Unknown"));

                match status_code {
                    // Rate limiting - transient
                    429 => GmailError::RateLimitExceeded { retry_after: 1 },
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
            google_gmail1::Error::BadRequest(ref err) => {
                GmailError::BadRequest(format!("{}", err))
            }
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
}
