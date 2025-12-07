//! Circuit breaker pattern implementation for Gmail API client
//!
//! Protects the Gmail API from cascading failures by tracking consecutive errors
//! and temporarily rejecting requests when a failure threshold is reached.
//!
//! # Circuit States
//!
//! - **Closed**: Normal operation, requests pass through
//! - **Open**: Threshold exceeded, requests are rejected immediately
//! - **HalfOpen**: Testing recovery, allows one request through to test if service recovered
//!
//! # Usage
//!
//! ```no_run
//! use gmail_automation::circuit_breaker::CircuitBreaker;
//! use gmail_automation::config::CircuitBreakerConfig;
//! use gmail_automation::error::GmailError;
//!
//! # async fn example() -> Result<(), GmailError> {
//! let config = CircuitBreakerConfig {
//!     enabled: true,
//!     failure_threshold: 5,
//!     reset_timeout_secs: 60,
//! };
//!
//! let breaker = CircuitBreaker::new(config);
//!
//! // Check if request should be allowed
//! breaker.check_request().await?;
//!
//! // Make your API request here...
//! let result: Result<(), GmailError> = Ok(());
//!
//! // Record the result
//! match result {
//!     Ok(_) => breaker.record_success().await,
//!     Err(ref e) => breaker.record_failure(e).await,
//! }
//! # Ok(())
//! # }
//! ```

use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tracing::{debug, warn};

use crate::config::CircuitBreakerConfig;
use crate::error::{GmailError, Result};

/// Circuit breaker state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    /// Normal operation - requests pass through
    Closed,
    /// Circuit is open - requests are rejected
    Open,
    /// Testing recovery - allow one request through
    HalfOpen,
}

/// Internal state tracking for circuit breaker
#[derive(Debug)]
struct CircuitBreakerState {
    /// Current circuit state
    state: CircuitState,
    /// Number of consecutive failures
    failure_count: u32,
    /// Number of consecutive successes (used in half-open state)
    success_count: u32,
    /// Time when circuit was opened
    opened_at: Option<Instant>,
    /// Configuration
    config: CircuitBreakerConfig,
}

impl CircuitBreakerState {
    fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            state: CircuitState::Closed,
            failure_count: 0,
            success_count: 0,
            opened_at: None,
            config,
        }
    }

    fn should_allow_request(&mut self) -> Result<()> {
        if !self.config.enabled {
            return Ok(());
        }

        match self.state {
            CircuitState::Closed => Ok(()),
            CircuitState::Open => {
                // Check if reset timeout has elapsed
                if let Some(opened_at) = self.opened_at {
                    let elapsed = opened_at.elapsed();
                    let timeout = Duration::from_secs(self.config.reset_timeout_secs);

                    if elapsed >= timeout {
                        // Transition to half-open to test recovery
                        debug!(
                            "Circuit breaker transitioning to half-open state after {:?}",
                            elapsed
                        );
                        self.state = CircuitState::HalfOpen;
                        self.success_count = 0;
                        Ok(())
                    } else {
                        let remaining = timeout - elapsed;
                        Err(GmailError::CircuitBreakerOpen {
                            message: format!(
                                "Circuit breaker is open after {} consecutive failures",
                                self.failure_count
                            ),
                            retry_after_secs: remaining.as_secs(),
                        })
                    }
                } else {
                    // Should never happen, but handle gracefully
                    warn!(
                        "Circuit breaker in open state but opened_at is None, resetting to closed"
                    );
                    self.state = CircuitState::Closed;
                    Ok(())
                }
            }
            CircuitState::HalfOpen => {
                // Allow request through to test recovery
                Ok(())
            }
        }
    }

    fn record_success(&mut self) {
        if !self.config.enabled {
            return;
        }

        match self.state {
            CircuitState::Closed => {
                // Reset failure count on success
                if self.failure_count > 0 {
                    debug!("Circuit breaker: resetting failure count after success");
                    self.failure_count = 0;
                }
            }
            CircuitState::HalfOpen => {
                // Success in half-open state - close the circuit
                debug!("Circuit breaker: request succeeded in half-open state, closing circuit");
                self.state = CircuitState::Closed;
                self.failure_count = 0;
                self.success_count = 0;
                self.opened_at = None;
            }
            CircuitState::Open => {
                // Should not happen as we check state before allowing request
                warn!("Circuit breaker: received success in open state, closing circuit");
                self.state = CircuitState::Closed;
                self.failure_count = 0;
                self.success_count = 0;
                self.opened_at = None;
            }
        }
    }

    fn record_failure(&mut self, error: &GmailError) {
        if !self.config.enabled {
            return;
        }

        // Only count transient errors (rate limits and server errors) as circuit breaker failures
        // Permanent errors (auth, bad request, etc.) should not affect circuit state
        if !should_count_as_failure(error) {
            return;
        }

        match self.state {
            CircuitState::Closed => {
                self.failure_count += 1;
                debug!(
                    "Circuit breaker: failure {}/{} in closed state",
                    self.failure_count, self.config.failure_threshold
                );

                if self.failure_count >= self.config.failure_threshold {
                    warn!(
                        "Circuit breaker: threshold reached ({} failures), opening circuit for {} seconds",
                        self.failure_count, self.config.reset_timeout_secs
                    );
                    self.state = CircuitState::Open;
                    self.opened_at = Some(Instant::now());
                }
            }
            CircuitState::HalfOpen => {
                // Failure in half-open state - reopen circuit
                warn!("Circuit breaker: request failed in half-open state, reopening circuit");
                self.state = CircuitState::Open;
                self.opened_at = Some(Instant::now());
                self.success_count = 0;
            }
            CircuitState::Open => {
                // Already open, just log
                debug!("Circuit breaker: failure recorded while circuit is already open");
            }
        }
    }

    fn get_state(&self) -> CircuitState {
        self.state
    }
}

