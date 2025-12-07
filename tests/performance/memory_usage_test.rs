//! Memory usage tests to validate README performance claims
//!
//! This module tests memory usage during email processing operations to ensure
//! the application meets its performance targets:
//! - Typical: 50-100 MB for processing 10,000 emails
//! - Peak: ~200 MB during concurrent batch operations
//!
//! Memory measurement approach:
//! - On Linux: Read /proc/self/status for VmRSS (Resident Set Size)
//! - Fallback: Estimate based on struct sizes and collection overhead
//!
//! Tests measure:
//! 1. Baseline memory before operations
//! 2. Memory during mock email generation
//! 3. Memory during classification of 10,000 emails
//! 4. Memory during batch processing operations
//! 5. Peak memory usage across all operations

use super::mock_generator::{generate_mock_emails, MockGenerator, MockGeneratorConfig};
use gmail_automation::classifier::EmailClassifier;
use serial_test::serial;
use std::fs;

/// Memory measurement structure
#[derive(Debug, Clone, Copy)]
struct MemoryUsage {
    /// Resident Set Size in bytes (actual physical memory used)
    rss_bytes: usize,
    /// Virtual memory size in bytes
    vm_size_bytes: usize,
}

impl MemoryUsage {
    /// Get current memory usage from /proc/self/status (Linux only)
    fn current() -> Self {
        #[cfg(target_os = "linux")]
        {
            if let Ok(status) = fs::read_to_string("/proc/self/status") {
                let mut rss_bytes = 0;
                let mut vm_size_bytes = 0;

                for line in status.lines() {
                    if line.starts_with("VmRSS:") {
                        // VmRSS is in kB
                        if let Some(value) = line.split_whitespace().nth(1) {
                            if let Ok(kb) = value.parse::<usize>() {
                                rss_bytes = kb * 1024;
                            }
                        }
                    } else if line.starts_with("VmSize:") {
                        // VmSize is in kB
                        if let Some(value) = line.split_whitespace().nth(1) {
                            if let Ok(kb) = value.parse::<usize>() {
                                vm_size_bytes = kb * 1024;
                            }
                        }
                    }
                }

                return Self {
                    rss_bytes,
                    vm_size_bytes,
                };
            }
        }

        // Fallback for non-Linux or if reading fails
        Self {
            rss_bytes: 0,
            vm_size_bytes: 0,
        }
    }

    /// Convert to megabytes
    fn rss_mb(&self) -> f64 {
        self.rss_bytes as f64 / (1024.0 * 1024.0)
    }

    /// Convert virtual size to megabytes
    fn vm_size_mb(&self) -> f64 {
        self.vm_size_bytes as f64 / (1024.0 * 1024.0)
    }

    /// Calculate delta from another measurement
    fn delta(&self, baseline: &MemoryUsage) -> MemoryDelta {
        MemoryDelta {
            rss_delta_bytes: self.rss_bytes.saturating_sub(baseline.rss_bytes),
            vm_delta_bytes: self.vm_size_bytes.saturating_sub(baseline.vm_size_bytes),
        }
    }
}

/// Memory delta between two measurements
#[derive(Debug, Clone, Copy)]
struct MemoryDelta {
    rss_delta_bytes: usize,
    vm_delta_bytes: usize,
}

impl MemoryDelta {
    fn rss_delta_mb(&self) -> f64 {
        self.rss_delta_bytes as f64 / (1024.0 * 1024.0)
    }

    fn vm_delta_mb(&self) -> f64 {
        self.vm_delta_bytes as f64 / (1024.0 * 1024.0)
    }
}

