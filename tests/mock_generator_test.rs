//! Integration tests for the mock_generator module
//!
//! This file demonstrates how to use the mock email generator utilities
//! for performance testing.

mod performance;

use performance::mock_generator::*;

#[test]
fn test_basic_mock_generation() {
    let messages = generate_mock_emails(100);
    assert_eq!(messages.len(), 100);

    // Verify all messages have required fields
    for msg in &messages {
        assert!(!msg.id.is_empty());
        assert!(!msg.sender_email.is_empty());
        assert!(!msg.subject.is_empty());
        assert!(msg.sender_email.contains('@'));
    }
}

#[test]
fn test_seeded_generation() {
    let messages1 = generate_mock_emails_with_seed(50, 12345);
    let messages2 = generate_mock_emails_with_seed(50, 12345);

    // Same seed should produce identical results
    for (m1, m2) in messages1.iter().zip(messages2.iter()) {
        assert_eq!(m1.sender_email, m2.sender_email);
        assert_eq!(m1.subject, m2.subject);
    }
}

#[test]
fn test_large_batch_generation() {
    // Test generating a large batch for performance testing
    let messages = generate_mock_emails_with_seed(10000, 42);
    assert_eq!(messages.len(), 10000);

    // Verify variety
    let unique_domains: std::collections::HashSet<_> =
        messages.iter().map(|m| &m.sender_domain).collect();
    assert!(unique_domains.len() > 20, "Should have variety of domains");
}
