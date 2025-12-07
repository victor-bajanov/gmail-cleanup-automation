//! Mock email generator utilities for performance testing
//!
//! This module provides utilities to generate large numbers of realistic mock emails
//! for stress testing and performance benchmarking. It uses deterministic randomness
//! (seeded) to ensure reproducible test results.
//!
//! ## Memory Safety
//!
//! All generation functions are memory-aware and will automatically limit the number
//! of messages generated based on available system memory. This prevents out-of-memory
//! conditions in containerized test environments.
//!
//! Use `generate_mock_emails_checked()` to get an explicit error if memory limits
//! would be exceeded, or `generate_mock_emails()` which automatically applies limits.

use chrono::{DateTime, Duration, Utc};
use gmail_automation::models::{EmailCategory, MessageMetadata};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

use super::memory_limits::{self, MemoryLimitExceeded};

/// Distribution of email types for realistic test data
#[derive(Debug, Clone)]
pub struct EmailDistribution {
    pub newsletter: f32,
    pub receipt: f32,
    pub notification: f32,
    pub marketing: f32,
    pub financial: f32,
    pub shipping: f32,
    pub personal: f32,
}

impl Default for EmailDistribution {
    fn default() -> Self {
        Self {
            newsletter: 0.30,
            receipt: 0.20,
            notification: 0.20,
            marketing: 0.15,
            financial: 0.10,
            shipping: 0.03,
            personal: 0.02,
        }
    }
}

/// Configuration for mock email generation
#[derive(Debug, Clone)]
pub struct MockGeneratorConfig {
    pub seed: u64,
    pub distribution: EmailDistribution,
    pub start_date: DateTime<Utc>,
    pub end_date: DateTime<Utc>,
}

impl Default for MockGeneratorConfig {
    fn default() -> Self {
        let now = Utc::now();
        Self {
            seed: 42, // Default seed for reproducibility
            distribution: EmailDistribution::default(),
            start_date: now - Duration::days(90), // 90 days of history
            end_date: now,
        }
    }
}

/// Mock email generator for performance testing
pub struct MockGenerator {
    rng: StdRng,
    config: MockGeneratorConfig,
}

impl MockGenerator {
    /// Create a new mock generator with the given configuration
    pub fn new(config: MockGeneratorConfig) -> Self {
        let rng = StdRng::seed_from_u64(config.seed);
        Self { rng, config }
    }

    /// Create a new mock generator with default configuration
    pub fn new_with_seed(seed: u64) -> Self {
        let config = MockGeneratorConfig {
            seed,
            ..Default::default()
        };
        Self::new(config)
    }

    /// Generate N mock MessageMetadata structs with varied realistic data
    ///
    /// This function automatically applies memory limits based on available system memory.
    /// If the requested count exceeds safe limits, it will be reduced and a warning printed.
    ///
    /// For explicit control over memory limits, use `generate_messages_checked()`.
    pub fn generate_messages(&mut self, count: usize) -> Vec<MessageMetadata> {
        let (safe_count, was_limited) = memory_limits::apply_memory_limit(count);
        if was_limited {
            eprintln!(
                "MockGenerator: Reduced message count from {} to {} due to memory constraints",
                count, safe_count
            );
        }
        self.generate_messages_unchecked(safe_count)
    }

    /// Generate N mock MessageMetadata structs, failing if memory limits would be exceeded
    ///
    /// Unlike `generate_messages()`, this function returns an error instead of
    /// automatically reducing the count.
    pub fn generate_messages_checked(
        &mut self,
        count: usize,
    ) -> Result<Vec<MessageMetadata>, MemoryLimitExceeded> {
        memory_limits::check_allocation_safe(count)?;
        Ok(self.generate_messages_unchecked(count))
    }

    /// Generate messages without memory limit checks
    ///
    /// # Safety
    /// This function does not check memory limits. Use with caution.
    /// Prefer `generate_messages()` or `generate_messages_checked()` for safe allocation.
    fn generate_messages_unchecked(&mut self, count: usize) -> Vec<MessageMetadata> {
        // Use incremental allocation instead of with_capacity to avoid
        // pre-allocating potentially huge amounts of memory
        let mut messages = Vec::new();

        // Allocate in batches to avoid huge upfront allocation
        const BATCH_SIZE: usize = 10_000;
        let initial_capacity = count.min(BATCH_SIZE);
        messages.reserve(initial_capacity);

        for i in 0..count {
            // Reserve more capacity incrementally
            if i > 0 && i % BATCH_SIZE == 0 && i + BATCH_SIZE <= count {
                messages.reserve(BATCH_SIZE);
            }
            messages.push(self.generate_single_message(i));
        }

        messages
    }