/// Determine if an error should count towards circuit breaker failure threshold
fn should_count_as_failure(error: &GmailError) -> bool {
    matches!(
        error,
        GmailError::RateLimitExceeded { .. }
            | GmailError::RateLimitError(_)
            | GmailError::ServerError {
                status: 500..=599,
                ..
            }
            | GmailError::NetworkError(_)
    )
}

/// Circuit breaker implementation using Arc<Mutex<>> for thread-safety
#[derive(Clone)]
pub struct CircuitBreaker {
    state: Arc<Mutex<CircuitBreakerState>>,
}

impl CircuitBreaker {
    /// Create a new circuit breaker with the given configuration
    pub fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            state: Arc::new(Mutex::new(CircuitBreakerState::new(config))),
        }
    }

    /// Check if a request should be allowed through the circuit breaker
    pub async fn check_request(&self) -> Result<()> {
        let mut state = self.state.lock().await;
        state.should_allow_request()
    }

    /// Record a successful request
    pub async fn record_success(&self) {
        let mut state = self.state.lock().await;
        state.record_success();
    }

    /// Record a failed request
    pub async fn record_failure(&self, error: &GmailError) {
        let mut state = self.state.lock().await;
        state.record_failure(error);
    }

    /// Get the current circuit state
    pub async fn state(&self) -> CircuitState {
        let state = self.state.lock().await;
        state.get_state()
    }

    /// Execute a closure with circuit breaker protection
    ///
    /// This is a convenience method that checks the circuit state,
    /// executes the operation, and records success/failure.
    pub async fn call<F, T, E>(&self, mut operation: F) -> Result<T>
    where
        F: FnMut() -> std::pin::Pin<
            Box<dyn std::future::Future<Output = std::result::Result<T, E>> + Send>,
        >,
        E: Into<GmailError>,
    {
        // Check if request should be allowed
        self.check_request().await?;

        // Execute operation
        match operation().await {
            Ok(result) => {
                self.record_success().await;
                Ok(result)
            }
            Err(e) => {
                let error: GmailError = e.into();
                self.record_failure(&error).await;
                Err(error)
            }
        }
    }

    /// Reset the circuit breaker to closed state
    ///
    /// This is useful for testing or manual intervention
    pub async fn reset(&self) {
        let mut state = self.state.lock().await;
        state.state = CircuitState::Closed;
        state.failure_count = 0;
        state.success_count = 0;
        state.opened_at = None;
        debug!("Circuit breaker manually reset to closed state");
    }

    /// Get circuit breaker statistics
    pub async fn stats(&self) -> CircuitBreakerStats {
        let state = self.state.lock().await;
        CircuitBreakerStats {
            state: state.state,
            failure_count: state.failure_count,
            success_count: state.success_count,
            opened_at: state.opened_at,
        }
    }
}

/// Circuit breaker statistics
#[derive(Debug, Clone)]
pub struct CircuitBreakerStats {
    pub state: CircuitState,
    pub failure_count: u32,
    pub success_count: u32,
    pub opened_at: Option<Instant>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_circuit_breaker_closed_state() {
        let config = CircuitBreakerConfig {
            enabled: true,
            failure_threshold: 3,
            reset_timeout_secs: 1,
        };
        let breaker = CircuitBreaker::new(config);

        // Should allow requests in closed state
        assert!(breaker.check_request().await.is_ok());
        assert_eq!(breaker.state().await, CircuitState::Closed);
    }