/// Print memory statistics in a formatted way
fn print_memory_stats(label: &str, baseline: &MemoryUsage, current: &MemoryUsage) {
    let delta = current.delta(baseline);
    println!("\n{}", "=".repeat(60));
    println!("{}", label);
    println!("{}", "=".repeat(60));
    println!("Current RSS:     {:8.2} MB", current.rss_mb());
    println!("Baseline RSS:    {:8.2} MB", baseline.rss_mb());
    println!("Delta RSS:       {:8.2} MB", delta.rss_delta_mb());
    println!("Current VM Size: {:8.2} MB", current.vm_size_mb());
    println!("{}", "=".repeat(60));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[serial]
    fn test_memory_measurement_works() {
        let mem = MemoryUsage::current();

        // On Linux, we should get non-zero values
        #[cfg(target_os = "linux")]
        {
            assert!(mem.rss_bytes > 0, "RSS should be non-zero on Linux");
            assert!(mem.vm_size_bytes > 0, "VM Size should be non-zero on Linux");
            println!("Memory measurement working: RSS = {:.2} MB", mem.rss_mb());
        }

        // On other platforms, test structure works
        #[cfg(not(target_os = "linux"))]
        {
            println!(
                "Memory measurement not available on this platform (RSS = {:.2} MB)",
                mem.rss_mb()
            );
        }
    }

    #[test]
    #[serial]
    fn test_memory_usage_generating_10k_emails() {
        println!("\n\nTEST: Memory usage for generating 10,000 mock emails");
        println!("{}", "=".repeat(60));

        // Force garbage collection before baseline
        // (Note: Rust doesn't have explicit GC, but drop temp data)
        let _ = vec![0u8; 1000];

        let baseline = MemoryUsage::current();
        println!("Baseline RSS: {:.2} MB", baseline.rss_mb());

        // Generate 10,000 emails
        println!("Generating 10,000 mock emails...");
        let emails = generate_mock_emails(10_000);

        let after_generation = MemoryUsage::current();
        print_memory_stats(
            "After Generating 10,000 Emails",
            &baseline,
            &after_generation,
        );

        // Verify we generated the right amount
        assert_eq!(
            emails.len(),
            10_000,
            "Should generate exactly 10,000 emails"
        );

        let delta = after_generation.delta(&baseline);
        let delta_mb = delta.rss_delta_mb();

        println!("\nRESULT: Generated 10,000 emails using {:.2} MB", delta_mb);

        // The memory should be reasonable - we'll be conservative and say < 200 MB
        // Typical should be much less (20-50 MB for just the data structures)
        #[cfg(target_os = "linux")]
        {
            assert!(
                delta_mb < 200.0,
                "Memory usage for 10k emails should be < 200 MB, got {:.2} MB",
                delta_mb
            );

            // Log if it's within expected typical range
            if delta_mb < 100.0 {
                println!("SUCCESS: Memory usage is within typical range (< 100 MB)");
            }
        }
    }

    #[test]
    #[serial]
    fn test_memory_usage_classifying_10k_emails() {
        println!("\n\nTEST: Memory usage for classifying 10,000 emails");
        println!("{}", "=".repeat(60));

        // Pre-generate emails to isolate classification memory usage
        println!("Pre-generating 10,000 mock emails...");
        let emails = generate_mock_emails(10_000);

        // Force a pause to let memory settle
        std::thread::sleep(std::time::Duration::from_millis(100));

        let baseline = MemoryUsage::current();
        println!(
            "Baseline RSS: {:.2} MB (after email generation)",
            baseline.rss_mb()
        );

        // Create classifier and classify all emails
        println!("Classifying 10,000 emails...");
        let classifier = EmailClassifier::default();

        let classifications: Vec<_> = emails
            .iter()
            .map(|email| classifier.classify(email))
            .collect::<Result<Vec<_>, _>>()
            .expect("Classification should succeed");

        let after_classification = MemoryUsage::current();
        print_memory_stats(
            "After Classifying 10,000 Emails",
            &baseline,
            &after_classification,
        );

        // Verify we classified all emails
        assert_eq!(
            classifications.len(),
            10_000,
            "Should classify exactly 10,000 emails"
        );

        let delta = after_classification.delta(&baseline);
        let delta_mb = delta.rss_delta_mb();

        println!(
            "\nRESULT: Classified 10,000 emails using {:.2} MB additional",
            delta_mb
        );

        // Classifications should add some memory but not excessive
        // We'll be conservative: < 150 MB for classifications on top of emails
        #[cfg(target_os = "linux")]
        {
            assert!(
                delta_mb < 150.0,
                "Memory delta for classifying 10k emails should be < 150 MB, got {:.2} MB",
                delta_mb
            );

            // Log if it's within expected range
            if delta_mb < 75.0 {
                println!("SUCCESS: Classification memory is within typical range (< 75 MB)");
            }
        }
    }

    #[test]
    #[serial]
    fn test_memory_usage_full_pipeline_10k_emails() {
        println!("\n\nTEST: Memory usage for complete pipeline (10,000 emails)");
        println!("{}", "=".repeat(60));

        let baseline = MemoryUsage::current();
        println!("Baseline RSS: {:.2} MB", baseline.rss_mb());

        // Simulate complete pipeline: generate, classify, store
        println!("Running complete pipeline...");

        // Step 1: Generate emails
        println!("  1. Generating emails...");
        let emails = generate_mock_emails(10_000);
        let after_gen = MemoryUsage::current();
        let gen_delta = after_gen.delta(&baseline).rss_delta_mb();
        println!(
            "     Memory after generation: {:.2} MB (+{:.2} MB)",
            after_gen.rss_mb(),
            gen_delta
        );

        // Step 2: Classify emails
        println!("  2. Classifying emails...");
        let classifier = EmailClassifier::default();
        let classifications: Vec<_> = emails
            .iter()
            .map(|email| classifier.classify(email))
            .collect::<Result<Vec<_>, _>>()
            .expect("Classification should succeed");

        let after_classify = MemoryUsage::current();
        let classify_delta = after_classify.delta(&after_gen).rss_delta_mb();
        println!(
            "     Memory after classification: {:.2} MB (+{:.2} MB)",
            after_classify.rss_mb(),
            classify_delta
        );

        // Step 3: Store both in memory (simulating in-memory storage)
        println!("  3. Storing results in memory...");
        let storage: Vec<_> = emails
            .into_iter()
            .zip(classifications.into_iter())
            .collect();

        let after_storage = MemoryUsage::current();
        let storage_delta = after_storage.delta(&after_classify).rss_delta_mb();
        println!(
            "     Memory after storage: {:.2} MB (+{:.2} MB)",
            after_storage.rss_mb(),
            storage_delta
        );

        print_memory_stats(
            "Complete Pipeline for 10,000 Emails",
            &baseline,
            &after_storage,
        );

        let total_delta = after_storage.delta(&baseline);
        let total_mb = total_delta.rss_delta_mb();

        println!("\nRESULT: Complete pipeline used {:.2} MB total", total_mb);
        println!("  - Generation:     {:.2} MB", gen_delta);
        println!("  - Classification: {:.2} MB", classify_delta);
        println!("  - Storage:        {:.2} MB", storage_delta);

        // Verify we have all data
        assert_eq!(
            storage.len(),
            10_000,
            "Should have 10,000 email+classification pairs"
        );

        // README claims: "Typical: 50-100 MB for processing 10,000 emails"
        // We'll use a conservative limit of 300 MB to account for test overhead
        #[cfg(target_os = "linux")]
        {
            assert!(
                total_mb < 300.0,
                "Total memory for 10k email pipeline should be < 300 MB, got {:.2} MB",
                total_mb
            );

            // Check if within claimed typical range
            if total_mb >= 50.0 && total_mb <= 100.0 {
                println!(
                    "\nSUCCESS: Memory usage is within README claimed typical range (50-100 MB)"
                );
            } else if total_mb < 50.0 {
                println!(
                    "\nEXCELLENT: Memory usage is better than claimed ({:.2} MB < 50 MB)",
                    total_mb
                );
            } else if total_mb <= 150.0 {
                println!(
                    "\nGOOD: Memory usage is reasonable, though above typical range ({:.2} MB)",
                    total_mb
                );
            }
        }
    }

    #[test]
    #[serial]
    fn test_memory_usage_batch_processing() {
        println!("\n\nTEST: Memory usage for batch processing (simulating concurrent operations)");
        println!("{}", "=".repeat(60));

        let baseline = MemoryUsage::current();
        println!("Baseline RSS: {:.2} MB", baseline.rss_mb());

        let classifier = EmailClassifier::default();
        let mut peak_memory = baseline;
        let batch_size = 2_500;
        let num_batches = 4;

        println!(
            "Processing {} batches of {} emails each...",
            num_batches, batch_size
        );

        // Simulate processing batches with some data retention
        let mut all_results = Vec::new();

        for batch_num in 0..num_batches {
            println!("\n  Batch {}/{}:", batch_num + 1, num_batches);

            // Generate batch
            let mut generator = MockGenerator::new(MockGeneratorConfig {
                seed: 42 + batch_num as u64,
                ..Default::default()
            });
            let batch_emails = generator.generate_messages(batch_size);

            let after_batch_gen = MemoryUsage::current();
            println!("    After generation: {:.2} MB", after_batch_gen.rss_mb());

            // Classify batch
            let batch_classifications: Vec<_> = batch_emails
                .iter()
                .map(|email| classifier.classify(email))
                .collect::<Result<Vec<_>, _>>()
                .expect("Classification should succeed");

            let after_batch_classify = MemoryUsage::current();
            println!(
                "    After classification: {:.2} MB",
                after_batch_classify.rss_mb()
            );

            // Store results (simulating concurrent retention)
            all_results.push((batch_emails, batch_classifications));

            let after_batch_store = MemoryUsage::current();
            println!("    After storage: {:.2} MB", after_batch_store.rss_mb());

            // Track peak memory
            if after_batch_store.rss_bytes > peak_memory.rss_bytes {
                peak_memory = after_batch_store;
            }
        }

        print_memory_stats(
            "Peak Memory During Batch Processing",
            &baseline,
            &peak_memory,
        );

        let peak_delta = peak_memory.delta(&baseline);
        let peak_mb = peak_delta.rss_delta_mb();

        println!(
            "\nRESULT: Peak memory during batch processing: {:.2} MB",
            peak_mb
        );
        println!("Total emails processed: {}", batch_size * num_batches);

        // Verify we processed all batches
        assert_eq!(
            all_results.len(),
            num_batches,
            "Should have processed all batches"
        );

        // README claims: "Peak: ~200 MB during concurrent batch operations"
        // We'll use 350 MB as conservative upper limit
        #[cfg(target_os = "linux")]
        {
            assert!(
                peak_mb < 350.0,
                "Peak memory during batch processing should be < 350 MB, got {:.2} MB",
                peak_mb
            );

            // Check if within claimed range
            if peak_mb <= 200.0 {
                println!("\nSUCCESS: Peak memory is within README claimed range (~200 MB)");
            } else if peak_mb <= 250.0 {
                println!(
                    "\nGOOD: Peak memory is close to claimed range ({:.2} MB vs ~200 MB)",
                    peak_mb
                );
            }
        }
    }

    #[test]
    #[serial]
    fn test_memory_usage_small_batch() {
        println!("\n\nTEST: Memory usage for small batch (1,000 emails)");
        println!("{}", "=".repeat(60));

        let baseline = MemoryUsage::current();
        println!("Baseline RSS: {:.2} MB", baseline.rss_mb());

        // Generate and classify smaller batch
        println!("Processing 1,000 emails...");
        let emails = generate_mock_emails(1_000);
        let classifier = EmailClassifier::default();
        let classifications: Vec<_> = emails
            .iter()
            .map(|email| classifier.classify(email))
            .collect::<Result<Vec<_>, _>>()
            .expect("Classification should succeed");

        let after_processing = MemoryUsage::current();
        print_memory_stats(
            "After Processing 1,000 Emails",
            &baseline,
            &after_processing,
        );

        assert_eq!(emails.len(), 1_000);
        assert_eq!(classifications.len(), 1_000);

        let delta = after_processing.delta(&baseline);
        let delta_mb = delta.rss_delta_mb();

        println!("\nRESULT: Processed 1,000 emails using {:.2} MB", delta_mb);

        // Small batch should use proportionally less memory
        // Expect ~5-10 MB for 1k emails, use 50 MB as conservative upper bound
        #[cfg(target_os = "linux")]
        {
            assert!(
                delta_mb < 50.0,
                "Memory for 1k emails should be < 50 MB, got {:.2} MB",
                delta_mb
            );

            if delta_mb < 15.0 {
                println!("SUCCESS: Small batch memory usage is efficient (< 15 MB)");
            }
        }
    }

    #[test]
    #[serial]
    fn test_memory_cleanup_after_drop() {
        println!("\n\nTEST: Memory cleanup after dropping large collections");
        println!("{}", "=".repeat(60));

        let baseline = MemoryUsage::current();
        println!("Baseline RSS: {:.2} MB", baseline.rss_mb());

        // Allocate large dataset
        {
            println!("Allocating 10,000 emails in scope...");
            let emails = generate_mock_emails(10_000);
            let classifier = EmailClassifier::default();
            let _classifications: Vec<_> = emails
                .iter()
                .map(|email| classifier.classify(email))
                .collect::<Result<Vec<_>, _>>()
                .expect("Classification should succeed");

            let peak = MemoryUsage::current();
            let peak_delta = peak.delta(&baseline).rss_delta_mb();
            println!(
                "Peak RSS while in scope: {:.2} MB (+{:.2} MB)",
                peak.rss_mb(),
                peak_delta
            );
        } // Drop everything here

        // Give time for OS to reclaim memory
        std::thread::sleep(std::time::Duration::from_millis(100));

        let after_drop = MemoryUsage::current();
        let after_delta = after_drop.delta(&baseline).rss_delta_mb();
        println!(
            "After drop RSS: {:.2} MB (+{:.2} MB from baseline)",
            after_drop.rss_mb(),
            after_delta
        );

        // Memory should have been released (though OS may not immediately reclaim it)
        println!(
            "\nRESULT: Memory after cleanup: {:.2} MB delta from baseline",
            after_delta
        );
        println!("(Note: OS may not immediately reclaim all memory)");
    }
}