    /// Generate a single mock message
    fn generate_single_message(&mut self, index: usize) -> MessageMetadata {
        let category = self.select_category();
        let sender_email = self.generate_sender(&category);
        let sender_domain = sender_email
            .split('@')
            .nth(1)
            .unwrap_or("example.com")
            .to_string();
        let subject = self.generate_subject(&category);
        let sender_name = self.generate_sender_name(&category, &sender_email);
        let date_received = self.generate_date();
        let (has_unsubscribe, is_automated) = self.generate_automated_flags(&category);
        let recipients = vec!["me@example.com".to_string()];
        let labels = vec!["INBOX".to_string()];

        MessageMetadata {
            id: format!("msg_{:010}", index),
            thread_id: format!("thread_{:010}", index),
            sender_email,
            sender_domain,
            sender_name,
            subject,
            recipients,
            date_received,
            labels,
            has_unsubscribe,
            is_automated,
        }
    }

    /// Select a category based on the configured distribution
    fn select_category(&mut self) -> EmailCategory {
        let roll: f32 = self.rng.gen();
        let dist = &self.config.distribution;

        let mut cumulative = 0.0;

        cumulative += dist.newsletter;
        if roll < cumulative {
            return EmailCategory::Newsletter;
        }

        cumulative += dist.receipt;
        if roll < cumulative {
            return EmailCategory::Receipt;
        }

        cumulative += dist.notification;
        if roll < cumulative {
            return EmailCategory::Notification;
        }

        cumulative += dist.marketing;
        if roll < cumulative {
            return EmailCategory::Marketing;
        }

        cumulative += dist.financial;
        if roll < cumulative {
            return EmailCategory::Financial;
        }

        cumulative += dist.shipping;
        if roll < cumulative {
            return EmailCategory::Shipping;
        }

        cumulative += dist.personal;
        if roll < cumulative {
            return EmailCategory::Personal;
        }

        EmailCategory::Other
    }

    /// Generate a realistic sender email based on category
    fn generate_sender(&mut self, category: &EmailCategory) -> String {
        let domain = self.select_random_domain();
        let prefix = self.select_sender_prefix(category);
        format!("{}@{}", prefix, domain)
    }

    /// Select a sender prefix based on category
    fn select_sender_prefix(&mut self, category: &EmailCategory) -> String {
        let prefixes: Vec<&str> = match category {
            EmailCategory::Newsletter => vec![
                "newsletter",
                "news",
                "updates",
                "digest",
                "weekly",
                "monthly",
                "bulletin",
                "noreply",
                "info",
            ],
            EmailCategory::Receipt => vec![
                "orders",
                "receipts",
                "noreply",
                "confirmation",
                "purchases",
                "order-confirmation",
                "transactions",
            ],
            EmailCategory::Notification => vec![
                "notifications",
                "notify",
                "alerts",
                "noreply",
                "updates",
                "system",
                "automated",
                "no-reply",
            ],
            EmailCategory::Marketing => vec![
                "marketing",
                "promo",
                "promotions",
                "deals",
                "offers",
                "sales",
                "special-offers",
                "noreply",
            ],
            EmailCategory::Financial => vec![
                "billing",
                "invoices",
                "finance",
                "accounts",
                "payments",
                "statements",
                "noreply",
            ],
            EmailCategory::Shipping => vec![
                "shipping",
                "delivery",
                "tracking",
                "logistics",
                "dispatch",
                "noreply",
            ],
            EmailCategory::Personal => vec![
                "john.doe",
                "jane.smith",
                "bob.wilson",
                "alice.jones",
                "charlie.brown",
                "david.lee",
                "emma.taylor",
                "frank.white",
            ],
            EmailCategory::Other => vec!["info", "contact", "support", "help", "service"],
        };

        let index = self.rng.gen_range(0..prefixes.len());
        prefixes[index].to_string()
    }

