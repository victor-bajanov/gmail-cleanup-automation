//! Integration tests for state file size performance
//!
//! This file validates the README claim: "State file: ~1 KB per 1,000 messages"

mod performance;

// Re-export the tests from the performance module
// The actual test implementations are in tests/performance/state_file_size_test.rs
