//! Performance testing utilities with mock email generators
//!
//! This module provides utilities for generating large volumes of realistic test emails
//! for performance benchmarking and load testing.
//!
//! ## Memory Safety
//!
//! All generation functions in this module are memory-aware and will automatically
//! limit allocations based on available system memory. This is critical for containerized
//! test environments with restricted memory.
//!
//! Use `memory_limits::print_memory_diagnostics()` to see current memory availability.

pub mod memory_limits;
pub mod mock_generator;

#[cfg(test)]
mod classification_benchmark;

#[cfg(test)]
mod memory_usage_test;

#[cfg(test)]
mod state_file_size_test;

use chrono::{DateTime, Duration, Utc};
use gmail_automation::models::MessageMetadata;
use rand::distributions::{Alphanumeric, DistString};
use rand::seq::SliceRandom;
use rand::Rng;

/// Pool of realistic email domains for test data generation
const DOMAINS: &[&str] = &[
    // Tech companies
    "github.com",
    "gitlab.com",
    "atlassian.com",
    "slack.com",
    "notion.so",
    "vercel.com",
    "netlify.com",
    "heroku.com",
    // E-commerce
    "amazon.com",
    "ebay.com",
    "etsy.com",
    "shopify.com",
    "aliexpress.com",
    "walmart.com",
    "target.com",
    "bestbuy.com",
    // Social media
    "facebook.com",
    "twitter.com",
    "linkedin.com",
    "instagram.com",
    "reddit.com",
    "medium.com",
    // Financial
    "paypal.com",
    "stripe.com",
    "square.com",
    "wise.com",
    "revolut.com",
    "bankofamerica.com",
    "chase.com",
    "wellsfargo.com",
    // Email services
    "mailchimp.com",
    "sendgrid.net",
    "mailgun.org",
    // Shipping
    "fedex.com",
    "ups.com",
    "usps.com",
    "dhl.com",
    // Newsletter/media
    "substack.com",
    "substackcdn.com",
    "nytimes.com",
    "wsj.com",
    "theguardian.com",
    "techcrunch.com",
    "wired.com",
    // Cloud providers
    "aws.amazon.com",
    "cloud.google.com",
    "azure.microsoft.com",
];

/// Subject patterns for different email categories
struct SubjectPatterns;

impl SubjectPatterns {
    fn newsletter() -> &'static [&'static str] {
        &[
            "Weekly Newsletter - {} Updates",
            "Your {} Weekly Digest",
            "This Week in {}: Top Stories",
            "{} Monthly Roundup",
            "Newsletter: {} Edition",
            "The {} Weekly Brief",
            "{} News Digest",
            "Your Weekly {} Update",
        ]
    }

    fn receipt() -> &'static [&'static str] {
        &[
            "Your Receipt for Order #{}",
            "Order Confirmation #{} - Thank You!",
            "Receipt: Your {} Purchase",
            "Payment Receipt #{}",
            "Transaction Confirmation #{}",
            "Your Order #{} is Confirmed",
            "Purchase Receipt from {}",
            "Invoice #{} - Payment Received",
        ]
    }

    fn shipping() -> &'static [&'static str] {
        &[
            "Your Order Has Shipped - Tracking #{}",
            "Package Dispatch Notification #{}",
            "Shipping Update: Out for Delivery",
            "Your {} Order is On the Way",
            "Delivery Scheduled for {}",
            "Tracking Update: Package #{}",
            "Your Shipment from {} Has Departed",
            "Delivery Confirmation Required",
        ]
    }

    fn marketing() -> &'static [&'static str] {
        &[
            "Exclusive 50% Off Sale - Limited Time!",
            "Special Offer Just For You",
            "Weekend Sale: Up to 70% Off",
            "Don't Miss Out - {} Flash Sale",
            "Your Exclusive Discount Code Inside",
            "Big Savings on {} - Today Only",
            "Limited Time Offer: Save ${}",
            "Hot Deals Alert - {} Edition",
        ]
    }

    fn notification() -> &'static [&'static str] {
        &[
            "Action Required: Verify Your Account",
            "Security Alert: New Login Detected",
            "Reminder: {} Update Available",
            "Important: Your Attention Needed",
            "Notification: {} Activity",
            "Alert: Password Change Requested",
            "Confirm Your {} Subscription",
            "Account Activity Notification",
        ]
    }

    fn financial() -> &'static [&'static str] {
        &[
            "Your {} Statement is Ready",
            "Monthly Account Statement - {}",
            "Payment Due: Invoice #{}",
            "Account Balance Notification",
            "Credit Card Statement for {}",
            "Invoice #{} from {}",
            "Wire Transfer Confirmation #{}",
            "Bank Statement - {} Available",
        ]
    }

    fn personal() -> &'static [&'static str] {
        &[
            "Re: {}",
            "Quick question about {}",
            "Following up on {}",
            "Thanks for {}",
            "Meeting notes: {}",
            "{} - What do you think?",
            "Can we discuss {}?",
            "FYI: {}",
        ]
    }
}