    /// Generate a realistic subject line based on category
    fn generate_subject(&mut self, category: &EmailCategory) -> String {
        let subjects: Vec<&str> = match category {
            EmailCategory::Newsletter => vec![
                "Weekly Newsletter - Tech Updates",
                "Your Daily Digest",
                "This Week in Tech",
                "Monthly Roundup - Top Stories",
                "Newsletter: Industry News",
                "The Daily Brief - Morning Edition",
                "Weekly Update from TechNews",
                "Your Weekly Summary",
                "Top Stories This Week",
                "Monthly Newsletter - January 2024",
            ],
            EmailCategory::Receipt => vec![
                "Your Receipt #12345",
                "Order Confirmation - Order #ABC123",
                "Purchase Receipt - Thank You!",
                "Your Order Has Been Confirmed",
                "Receipt for Your Recent Purchase",
                "Order Confirmation #789456",
                "Thank you for your purchase!",
                "Your Payment Receipt",
                "Transaction Confirmation",
                "Order Receipt - Delivered",
            ],
            EmailCategory::Notification => vec![
                "Security Alert: New Login",
                "Action Required: Verify Your Account",
                "Reminder: Update Your Profile",
                "Password Reset Requested",
                "New Comment on Your Post",
                "You have a new message",
                "Account Activity Notification",
                "Important: Verify Your Email",
                "Security Notification",
                "Update Required: Account Settings",
            ],
            EmailCategory::Marketing => vec![
                "50% Off Sale - Limited Time!",
                "Exclusive Offer Just For You",
                "Flash Sale: Save Big Today!",
                "Don't Miss Out - 30% Discount",
                "Special Promotion Inside",
                "Limited Time Offer - Act Now!",
                "Your Exclusive Coupon Code",
                "Weekend Sale - Up to 70% Off",
                "Save 25% on Your Next Order",
                "Black Friday Preview - Early Access",
            ],
            EmailCategory::Financial => vec![
                "Your Monthly Statement is Ready",
                "Invoice #INV-2024-001",
                "Payment Reminder - Due Soon",
                "Account Balance Update",
                "Your Credit Card Statement",
                "Invoice from Acme Corp",
                "Payment Received - Thank You",
                "Bank Statement - January 2024",
                "Account Activity Summary",
                "Payment Due: Invoice #12345",
            ],
            EmailCategory::Shipping => vec![
                "Your Package is Out for Delivery",
                "Shipping Confirmation - Tracking #123456",
                "Your Order Has Shipped!",
                "Delivery Update: Package Arriving Today",
                "Tracking Information for Your Order",
                "Package Delivered - Confirmation",
                "Your Shipment is On the Way",
                "Delivery Scheduled for Tomorrow",
                "FedEx Tracking Update",
                "UPS: Your Package Has Shipped",
            ],
            EmailCategory::Personal => vec![
                "Quick question",
                "Let's catch up soon",
                "Re: Meeting tomorrow",
                "Lunch next week?",
                "About the project",
                "Following up on our conversation",
                "Great meeting you!",
                "Thoughts on this?",
                "Can we chat?",
                "Question for you",
            ],
            EmailCategory::Other => vec![
                "Information Request",
                "General Inquiry",
                "Hello",
                "Question",
                "Update",
                "FYI",
                "For your information",
                "Misc",
                "Various",
                "General",
            ],
        };

        let index = self.rng.gen_range(0..subjects.len());
        subjects[index].to_string()
    }

    /// Generate a sender name based on category and email
    fn generate_sender_name(&mut self, category: &EmailCategory, email: &str) -> String {
        match category {
            EmailCategory::Personal => {
                let names = vec![
                    "John Doe",
                    "Jane Smith",
                    "Bob Wilson",
                    "Alice Jones",
                    "Charlie Brown",
                    "David Lee",
                    "Emma Taylor",
                    "Frank White",
                    "Grace Miller",
                    "Henry Davis",
                    "Ivy Martinez",
                    "Jack Anderson",
                ];
                let index = self.rng.gen_range(0..names.len());
                names[index].to_string()
            }
            _ => {
                // Extract domain and create company name
                let domain = email.split('@').nth(1).unwrap_or("Example");
                let company_name = domain.split('.').next().unwrap_or("Example");

                // Capitalize first letter
                let mut chars = company_name.chars();
                match chars.next() {
                    None => String::from("Company"),
                    Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                }
            }
        }
    }