    #[tokio::test]
    async fn test_circuit_breaker_opens_after_threshold() {
        let config = CircuitBreakerConfig {
            enabled: true,
            failure_threshold: 3,
            reset_timeout_secs: 1,
        };
        let breaker = CircuitBreaker::new(config);

        // Record failures to reach threshold
        for _ in 0..3 {
            breaker
                .record_failure(&GmailError::ServerError {
                    status: 500,
                    message: "Internal server error".to_string(),
                })
                .await;
        }

        // Circuit should be open
        assert_eq!(breaker.state().await, CircuitState::Open);

        // Requests should be rejected
        let result = breaker.check_request().await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            GmailError::CircuitBreakerOpen { .. }
        ));
    }

    #[tokio::test]
    async fn test_circuit_breaker_resets_failure_count_on_success() {
        let config = CircuitBreakerConfig {
            enabled: true,
            failure_threshold: 3,
            reset_timeout_secs: 1,
        };
        let breaker = CircuitBreaker::new(config);

        // Record some failures
        for _ in 0..2 {
            breaker
                .record_failure(&GmailError::ServerError {
                    status: 500,
                    message: "Internal server error".to_string(),
                })
                .await;
        }

        let stats = breaker.stats().await;
        assert_eq!(stats.failure_count, 2);

        // Record success
        breaker.record_success().await;

        // Failure count should be reset
        let stats = breaker.stats().await;
        assert_eq!(stats.failure_count, 0);
        assert_eq!(breaker.state().await, CircuitState::Closed);
    }

    #[tokio::test]
    async fn test_circuit_breaker_transitions_to_half_open() {
        let config = CircuitBreakerConfig {
            enabled: true,
            failure_threshold: 2,
            reset_timeout_secs: 1,
        };
        let breaker = CircuitBreaker::new(config);

        // Open the circuit
        for _ in 0..2 {
            breaker
                .record_failure(&GmailError::ServerError {
                    status: 500,
                    message: "Internal server error".to_string(),
                })
                .await;
        }

        assert_eq!(breaker.state().await, CircuitState::Open);

        // Wait for reset timeout
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Should transition to half-open
        assert!(breaker.check_request().await.is_ok());
        assert_eq!(breaker.state().await, CircuitState::HalfOpen);
    }

    #[tokio::test]
    async fn test_circuit_breaker_closes_on_success_in_half_open() {
        let config = CircuitBreakerConfig {
            enabled: true,
            failure_threshold: 2,
            reset_timeout_secs: 1,
        };
        let breaker = CircuitBreaker::new(config);

        // Open the circuit
        for _ in 0..2 {
            breaker
                .record_failure(&GmailError::ServerError {
                    status: 500,
                    message: "Internal server error".to_string(),
                })
                .await;
        }

        // Wait and transition to half-open
        tokio::time::sleep(Duration::from_secs(2)).await;
        breaker.check_request().await.ok();

        // Record success in half-open state
        breaker.record_success().await;

        // Should close circuit
        assert_eq!(breaker.state().await, CircuitState::Closed);
    }

    #[tokio::test]
    async fn test_circuit_breaker_reopens_on_failure_in_half_open() {
        let config = CircuitBreakerConfig {
            enabled: true,
            failure_threshold: 2,
            reset_timeout_secs: 1,
        };
        let breaker = CircuitBreaker::new(config);

        // Open the circuit
        for _ in 0..2 {
            breaker
                .record_failure(&GmailError::ServerError {
                    status: 500,
                    message: "Internal server error".to_string(),
                })
                .await;
        }

        // Wait and transition to half-open
        tokio::time::sleep(Duration::from_secs(2)).await;
        breaker.check_request().await.ok();

        // Record failure in half-open state
        breaker
            .record_failure(&GmailError::ServerError {
                status: 500,
                message: "Internal server error".to_string(),
            })
            .await;

        // Should reopen circuit
        assert_eq!(breaker.state().await, CircuitState::Open);
    }

    #[tokio::test]
    async fn test_circuit_breaker_disabled() {
        let config = CircuitBreakerConfig {
            enabled: false,
            failure_threshold: 2,
            reset_timeout_secs: 1,
        };
        let breaker = CircuitBreaker::new(config);

        // Record many failures
        for _ in 0..10 {
            breaker
                .record_failure(&GmailError::ServerError {
                    status: 500,
                    message: "Internal server error".to_string(),
                })
                .await;
        }

        // Should still allow requests (circuit breaker disabled)
        assert!(breaker.check_request().await.is_ok());
    }

    #[tokio::test]
    async fn test_circuit_breaker_only_counts_transient_errors() {
        let config = CircuitBreakerConfig {
            enabled: true,
            failure_threshold: 3,
            reset_timeout_secs: 1,
        };
        let breaker = CircuitBreaker::new(config);

        // Record permanent errors (should not affect circuit)
        breaker
            .record_failure(&GmailError::AuthError("Invalid token".to_string()))
            .await;
        breaker
            .record_failure(&GmailError::BadRequest("Invalid query".to_string()))
            .await;
        breaker
            .record_failure(&GmailError::Forbidden("Access denied".to_string()))
            .await;

        // Circuit should still be closed
        assert_eq!(breaker.state().await, CircuitState::Closed);
        let stats = breaker.stats().await;
        assert_eq!(stats.failure_count, 0);

        // Record transient errors
        breaker
            .record_failure(&GmailError::RateLimitExceeded { retry_after: 5 })
            .await;
        breaker
            .record_failure(&GmailError::ServerError {
                status: 500,
                message: "Internal error".to_string(),
            })
            .await;

        // Should count these failures
        let stats = breaker.stats().await;
        assert_eq!(stats.failure_count, 2);
    }

    #[tokio::test]
    async fn test_circuit_breaker_reset() {
        let config = CircuitBreakerConfig {
            enabled: true,
            failure_threshold: 2,
            reset_timeout_secs: 60,
        };
        let breaker = CircuitBreaker::new(config);

        // Open the circuit
        for _ in 0..2 {
            breaker
                .record_failure(&GmailError::ServerError {
                    status: 500,
                    message: "Internal server error".to_string(),
                })
                .await;
        }

        assert_eq!(breaker.state().await, CircuitState::Open);

        // Manually reset
        breaker.reset().await;

        // Should be closed and allow requests
        assert_eq!(breaker.state().await, CircuitState::Closed);
        assert!(breaker.check_request().await.is_ok());
        let stats = breaker.stats().await;
        assert_eq!(stats.failure_count, 0);
    }

    #[test]
    fn test_should_count_as_failure() {
        // Transient errors should count
        assert!(should_count_as_failure(&GmailError::RateLimitExceeded {
            retry_after: 5
        }));
        assert!(should_count_as_failure(&GmailError::ServerError {
            status: 500,
            message: "Error".to_string()
        }));
        assert!(should_count_as_failure(&GmailError::ServerError {
            status: 503,
            message: "Error".to_string()
        }));
        assert!(should_count_as_failure(&GmailError::NetworkError(
            "Connection error".to_string()
        )));

        // Permanent errors should not count
        assert!(!should_count_as_failure(&GmailError::AuthError(
            "Invalid token".to_string()
        )));
        assert!(!should_count_as_failure(&GmailError::BadRequest(
            "Invalid query".to_string()
        )));
        assert!(!should_count_as_failure(&GmailError::Forbidden(
            "Access denied".to_string()
        )));
        assert!(!should_count_as_failure(&GmailError::MessageNotFound(
            "Not found".to_string()
        )));
    }

    #[tokio::test]
    async fn test_circuit_breaker_stats() {
        let config = CircuitBreakerConfig {
            enabled: true,
            failure_threshold: 3,
            reset_timeout_secs: 1,
        };
        let breaker = CircuitBreaker::new(config);

        // Initial stats
        let stats = breaker.stats().await;
        assert_eq!(stats.state, CircuitState::Closed);
        assert_eq!(stats.failure_count, 0);
        assert_eq!(stats.success_count, 0);
        assert!(stats.opened_at.is_none());

        // Record failures
        for _ in 0..2 {
            breaker
                .record_failure(&GmailError::ServerError {
                    status: 500,
                    message: "Internal server error".to_string(),
                })
                .await;
        }

        let stats = breaker.stats().await;
        assert_eq!(stats.failure_count, 2);

        // Open circuit
        breaker
            .record_failure(&GmailError::ServerError {
                status: 500,
                message: "Internal server error".to_string(),
            })
            .await;

        let stats = breaker.stats().await;
        assert_eq!(stats.state, CircuitState::Open);
        assert!(stats.opened_at.is_some());
    }
}
