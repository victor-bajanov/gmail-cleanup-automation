//! Quota-aware rate limiter for Gmail API
//!
//! Gmail API uses a quota system based on "quota units" per second:
//! - Default: 250 quota units per user per second
//! - Read operations: 5 units (messages.get, labels.list, filters.list)
//! - Write operations: 50 units (labels.create, filters.create)
//! - Batch modify: 50 units
//!
//! This module implements a token bucket algorithm that:
//! - Tracks quota units consumed over time
//! - Refills at Google's rate limit
//! - Allows bursting when quota is available
//! - Blocks when quota is exhausted

use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tracing::{debug, trace};

/// Gmail API quota costs for different operations
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuotaCost {
    /// Read operations: messages.get, messages.list, labels.list, filters.list
    /// Cost: 5 quota units
    Read,
    /// Write operations: labels.create, filters.create, messages.modify
    /// Cost: 50 quota units
    Write,
    /// Batch operations: messages.batchModify
    /// Cost: 50 quota units
    Batch,
    /// Custom cost for special operations
    Custom(u32),
}

impl QuotaCost {
    /// Get the quota unit cost for this operation type
    pub fn units(&self) -> u32 {
        match self {
            QuotaCost::Read => 5,
            QuotaCost::Write => 50,
            QuotaCost::Batch => 50,
            QuotaCost::Custom(units) => *units,
        }
    }
}

/// Token bucket rate limiter for Gmail API quota management
///
/// This implementation uses a token bucket algorithm where:
/// - Tokens represent available quota units
/// - Tokens are added at `refill_rate` per second
/// - Each operation consumes tokens based on its quota cost
/// - Maximum bucket size limits burst capacity
#[derive(Debug)]
pub struct QuotaRateLimiter {
    inner: Arc<Mutex<RateLimiterState>>,
}

#[derive(Debug)]
struct RateLimiterState {
    /// Current available quota units (tokens)
    available_units: f64,
    /// Maximum quota units that can be stored (burst capacity)
    max_units: f64,
    /// Quota units added per second
    refill_rate: f64,
    /// Last time we refilled the bucket
    last_refill: Instant,
    /// Total units consumed (for stats)
    total_consumed: u64,
    /// Total operations performed (for stats)
    total_operations: u64,
}

impl QuotaRateLimiter {
    /// Create a new rate limiter with Gmail's default quota limits
    ///
    /// Default settings:
    /// - 250 quota units per second refill rate
    /// - 500 unit burst capacity (2 seconds worth)
    pub fn new() -> Self {
        Self::with_config(250.0, 500.0)
    }

    /// Create a rate limiter with custom configuration
    ///
    /// # Arguments
    /// * `refill_rate` - Quota units added per second
    /// * `max_units` - Maximum burst capacity
    pub fn with_config(refill_rate: f64, max_units: f64) -> Self {
        Self {
            inner: Arc::new(Mutex::new(RateLimiterState {
                available_units: max_units, // Start with full bucket
                max_units,
                refill_rate,
                last_refill: Instant::now(),
                total_consumed: 0,
                total_operations: 0,
            })),
        }
    }

    /// Acquire quota units for an operation, waiting if necessary
    ///
    /// This method will:
    /// 1. Refill the bucket based on elapsed time
    /// 2. If enough units available, consume them immediately
    /// 3. If not enough units, wait until they become available
    ///
    /// # Arguments
    /// * `cost` - The quota cost of the operation
    ///
    /// # Returns
    /// A guard that can be used to track the operation (currently just returns ())
    pub async fn acquire(&self, cost: QuotaCost) -> QuotaPermit {
        let units_needed = cost.units() as f64;

        loop {
            let wait_time = {
                let mut state = self.inner.lock().await;

                // Refill bucket based on elapsed time
                let now = Instant::now();
                let elapsed = now.duration_since(state.last_refill).as_secs_f64();
                let refill_amount = elapsed * state.refill_rate;
                state.available_units = (state.available_units + refill_amount).min(state.max_units);
                state.last_refill = now;

                trace!(
                    "Quota state: {:.1}/{:.1} units available, requesting {:.0}",
                    state.available_units,
                    state.max_units,
                    units_needed
                );

                if state.available_units >= units_needed {
                    // We have enough quota, consume it
                    state.available_units -= units_needed;
                    state.total_consumed += units_needed as u64;
                    state.total_operations += 1;

                    debug!(
                        "Acquired {} quota units, {:.1} remaining",
                        units_needed,
                        state.available_units
                    );

                    return QuotaPermit { _private: () };
                }

                // Calculate how long to wait for enough quota
                let units_deficit = units_needed - state.available_units;
                let wait_seconds = units_deficit / state.refill_rate;
                Duration::from_secs_f64(wait_seconds)
            };

            // Wait outside the lock to allow other operations to proceed
            debug!(
                "Quota exhausted, waiting {:.2}s for {} units",
                wait_time.as_secs_f64(),
                units_needed
            );
            tokio::time::sleep(wait_time).await;
        }
    }