/// Sender prefix patterns for different categories
struct SenderPrefixes;

impl SenderPrefixes {
    fn newsletter() -> &'static [&'static str] {
        &[
            "newsletter",
            "news",
            "updates",
            "digest",
            "weekly",
            "info",
        ]
    }

    fn automated() -> &'static [&'static str] {
        &[
            "noreply",
            "no-reply",
            "donotreply",
            "notifications",
            "automated",
            "system",
        ]
    }

    fn financial() -> &'static [&'static str] {
        &["billing", "invoices", "accounts", "finance", "payments"]
    }

    fn shipping() -> &'static [&'static str] {
        &["shipping", "delivery", "tracking", "logistics"]
    }

    fn marketing() -> &'static [&'static str] {
        &["marketing", "promo", "promotions", "deals", "offers"]
    }

    fn personal() -> &'static [&'static str] {
        &[
            "john", "jane", "alex", "sam", "chris", "pat", "taylor", "morgan", "jordan", "casey",
        ]
    }
}

/// Topic words for generating varied subject lines
const TOPICS: &[&str] = &[
    "Tech",
    "Business",
    "Finance",
    "Health",
    "Travel",
    "Food",
    "Sports",
    "Entertainment",
    "Science",
    "Education",
    "Fashion",
    "Music",
    "Books",
    "Gaming",
    "News",
];

/// Generate a random email ID
fn generate_id(rng: &mut impl Rng) -> String {
    format!("msg_{}", Alphanumeric.sample_string(rng, 16))
}

/// Generate a random thread ID
fn generate_thread_id(rng: &mut impl Rng) -> String {
    format!("thread_{}", Alphanumeric.sample_string(rng, 16))
}

/// Generate a random date within the last N days
fn generate_date(rng: &mut impl Rng, days_back: i64) -> DateTime<Utc> {
    let days_offset = rng.gen_range(0..days_back);
    let hours_offset = rng.gen_range(0..24);
    let minutes_offset = rng.gen_range(0..60);

    Utc::now()
        - Duration::days(days_offset)
        - Duration::hours(hours_offset)
        - Duration::minutes(minutes_offset)
}

/// Generate a sender email for a specific category
fn generate_sender_email(
    rng: &mut impl Rng,
    domain: &str,
    category: EmailCategory,
) -> (String, String) {
    let prefix = match category {
        EmailCategory::Newsletter => SenderPrefixes::newsletter()
            .choose(rng)
            .unwrap()
            .to_string(),
        EmailCategory::Receipt | EmailCategory::Notification => SenderPrefixes::automated()
            .choose(rng)
            .unwrap()
            .to_string(),
        EmailCategory::Financial => SenderPrefixes::financial()
            .choose(rng)
            .unwrap()
            .to_string(),
        EmailCategory::Shipping => SenderPrefixes::shipping()
            .choose(rng)
            .unwrap()
            .to_string(),
        EmailCategory::Marketing => SenderPrefixes::marketing()
            .choose(rng)
            .unwrap()
            .to_string(),
        EmailCategory::Personal => SenderPrefixes::personal()
            .choose(rng)
            .unwrap()
            .to_string(),
        EmailCategory::Other => "contact".to_string(),
    };

    let email = format!("{}@{}", prefix, domain);
    let name = format!(
        "{} {}",
        prefix
            .chars()
            .next()
            .unwrap()
            .to_uppercase()
            .to_string()
            + &prefix[1..],
        domain.split('.').next().unwrap_or("Service")
    );

    (email, name)
}

