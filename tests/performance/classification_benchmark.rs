//! Classification throughput benchmarks
//!
//! These benchmarks validate the README claim:
//! "Classification: ~10,000 messages/second (CPU-bound, no API calls)"
//!
//! The benchmarks test the EmailClassifier's performance at different scales
//! to ensure it can handle high-throughput classification workloads.

use gmail_automation::classifier::EmailClassifier;
use serial_test::serial;
use std::time::Instant;

use super::mock_generator::generate_mock_emails;

/// Format throughput results in a clear, readable format
fn format_benchmark_result(count: usize, duration: std::time::Duration) -> String {
    let duration_secs = duration.as_secs_f64();
    let msgs_per_sec = count as f64 / duration_secs;

    format!(
        "\n  Messages: {}\n  Duration: {:.3}s\n  Throughput: {:.0} msgs/sec",
        count, duration_secs, msgs_per_sec
    )
}

/// Calculate messages per second from count and duration
fn calculate_throughput(count: usize, duration: std::time::Duration) -> f64 {
    count as f64 / duration.as_secs_f64()
}

#[test]
#[serial]
fn test_classification_baseline_10k() {
    println!("\n=== Classification Benchmark: 10,000 messages ===");

    // Generate test data
    println!("Generating 10,000 mock emails...");
    let messages = generate_mock_emails(10_000);
    assert_eq!(messages.len(), 10_000, "Should generate exactly 10,000 messages");

    // Create classifier
    let classifier = EmailClassifier::new("auto".to_string());

    // Warm-up: classify a few messages to ensure caches are initialized
    println!("Warming up classifier...");
    for message in messages.iter().take(100) {
        let _ = classifier.classify(message);
    }

    // Benchmark classification
    println!("Starting classification benchmark...");
    let start = Instant::now();

    let mut success_count = 0;
    for message in &messages {
        match classifier.classify(message) {
            Ok(_) => success_count += 1,
            Err(e) => panic!("Classification failed: {:?}", e),
        }
    }

    let duration = start.elapsed();

    // Calculate throughput
    let throughput = calculate_throughput(messages.len(), duration);

    // Print results
    println!("{}", format_benchmark_result(messages.len(), duration));
    println!("  Success rate: {}/{} ({}%)",
        success_count,
        messages.len(),
        (success_count as f64 / messages.len() as f64 * 100.0) as u32
    );

    // Assert performance threshold
    // Conservative threshold: claim is 10,000 msgs/sec, we assert >= 5,000
    assert!(
        throughput >= 5000.0,
        "Classification throughput ({:.0} msgs/sec) is below minimum threshold (5,000 msgs/sec). Expected ~10,000 msgs/sec.",
        throughput
    );

    println!("\n  Status: PASS - Throughput exceeds minimum threshold");

    // Bonus: check if we meet the claimed 10k threshold
    if throughput >= 10_000.0 {
        println!("  Bonus: Meets claimed 10,000 msgs/sec threshold!");
    } else {
        println!("  Note: Below claimed 10,000 msgs/sec, but above minimum requirement");
    }
}

#[test]
#[serial]
fn test_classification_stress_15k() {
    println!("\n=== Classification Stress Test: 15,000 messages ===");

    // Generate larger test dataset
    println!("Generating 15,000 mock emails...");
    let messages = generate_mock_emails(15_000);
    assert_eq!(messages.len(), 15_000);

    // Create classifier
    let classifier = EmailClassifier::new("auto".to_string());

    // Warm-up
    println!("Warming up classifier...");
    for message in messages.iter().take(100) {
        let _ = classifier.classify(message);
    }

    // Benchmark
    println!("Starting stress test classification...");
    let start = Instant::now();

    let mut success_count = 0;
    for message in &messages {
        match classifier.classify(message) {
            Ok(_) => success_count += 1,
            Err(e) => panic!("Classification failed at stress test: {:?}", e),
        }
    }

    let duration = start.elapsed();
    let throughput = calculate_throughput(messages.len(), duration);

    // Print results
    println!("{}", format_benchmark_result(messages.len(), duration));
    println!("  Success rate: {}/{} ({}%)",
        success_count,
        messages.len(),
        (success_count as f64 / messages.len() as f64 * 100.0) as u32
    );

    // Assert threshold (same as baseline)
    assert!(
        throughput >= 5000.0,
        "Stress test throughput ({:.0} msgs/sec) is below threshold (5,000 msgs/sec)",
        throughput
    );

    println!("\n  Status: PASS - Maintains throughput at 15k scale");

    if throughput >= 10_000.0 {
        println!("  Bonus: Maintains claimed 10,000 msgs/sec at 15k scale!");
    }
}