    /// Try to acquire quota units without waiting
    ///
    /// Returns `Some(QuotaPermit)` if quota was available, `None` otherwise
    pub async fn try_acquire(&self, cost: QuotaCost) -> Option<QuotaPermit> {
        let units_needed = cost.units() as f64;
        let mut state = self.inner.lock().await;

        // Refill bucket
        let now = Instant::now();
        let elapsed = now.duration_since(state.last_refill).as_secs_f64();
        let refill_amount = elapsed * state.refill_rate;
        state.available_units = (state.available_units + refill_amount).min(state.max_units);
        state.last_refill = now;

        if state.available_units >= units_needed {
            state.available_units -= units_needed;
            state.total_consumed += units_needed as u64;
            state.total_operations += 1;
            Some(QuotaPermit { _private: () })
        } else {
            None
        }
    }

    /// Get current statistics about quota usage
    pub async fn stats(&self) -> QuotaStats {
        let state = self.inner.lock().await;
        QuotaStats {
            available_units: state.available_units as u32,
            max_units: state.max_units as u32,
            refill_rate: state.refill_rate as u32,
            total_consumed: state.total_consumed,
            total_operations: state.total_operations,
        }
    }

    /// Check current available quota without consuming any
    pub async fn available(&self) -> f64 {
        let mut state = self.inner.lock().await;

        // Refill bucket based on elapsed time
        let now = Instant::now();
        let elapsed = now.duration_since(state.last_refill).as_secs_f64();
        let refill_amount = elapsed * state.refill_rate;
        state.available_units = (state.available_units + refill_amount).min(state.max_units);
        state.last_refill = now;

        state.available_units
    }
}

impl Default for QuotaRateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for QuotaRateLimiter {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

/// A permit representing acquired quota
///
/// This is returned from `acquire()` and can be used for tracking.
/// Currently just a marker type.
#[derive(Debug)]
pub struct QuotaPermit {
    _private: (),
}

/// Statistics about quota usage
#[derive(Debug, Clone)]
pub struct QuotaStats {
    /// Currently available quota units
    pub available_units: u32,
    /// Maximum burst capacity
    pub max_units: u32,
    /// Refill rate (units per second)
    pub refill_rate: u32,
    /// Total quota units consumed since creation
    pub total_consumed: u64,
    /// Total operations performed
    pub total_operations: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quota_cost_units() {
        assert_eq!(QuotaCost::Read.units(), 5);
        assert_eq!(QuotaCost::Write.units(), 50);
        assert_eq!(QuotaCost::Batch.units(), 50);
        assert_eq!(QuotaCost::Custom(100).units(), 100);
    }

    #[tokio::test]
    async fn test_acquire_immediate() {
        // Create limiter with 100 units capacity
        let limiter = QuotaRateLimiter::with_config(100.0, 100.0);

        // Should be able to acquire 5 units immediately (starts full)
        let _permit = limiter.acquire(QuotaCost::Read).await;

        let stats = limiter.stats().await;
        assert_eq!(stats.total_operations, 1);
        assert_eq!(stats.total_consumed, 5);
    }

    #[tokio::test]
    async fn test_try_acquire_success() {
        let limiter = QuotaRateLimiter::with_config(100.0, 100.0);

        let permit = limiter.try_acquire(QuotaCost::Read).await;
        assert!(permit.is_some());
    }

    #[tokio::test]
    async fn test_try_acquire_insufficient_quota() {
        // Create limiter with very low capacity
        let limiter = QuotaRateLimiter::with_config(1.0, 2.0);

        // Try to acquire 5 units when only 2 available
        let permit = limiter.try_acquire(QuotaCost::Read).await;
        assert!(permit.is_none());
    }

    #[tokio::test]
    async fn test_acquire_waits_for_quota() {
        // Create limiter with 100 units/sec refill
        let limiter = QuotaRateLimiter::with_config(100.0, 10.0);

        // Exhaust the bucket
        for _ in 0..2 {
            let _ = limiter.acquire(QuotaCost::Read).await; // 5 units each
        }

        // Now bucket should be at 0, next acquire should wait
        let start = Instant::now();
        let _ = limiter.acquire(QuotaCost::Read).await;
        let elapsed = start.elapsed();

        // Should have waited ~50ms (5 units / 100 units per sec)
        assert!(elapsed.as_millis() >= 40, "Should have waited for quota refill");
    }

    #[tokio::test]
    async fn test_refill_over_time() {
        let limiter = QuotaRateLimiter::with_config(100.0, 100.0);

        // Consume all quota
        for _ in 0..20 {
            let _ = limiter.acquire(QuotaCost::Read).await;
        }

        // Wait for refill
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Should have refilled ~50 units
        let available = limiter.available().await;
        assert!(available >= 40.0 && available <= 60.0, "Should have refilled ~50 units, got {}", available);
    }

    #[tokio::test]
    async fn test_stats() {
        let limiter = QuotaRateLimiter::with_config(100.0, 100.0);

        let _ = limiter.acquire(QuotaCost::Read).await;
        let _ = limiter.acquire(QuotaCost::Write).await;

        let stats = limiter.stats().await;
        assert_eq!(stats.total_operations, 2);
        assert_eq!(stats.total_consumed, 55); // 5 + 50
        assert_eq!(stats.refill_rate, 100);
        assert_eq!(stats.max_units, 100);
    }

    #[tokio::test]
    async fn test_clone_shares_state() {
        let limiter1 = QuotaRateLimiter::with_config(100.0, 100.0);
        let limiter2 = limiter1.clone();

        // Consume via limiter1
        let _ = limiter1.acquire(QuotaCost::Read).await;

        // Stats should be visible via limiter2
        let stats = limiter2.stats().await;
        assert_eq!(stats.total_operations, 1);
    }
}