/// Generate a subject line for a specific category
fn generate_subject(rng: &mut impl Rng, category: EmailCategory) -> String {
    let patterns = match category {
        EmailCategory::Newsletter => SubjectPatterns::newsletter(),
        EmailCategory::Receipt => SubjectPatterns::receipt(),
        EmailCategory::Shipping => SubjectPatterns::shipping(),
        EmailCategory::Marketing => SubjectPatterns::marketing(),
        EmailCategory::Notification => SubjectPatterns::notification(),
        EmailCategory::Financial => SubjectPatterns::financial(),
        EmailCategory::Personal => SubjectPatterns::personal(),
        EmailCategory::Other => &["Update from {}", "Message: {}"],
    };

    let pattern = patterns.choose(rng).unwrap();

    // Replace {} with contextual content
    if pattern.contains("{}") {
        let replacement = if category == EmailCategory::Receipt
            || category == EmailCategory::Shipping
            || category == EmailCategory::Financial
        {
            // Use random number for transactional emails
            format!("{:06}", rng.gen_range(100000..999999))
        } else {
            // Use topic for content emails
            TOPICS.choose(rng).unwrap().to_string()
        };
        pattern.replace("{}", &replacement)
    } else {
        pattern.to_string()
    }
}

/// Email category for generation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EmailCategory {
    Newsletter,
    Receipt,
    Shipping,
    Marketing,
    Notification,
    Financial,
    Personal,
    Other,
}

impl EmailCategory {
    fn all() -> &'static [EmailCategory] {
        &[
            EmailCategory::Newsletter,
            EmailCategory::Receipt,
            EmailCategory::Shipping,
            EmailCategory::Marketing,
            EmailCategory::Notification,
            EmailCategory::Financial,
            EmailCategory::Personal,
            EmailCategory::Other,
        ]
    }

    fn should_have_unsubscribe(&self) -> bool {
        matches!(
            self,
            EmailCategory::Newsletter | EmailCategory::Marketing | EmailCategory::Notification
        )
    }

    fn is_automated(&self) -> bool {
        !matches!(self, EmailCategory::Personal)
    }
}

/// Generate a single email message for a specific category
fn generate_message(
    rng: &mut impl Rng,
    domain: &str,
    category: EmailCategory,
    days_back: i64,
) -> MessageMetadata {
    let (sender_email, sender_name) = generate_sender_email(rng, domain, category);
    let subject = generate_subject(rng, category);

    MessageMetadata {
        id: generate_id(rng),
        thread_id: generate_thread_id(rng),
        sender_email: sender_email.clone(),
        sender_domain: domain.to_string(),
        sender_name,
        subject,
        recipients: vec!["test@example.com".to_string()],
        date_received: generate_date(rng, days_back),
        labels: vec!["INBOX".to_string()],
        has_unsubscribe: category.should_have_unsubscribe() && rng.gen_bool(0.8),
        is_automated: category.is_automated(),
    }
}

/// Generate random emails with diverse categories
///
/// This generates emails with a realistic distribution of categories:
/// - 25% Newsletters
/// - 20% Marketing
/// - 15% Notifications
/// - 15% Receipts
/// - 10% Shipping
/// - 10% Financial
/// - 5% Personal
/// - Remaining: Other
///
/// This function automatically applies memory limits based on available system memory.
/// If the requested count exceeds safe limits, it will be reduced.
pub fn generate_random_emails(count: usize) -> Vec<MessageMetadata> {
    let (safe_count, was_limited) = memory_limits::apply_memory_limit(count);
    if was_limited {
        eprintln!(
            "generate_random_emails: Reduced count from {} to {} due to memory constraints",
            count, safe_count
        );
    }

    let mut rng = rand::thread_rng();

    // Use incremental allocation instead of with_capacity
    let mut messages = Vec::new();
    const BATCH_SIZE: usize = 10_000;
    messages.reserve(safe_count.min(BATCH_SIZE));

    // Define category distribution weights
    let category_weights = vec![
        (EmailCategory::Newsletter, 25),
        (EmailCategory::Marketing, 20),
        (EmailCategory::Notification, 15),
        (EmailCategory::Receipt, 15),
        (EmailCategory::Shipping, 10),
        (EmailCategory::Financial, 10),
        (EmailCategory::Personal, 5),
    ];

    // Flatten weights into a selection array
    let mut category_pool: Vec<EmailCategory> = Vec::new();
    for (category, weight) in category_weights {
        category_pool.extend(std::iter::repeat(category).take(weight));
    }

    for i in 0..safe_count {
        // Reserve more capacity incrementally
        if i > 0 && i % BATCH_SIZE == 0 && i + BATCH_SIZE <= safe_count {
            messages.reserve(BATCH_SIZE);
        }
        let domain = DOMAINS.choose(&mut rng).unwrap();
        let category = category_pool.choose(&mut rng).unwrap_or(&EmailCategory::Other);
        messages.push(generate_message(&mut rng, domain, *category, 90));
    }

    messages
}