#[test]
#[serial]
fn test_classification_extreme_25k() {
    println!("\n=== Classification Extreme Test: 25,000 messages ===");

    // Generate extreme test dataset
    println!("Generating 25,000 mock emails...");
    let messages = generate_mock_emails(25_000);
    assert_eq!(messages.len(), 25_000);

    // Create classifier
    let classifier = EmailClassifier::new("auto".to_string());

    // Warm-up
    println!("Warming up classifier...");
    for message in messages.iter().take(100) {
        let _ = classifier.classify(message);
    }

    // Benchmark
    println!("Starting extreme classification test...");
    let start = Instant::now();

    let mut success_count = 0;
    for message in &messages {
        match classifier.classify(message) {
            Ok(_) => success_count += 1,
            Err(e) => panic!("Classification failed at extreme test: {:?}", e),
        }
    }

    let duration = start.elapsed();
    let throughput = calculate_throughput(messages.len(), duration);

    // Print results
    println!("{}", format_benchmark_result(messages.len(), duration));
    println!("  Success rate: {}/{} ({}%)",
        success_count,
        messages.len(),
        (success_count as f64 / messages.len() as f64 * 100.0) as u32
    );

    // Assert threshold (same as baseline)
    assert!(
        throughput >= 5000.0,
        "Extreme test throughput ({:.0} msgs/sec) is below threshold (5,000 msgs/sec)",
        throughput
    );

    println!("\n  Status: PASS - Maintains throughput at 25k scale");

    if throughput >= 10_000.0 {
        println!("  Bonus: Maintains claimed 10,000 msgs/sec at 25k scale!");
    } else if throughput >= 8_000.0 {
        println!("  Good: Achieves 8,000+ msgs/sec at 25k scale");
    }
}

#[test]
#[serial]
fn test_classification_category_distribution() {
    println!("\n=== Classification Category Distribution Test ===");
    println!("Testing that classifier properly categorizes diverse email types...");

    // Generate test data
    let messages = generate_mock_emails(1_000);
    let classifier = EmailClassifier::new("auto".to_string());

    // Track category distribution
    use std::collections::HashMap;
    let mut category_counts: HashMap<String, usize> = HashMap::new();

    let start = Instant::now();

    for message in &messages {
        let classification = classifier.classify(message)
            .expect("Classification should succeed");

        let category_name = format!("{:?}", classification.category);
        *category_counts.entry(category_name).or_insert(0) += 1;
    }

    let duration = start.elapsed();
    let throughput = calculate_throughput(messages.len(), duration);

    println!("\nCategory distribution:");
    let mut sorted_categories: Vec<_> = category_counts.iter().collect();
    sorted_categories.sort_by_key(|(_, count)| std::cmp::Reverse(**count));

    for (category, count) in sorted_categories {
        let percentage = (*count as f64 / messages.len() as f64) * 100.0;
        println!("  {:20} {:4} ({:5.1}%)", category, count, percentage);
    }

    println!("\nThroughput: {:.0} msgs/sec", throughput);

    // Verify we have multiple categories (classifier is actually categorizing, not just returning one type)
    assert!(
        category_counts.len() >= 3,
        "Classifier should produce multiple categories, got only: {:?}",
        category_counts.keys()
    );

    println!("\n  Status: PASS - Classifier produces diverse categories");
}

#[test]
#[serial]
fn test_classification_consistency() {
    println!("\n=== Classification Consistency Test ===");
    println!("Testing that classifier produces consistent results for the same input...");

    // Generate test messages
    let messages = generate_mock_emails(100);
    let classifier = EmailClassifier::new("auto".to_string());

    // Classify twice and compare
    let mut first_results = Vec::new();
    let mut second_results = Vec::new();

    for message in &messages {
        let result1 = classifier.classify(message)
            .expect("First classification should succeed");
        first_results.push(result1);
    }

    for message in &messages {
        let result2 = classifier.classify(message)
            .expect("Second classification should succeed");
        second_results.push(result2);
    }

    // Compare results
    let mut consistent_count = 0;
    for (r1, r2) in first_results.iter().zip(second_results.iter()) {
        if r1.category == r2.category
            && r1.suggested_label == r2.suggested_label
            && r1.should_archive == r2.should_archive {
            consistent_count += 1;
        }
    }

    let consistency_rate = (consistent_count as f64 / messages.len() as f64) * 100.0;
    println!("\nConsistency: {}/{} ({:.1}%)", consistent_count, messages.len(), consistency_rate);

    // Should be 100% consistent (deterministic)
    assert_eq!(
        consistent_count,
        messages.len(),
        "Classifier should be deterministic and produce identical results for identical input"
    );

    println!("  Status: PASS - Classifier is deterministic");
}