    /// Generate a random date within the configured range
    fn generate_date(&mut self) -> DateTime<Utc> {
        let start = self.config.start_date.timestamp();
        let end = self.config.end_date.timestamp();
        let timestamp = self.rng.gen_range(start..=end);
        DateTime::from_timestamp(timestamp, 0).unwrap_or_else(Utc::now)
    }

    /// Generate automated flags based on category
    fn generate_automated_flags(&mut self, category: &EmailCategory) -> (bool, bool) {
        match category {
            EmailCategory::Personal => (false, false),
            EmailCategory::Newsletter => (true, true),
            EmailCategory::Marketing => (true, true),
            EmailCategory::Receipt => (false, true),
            EmailCategory::Notification => {
                // Some notifications have unsubscribe, some don't
                let has_unsubscribe = self.rng.gen_bool(0.6);
                (has_unsubscribe, true)
            }
            EmailCategory::Financial => (false, true),
            EmailCategory::Shipping => (false, true),
            EmailCategory::Other => {
                let is_automated = self.rng.gen_bool(0.5);
                let has_unsubscribe = is_automated && self.rng.gen_bool(0.3);
                (has_unsubscribe, is_automated)
            }
        }
    }

    /// Select a random domain from a pool of common domains
    fn select_random_domain(&mut self) -> String {
        let domains = COMMON_DOMAINS;
        let index = self.rng.gen_range(0..domains.len());
        domains[index].to_string()
    }
}

/// Pool of ~50 common domains for realistic email generation
const COMMON_DOMAINS: &[&str] = &[
    // E-commerce
    "amazon.com",
    "ebay.com",
    "etsy.com",
    "shopify.com",
    "walmart.com",
    "target.com",
    "bestbuy.com",
    // Social Media
    "facebook.com",
    "twitter.com",
    "linkedin.com",
    "instagram.com",
    "reddit.com",
    "pinterest.com",
    // Tech Companies
    "google.com",
    "microsoft.com",
    "apple.com",
    "github.com",
    "gitlab.com",
    "slack.com",
    "zoom.us",
    "dropbox.com",
    // Financial
    "paypal.com",
    "stripe.com",
    "square.com",
    "venmo.com",
    "chase.com",
    "wellsfargo.com",
    "bankofamerica.com",
    // Streaming/Media
    "netflix.com",
    "spotify.com",
    "youtube.com",
    "hulu.com",
    "disney.com",
    "twitch.tv",
    // Shipping
    "fedex.com",
    "ups.com",
    "usps.com",
    "dhl.com",
    // Email Services
    "gmail.com",
    "outlook.com",
    "yahoo.com",
    "protonmail.com",
    // SaaS/Services
    "salesforce.com",
    "atlassian.com",
    "zendesk.com",
    "hubspot.com",
    "mailchimp.com",
    "sendgrid.com",
    // News/Media
    "nytimes.com",
    "washingtonpost.com",
    "theguardian.com",
    "medium.com",
    "substack.com",
];

/// Generate mock emails with default settings
///
/// This function automatically applies memory limits based on available system memory.
/// If the requested count exceeds safe limits, it will be reduced.
pub fn generate_mock_emails(count: usize) -> Vec<MessageMetadata> {
    let mut generator = MockGenerator::new(MockGeneratorConfig::default());
    generator.generate_messages(count)
}

/// Generate mock emails with default settings, returning an error if memory limits would be exceeded
///
/// Unlike `generate_mock_emails()`, this function returns an error instead of
/// automatically reducing the count.
pub fn generate_mock_emails_checked(
    count: usize,
) -> Result<Vec<MessageMetadata>, MemoryLimitExceeded> {
    let mut generator = MockGenerator::new(MockGeneratorConfig::default());
    generator.generate_messages_checked(count)
}

/// Generate mock emails with a specific seed
///
/// This function automatically applies memory limits based on available system memory.
pub fn generate_mock_emails_with_seed(count: usize, seed: u64) -> Vec<MessageMetadata> {
    let mut generator = MockGenerator::new_with_seed(seed);
    generator.generate_messages(count)
}