/// Generate bulk newsletter-style emails
///
/// All emails will be newsletters with unsubscribe headers,
/// using a variety of newsletter domains.
///
/// This function automatically applies memory limits based on available system memory.
pub fn generate_newsletter_emails(count: usize) -> Vec<MessageMetadata> {
    let (safe_count, was_limited) = memory_limits::apply_memory_limit(count);
    if was_limited {
        eprintln!(
            "generate_newsletter_emails: Reduced count from {} to {} due to memory constraints",
            count, safe_count
        );
    }

    let mut rng = rand::thread_rng();

    // Use incremental allocation
    let mut messages = Vec::new();
    const BATCH_SIZE: usize = 10_000;
    messages.reserve(safe_count.min(BATCH_SIZE));

    // Focus on newsletter-heavy domains
    let newsletter_domains = [
        "substack.com",
        "substackcdn.com",
        "mailchimp.com",
        "sendgrid.net",
        "medium.com",
        "nytimes.com",
        "techcrunch.com",
        "wired.com",
    ];

    for i in 0..safe_count {
        if i > 0 && i % BATCH_SIZE == 0 && i + BATCH_SIZE <= safe_count {
            messages.reserve(BATCH_SIZE);
        }
        let domain = newsletter_domains.choose(&mut rng).unwrap();
        messages.push(generate_message(
            &mut rng,
            domain,
            EmailCategory::Newsletter,
            90,
        ));
    }

    messages
}

/// Generate a realistic mixed workload of emails
///
/// This simulates a typical user's inbox with:
/// - Bursts of newsletters (morning/evening)
/// - Scattered receipts and shipping updates
/// - Regular notifications throughout the day
/// - Occasional personal emails
/// - Marketing campaigns in waves
///
/// This function automatically applies memory limits based on available system memory.
pub fn generate_mixed_workload(count: usize) -> Vec<MessageMetadata> {
    let (safe_count, was_limited) = memory_limits::apply_memory_limit(count);
    if was_limited {
        eprintln!(
            "generate_mixed_workload: Reduced count from {} to {} due to memory constraints",
            count, safe_count
        );
    }

    let mut rng = rand::thread_rng();

    // Use incremental allocation
    let mut messages = Vec::new();
    const BATCH_SIZE: usize = 10_000;
    messages.reserve(safe_count.min(BATCH_SIZE));

    // Split into chunks to simulate realistic patterns
    let newsletter_count = safe_count / 3; // Morning/evening newsletter bursts
    let transactional_count = safe_count / 5; // Receipts, shipping
    let notification_count = safe_count / 4; // Notifications spread throughout
    let marketing_count = safe_count / 6; // Marketing waves
    let personal_count = safe_count / 10; // Occasional personal
    let remaining = safe_count
        - newsletter_count
        - transactional_count
        - notification_count
        - marketing_count
        - personal_count;

    // Generate newsletters (older, clustered)
    for _ in 0..newsletter_count {
        reserve_if_needed(&mut messages, BATCH_SIZE);
        let domain = DOMAINS.choose(&mut rng).unwrap();
        messages.push(generate_message(
            &mut rng,
            domain,
            EmailCategory::Newsletter,
            7,
        ));
    }

    // Generate transactional emails (receipts + shipping)
    for i in 0..transactional_count {
        reserve_if_needed(&mut messages, BATCH_SIZE);
        let domain = DOMAINS.choose(&mut rng).unwrap();
        let category = if i % 2 == 0 {
            EmailCategory::Receipt
        } else {
            EmailCategory::Shipping
        };
        messages.push(generate_message(&mut rng, domain, category, 30));
    }

    // Generate notifications (spread out)
    for _ in 0..notification_count {
        reserve_if_needed(&mut messages, BATCH_SIZE);
        let domain = DOMAINS.choose(&mut rng).unwrap();
        messages.push(generate_message(
            &mut rng,
            domain,
            EmailCategory::Notification,
            14,
        ));
    }

    // Generate marketing (recent wave)
    for _ in 0..marketing_count {
        reserve_if_needed(&mut messages, BATCH_SIZE);
        let domain = DOMAINS.choose(&mut rng).unwrap();
        messages.push(generate_message(
            &mut rng,
            domain,
            EmailCategory::Marketing,
            3,
        ));
    }

    // Generate personal emails (very recent)
    for _ in 0..personal_count {
        reserve_if_needed(&mut messages, BATCH_SIZE);
        let domain = DOMAINS.choose(&mut rng).unwrap();
        messages.push(generate_message(
            &mut rng,
            domain,
            EmailCategory::Personal,
            1,
        ));
    }

    // Fill remaining with financial and other
    for i in 0..remaining {
        reserve_if_needed(&mut messages, BATCH_SIZE);
        let domain = DOMAINS.choose(&mut rng).unwrap();
        let category = if i % 2 == 0 {
            EmailCategory::Financial
        } else {
            EmailCategory::Other
        };
        messages.push(generate_message(&mut rng, domain, category, 60));
    }

    // Shuffle to simulate realistic arrival order
    messages.shuffle(&mut rng);

    messages
}