/// Generate mock emails with a specific seed, returning an error if memory limits would be exceeded
pub fn generate_mock_emails_with_seed_checked(
    count: usize,
    seed: u64,
) -> Result<Vec<MessageMetadata>, MemoryLimitExceeded> {
    let mut generator = MockGenerator::new_with_seed(seed);
    generator.generate_messages_checked(count)
}

/// Generate mock emails with custom distribution
///
/// This function automatically applies memory limits based on available system memory.
pub fn generate_mock_emails_with_distribution(
    count: usize,
    seed: u64,
    distribution: EmailDistribution,
) -> Vec<MessageMetadata> {
    let config = MockGeneratorConfig {
        seed,
        distribution,
        ..Default::default()
    };
    let mut generator = MockGenerator::new(config);
    generator.generate_messages(count)
}

/// Get the maximum safe message count based on available memory
///
/// This is useful for tests that want to use the maximum available capacity
/// without risking out-of-memory conditions.
pub fn max_safe_message_count() -> usize {
    memory_limits::calculate_max_messages_default()
}

/// Print memory diagnostics for debugging
pub fn print_memory_diagnostics() {
    memory_limits::print_memory_diagnostics();
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    #[serial]
    fn test_generate_mock_emails() {
        let messages = generate_mock_emails(100);
        assert_eq!(messages.len(), 100);
    }

    #[test]
    #[serial]
    fn test_deterministic_generation() {
        let messages1 = generate_mock_emails_with_seed(50, 42);
        let messages2 = generate_mock_emails_with_seed(50, 42);

        // With the same seed, should generate identical emails
        assert_eq!(messages1.len(), messages2.len());
        for (m1, m2) in messages1.iter().zip(messages2.iter()) {
            assert_eq!(m1.id, m2.id);
            assert_eq!(m1.sender_email, m2.sender_email);
            assert_eq!(m1.subject, m2.subject);
            assert_eq!(m1.has_unsubscribe, m2.has_unsubscribe);
            assert_eq!(m1.is_automated, m2.is_automated);
        }
    }

    #[test]
    #[serial]
    fn test_different_seeds_produce_different_results() {
        let messages1 = generate_mock_emails_with_seed(50, 42);
        let messages2 = generate_mock_emails_with_seed(50, 99);

        // Different seeds should produce different results
        let mut different_count = 0;
        for (m1, m2) in messages1.iter().zip(messages2.iter()) {
            if m1.sender_email != m2.sender_email || m1.subject != m2.subject {
                different_count += 1;
            }
        }

        // At least 80% should be different
        assert!(different_count > 40);
    }

    #[test]
    #[serial]
    fn test_category_distribution() {
        let messages = generate_mock_emails_with_seed(1000, 42);

        // Count by sender patterns and automation flags which are more reliable indicators
        let automated_count = messages.iter().filter(|m| m.is_automated).count();
        let unsubscribe_count = messages.iter().filter(|m| m.has_unsubscribe).count();
        let personal_count = messages.iter().filter(|m| !m.is_automated).count();

        // Verify distribution ratios roughly match expectations
        // Personal should be ~2% (allow 0-5%)
        assert!(
            personal_count <= 50,
            "Personal emails should be ~2%, got {}/1000",
            personal_count
        );

        // Most should be automated (>95%)
        assert!(
            automated_count > 950,
            "Expected >950 automated emails, got {}",
            automated_count
        );

        // Many should have unsubscribe headers (newsletters, marketing ~45%)
        assert!(
            unsubscribe_count > 300,
            "Expected >300 with unsubscribe, got {}",
            unsubscribe_count
        );
        assert!(
            unsubscribe_count < 600,
            "Expected <600 with unsubscribe, got {}",
            unsubscribe_count
        );

        // Verify we have a good variety of subject lines
        let unique_subjects: std::collections::HashSet<_> =
            messages.iter().map(|m| &m.subject).collect();
        assert!(
            unique_subjects.len() > 50,
            "Should have variety in subjects, got {}",
            unique_subjects.len()
        );
    }

    #[test]
    #[serial]
    fn test_automated_flags() {
        let messages = generate_mock_emails_with_seed(100, 42);

        let automated_count = messages.iter().filter(|m| m.is_automated).count();
        let unsubscribe_count = messages.iter().filter(|m| m.has_unsubscribe).count();

        // Most messages should be automated (only ~2% are personal)
        assert!(automated_count > 90, "Expected >90 automated emails");

        // Many but not all automated emails have unsubscribe
        assert!(
            unsubscribe_count > 30,
            "Expected >30 emails with unsubscribe"
        );
        assert!(
            unsubscribe_count < automated_count,
            "Unsubscribe count should be less than automated count"
        );
    }

    #[test]
    #[serial]
    fn test_domain_variety() {
        let messages = generate_mock_emails_with_seed(200, 42);

        let mut domains = std::collections::HashSet::new();
        for msg in &messages {
            domains.insert(msg.sender_domain.clone());
        }

        // Should have good variety of domains (at least 20 different ones)
        assert!(
            domains.len() >= 20,
            "Expected at least 20 different domains, got {}",
            domains.len()
        );
    }

    #[test]
    #[serial]
    fn test_date_range() {
        let config = MockGeneratorConfig {
            seed: 42,
            start_date: Utc::now() - Duration::days(30),
            end_date: Utc::now(),
            ..Default::default()
        };

        let mut generator = MockGenerator::new(config.clone());
        let messages = generator.generate_messages(100);

        // All dates should be within the configured range
        for msg in &messages {
            assert!(msg.date_received >= config.start_date);
            assert!(msg.date_received <= config.end_date);
        }
    }

    #[test]
    #[serial]
    fn test_custom_distribution() {
        let distribution = EmailDistribution {
            newsletter: 0.50,   // 50% newsletters
            receipt: 0.30,      // 30% receipts
            notification: 0.10, // 10% notifications
            marketing: 0.05,    // 5% marketing
            financial: 0.03,    // 3% financial
            shipping: 0.01,     // 1% shipping
            personal: 0.01,     // 1% personal
        };

        let messages = generate_mock_emails_with_distribution(1000, 42, distribution);
        assert_eq!(messages.len(), 1000);

        // With custom distribution, most should have unsubscribe (newsletters 50% + marketing 5% = 55%)
        let unsubscribe_count = messages.iter().filter(|m| m.has_unsubscribe).count();

        // Should be roughly 55% with unsubscribe (allow variance)
        assert!(
            unsubscribe_count > 400,
            "Expected ~550 with unsubscribe, got {}",
            unsubscribe_count
        );
        assert!(
            unsubscribe_count < 700,
            "Expected ~550 with unsubscribe, got {}",
            unsubscribe_count
        );

        // Very few should be personal (~1%)
        let personal_count = messages.iter().filter(|m| !m.is_automated).count();
        assert!(
            personal_count < 30,
            "Expected ~10 personal, got {}",
            personal_count
        );
    }

    #[test]
    #[serial]
    fn test_message_id_format() {
        let messages = generate_mock_emails_with_seed(10, 42);

        for (i, msg) in messages.iter().enumerate() {
            assert_eq!(msg.id, format!("msg_{:010}", i));
            assert_eq!(msg.thread_id, format!("thread_{:010}", i));
        }
    }

    #[test]
    #[serial]
    fn test_sender_email_format() {
        let messages = generate_mock_emails_with_seed(100, 42);

        for msg in &messages {
            // Should have valid email format
            assert!(msg.sender_email.contains('@'));
            assert_eq!(msg.sender_email.split('@').count(), 2);

            // Domain should match extracted domain
            let email_domain = msg.sender_email.split('@').nth(1).unwrap();
            assert_eq!(msg.sender_domain, email_domain);
        }
    }

    #[test]
    #[serial]
    fn test_all_messages_have_recipients() {
        let messages = generate_mock_emails_with_seed(50, 42);

        for msg in &messages {
            assert!(!msg.recipients.is_empty());
            assert_eq!(msg.recipients[0], "me@example.com");
        }
    }

    #[test]
    #[serial]
    fn test_all_messages_have_inbox_label() {
        let messages = generate_mock_emails_with_seed(50, 42);

        for msg in &messages {
            assert!(msg.labels.contains(&"INBOX".to_string()));
        }
    }
}