/// Helper function to reserve capacity incrementally
fn reserve_if_needed<T>(vec: &mut Vec<T>, batch_size: usize) {
    if vec.len() == vec.capacity() {
        vec.reserve(batch_size);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_random_emails() {
        let emails = generate_random_emails(100);
        assert_eq!(emails.len(), 100);

        // Verify all have required fields
        for email in &emails {
            assert!(!email.id.is_empty());
            assert!(!email.thread_id.is_empty());
            assert!(!email.sender_email.is_empty());
            assert!(!email.sender_domain.is_empty());
            assert!(!email.subject.is_empty());
            assert!(email.sender_email.contains('@'));
        }

        // Check for variety in domains
        let unique_domains: std::collections::HashSet<_> =
            emails.iter().map(|e| &e.sender_domain).collect();
        assert!(
            unique_domains.len() > 5,
            "Should have variety in domains"
        );

        // Check for variety in subjects
        let unique_subjects: std::collections::HashSet<_> =
            emails.iter().map(|e| &e.subject).collect();
        assert!(
            unique_subjects.len() > 50,
            "Should have variety in subjects"
        );
    }

    #[test]
    fn test_generate_newsletter_emails() {
        let emails = generate_newsletter_emails(50);
        assert_eq!(emails.len(), 50);

        // Most should have unsubscribe headers
        let with_unsubscribe = emails.iter().filter(|e| e.has_unsubscribe).count();
        assert!(
            with_unsubscribe > 40,
            "Most newsletters should have unsubscribe"
        );

        // All should be automated
        for email in &emails {
            assert!(email.is_automated, "Newsletters should be automated");
        }
    }

    #[test]
    fn test_generate_mixed_workload() {
        let emails = generate_mixed_workload(100);
        assert_eq!(emails.len(), 100);

        // Check for variety in automation flags
        let automated_count = emails.iter().filter(|e| e.is_automated).count();
        let personal_count = emails.iter().filter(|e| !e.is_automated).count();

        assert!(automated_count > 0, "Should have automated emails");
        assert!(personal_count > 0, "Should have personal emails");

        // Check date distribution (should be spread across time)
        let dates: Vec<_> = emails.iter().map(|e| e.date_received).collect();
        let min_date = dates.iter().min().unwrap();
        let max_date = dates.iter().max().unwrap();
        let date_range = max_date.signed_duration_since(*min_date);

        assert!(
            date_range.num_days() > 0,
            "Emails should be spread across multiple days"
        );
    }

    #[test]
    fn test_domain_variety() {
        let emails = generate_random_emails(1000);

        let domain_counts: std::collections::HashMap<_, usize> =
            emails.iter().fold(std::collections::HashMap::new(), |mut acc, e| {
                *acc.entry(&e.sender_domain).or_insert(0) += 1;
                acc
            });

        // Should use multiple domains
        assert!(domain_counts.len() >= 20, "Should use many different domains");

        // No single domain should dominate excessively
        for (_, count) in domain_counts {
            assert!(
                count < 500,
                "No domain should have more than half the emails"
            );
        }
    }

    #[test]
    fn test_email_metadata_validity() {
        let emails = generate_random_emails(10);

        for email in emails {
            // Verify email format
            assert!(email.sender_email.contains('@'));
            assert_eq!(
                email.sender_email.split('@').nth(1).unwrap(),
                email.sender_domain
            );

            // Verify IDs are unique-looking
            assert!(email.id.starts_with("msg_"));
            assert!(email.thread_id.starts_with("thread_"));

            // Verify dates are in the past
            assert!(email.date_received <= Utc::now());

            // Verify labels exist
            assert!(!email.labels.is_empty());
        }
    }
}
