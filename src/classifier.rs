//! Email classification engine with rule-based pattern matching

use crate::error::Result;
use crate::models::{Classification, EmailCategory, MessageMetadata};
use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::HashMap;

/// Automated email patterns (lines 1388-1397)
static AUTOMATED_PATTERNS: Lazy<HashMap<&'static str, Vec<&'static str>>> = Lazy::new(|| {
    let mut map = HashMap::new();
    map.insert("noreply", vec!["noreply@", "no-reply@", "donotreply@", "do-not-reply@"]);
    map.insert("notifications", vec!["notifications@", "notify@", "alerts@"]);
    map.insert("marketing", vec!["marketing@", "promo@", "promotions@", "deals@"]);
    map.insert("newsletter", vec!["newsletter@", "news@", "updates@"]);
    map.insert("automated", vec!["automated@", "auto@", "bot@", "system@"]);
    map.insert("info", vec!["info@", "contact@", "support@", "help@"]);
    map
});

/// Commercial domains list (lines 1399-1406)
static COMMERCIAL_DOMAINS: Lazy<Vec<&'static str>> = Lazy::new(|| {
    vec![
        "amazonses.com",
        "mailchimp.com",
        "sendgrid.net",
        "mailgun.org",
        "sparkpostmail.com",
        "mandrillapp.com",
        "postmarkapp.com",
        "email.com",
    ]
});

/// Subject pattern regexes using once_cell (lines 1415-1451)
static SUBJECT_PATTERNS: Lazy<SubjectPatterns> = Lazy::new(|| SubjectPatterns {
    receipt: Regex::new(
        r"(?i)(receipt|invoice|order|purchase|payment|transaction|confirmation|bill)"
    )
    .unwrap(),

    shipping: Regex::new(
        r"(?i)(ship|deliver|tracking|dispatch|out for delivery|package|parcel|fedex|ups|usps|dhl)"
    )
    .unwrap(),

    newsletter: Regex::new(
        r"(?i)(newsletter|digest|weekly|monthly|roundup|bulletin|update)"
    )
    .unwrap(),

    marketing: Regex::new(
        r"(?i)(sale|discount|offer|deal|promo|coupon|limited time|exclusive|save|% off)"
    )
    .unwrap(),

    notification: Regex::new(
        r"(?i)(notification|alert|reminder|verify|confirm|action required|security)"
    )
    .unwrap(),

    financial: Regex::new(
        r"(?i)(statement|balance|credit card|bank|account|payment due|funds|wire|transfer)"
    )
    .unwrap(),

    automated: Regex::new(
        r"(?i)(automated|automatic|do not reply|this is an automated|system generated)"
    )
    .unwrap(),

    unsubscribe: Regex::new(
        r"(?i)(unsubscribe|opt.?out|manage.?preferences|update.?subscription)"
    )
    .unwrap(),
});

struct SubjectPatterns {
    receipt: Regex,
    shipping: Regex,
    newsletter: Regex,
    marketing: Regex,
    notification: Regex,
    financial: Regex,
    automated: Regex,
    unsubscribe: Regex,
}

/// Service information for known services (lines 1469-1498)
#[derive(Debug, Clone)]
struct ServiceInfo {
    name: String,
    category: EmailCategory,
    priority: i32,
}

static KNOWN_SERVICES: Lazy<HashMap<&'static str, ServiceInfo>> = Lazy::new(|| {
    let mut map = HashMap::new();

    // E-commerce
    map.insert("amazon.com", ServiceInfo {
        name: "Amazon".to_string(),
        category: EmailCategory::Receipt,
        priority: 70,
    });
    map.insert("ebay.com", ServiceInfo {
        name: "eBay".to_string(),
        category: EmailCategory::Receipt,
        priority: 70,
    });

    // Social media
    map.insert("facebook.com", ServiceInfo {
        name: "Facebook".to_string(),
        category: EmailCategory::Notification,
        priority: 40,
    });
    map.insert("twitter.com", ServiceInfo {
        name: "Twitter".to_string(),
        category: EmailCategory::Notification,
        priority: 40,
    });
    map.insert("linkedin.com", ServiceInfo {
        name: "LinkedIn".to_string(),
        category: EmailCategory::Notification,
        priority: 50,
    });

    // Financial
    map.insert("paypal.com", ServiceInfo {
        name: "PayPal".to_string(),
        category: EmailCategory::Financial,
        priority: 90,
    });
    map.insert("stripe.com", ServiceInfo {
        name: "Stripe".to_string(),
        category: EmailCategory::Financial,
        priority: 90,
    });

    // Tech services
    map.insert("github.com", ServiceInfo {
        name: "GitHub".to_string(),
        category: EmailCategory::Notification,
        priority: 60,
    });
    map.insert("gitlab.com", ServiceInfo {
        name: "GitLab".to_string(),
        category: EmailCategory::Notification,
        priority: 60,
    });

    map
});

pub struct EmailClassifier {
    // Configuration can be added here
}

impl EmailClassifier {
    pub fn new() -> Self {
        Self {}
    }

    /// Classify an email using rule-based logic
    pub fn classify(&self, message: &MessageMetadata) -> Result<Classification> {
        // Determine if automated
        let is_automated = self.is_automated_sender(message);

        // Detect category
        let category = self.detect_category(message);

        // Calculate priority score (lines 1504-1566)
        let priority_score = self.calculate_priority_score(message, &category);

        // Generate suggested label
        let suggested_label = self.generate_label(message, &category);

        // Determine if should archive
        let should_archive = self.should_auto_archive(message, &category, priority_score);

        // Calculate confidence based on multiple factors
        let confidence = self.calculate_confidence(message, &category, is_automated);

        // Generate reasoning
        let reasoning = self.generate_reasoning(message, &category, is_automated, priority_score);

        Ok(Classification {
            message_id: message.id.clone(),
            category,
            confidence,
            suggested_label,
            should_archive,
            reasoning: Some(reasoning),
        })
    }

    /// Check if sender appears to be automated
    pub fn is_automated_sender(&self, message: &MessageMetadata) -> bool {
        let email = message.sender_email.to_lowercase();

        // Check for automated patterns
        for (_, patterns) in AUTOMATED_PATTERNS.iter() {
            for pattern in patterns {
                if email.starts_with(pattern) {
                    return true;
                }
            }
        }

        // Check if has unsubscribe header
        if message.has_unsubscribe {
            return true;
        }

        // Check subject for automated patterns
        if SUBJECT_PATTERNS.automated.is_match(&message.subject) {
            return true;
        }

        // Check for commercial email service domains
        for domain in COMMERCIAL_DOMAINS.iter() {
            if message.sender_domain.ends_with(domain) {
                return true;
            }
        }

        false
    }

    /// Detect category from subject and sender
    pub fn detect_category(&self, message: &MessageMetadata) -> EmailCategory {
        let subject_lower = message.subject.to_lowercase();

        // Check known services first
        if let Some(service_info) = KNOWN_SERVICES.get(message.sender_domain.as_str()) {
            return service_info.category.clone();
        }

        // Check for financial sender domains before subject patterns
        // Invoices from billing/finance addresses should be Financial, not Receipt
        let sender_email_lower = message.sender_email.to_lowercase();
        if sender_email_lower.starts_with("billing@")
            || sender_email_lower.starts_with("finance@")
            || sender_email_lower.starts_with("invoices@")
            || sender_email_lower.starts_with("accounts@") {
            if SUBJECT_PATTERNS.financial.is_match(&subject_lower)
                || subject_lower.contains("invoice")
                || subject_lower.contains("statement")
                || subject_lower.contains("bill") {
                return EmailCategory::Financial;
            }
        }

        // Pattern matching on subject
        if SUBJECT_PATTERNS.receipt.is_match(&subject_lower) {
            return EmailCategory::Receipt;
        }

        if SUBJECT_PATTERNS.shipping.is_match(&subject_lower) {
            return EmailCategory::Shipping;
        }

        if SUBJECT_PATTERNS.financial.is_match(&subject_lower) {
            return EmailCategory::Financial;
        }

        if SUBJECT_PATTERNS.newsletter.is_match(&subject_lower) {
            return EmailCategory::Newsletter;
        }

        if SUBJECT_PATTERNS.marketing.is_match(&subject_lower) {
            return EmailCategory::Marketing;
        }

        if SUBJECT_PATTERNS.notification.is_match(&subject_lower) {
            return EmailCategory::Notification;
        }

        // Check sender patterns
        for (category_name, patterns) in AUTOMATED_PATTERNS.iter() {
            for pattern in patterns {
                if message.sender_email.starts_with(pattern) {
                    return match *category_name {
                        "marketing" => EmailCategory::Marketing,
                        "newsletter" => EmailCategory::Newsletter,
                        "notifications" => EmailCategory::Notification,
                        _ => EmailCategory::Other,
                    };
                }
            }
        }

        // If not automated, likely personal
        if !self.is_automated_sender(message) {
            return EmailCategory::Personal;
        }

        EmailCategory::Other
    }

    /// Calculate priority score (lines 1504-1566)
    fn calculate_priority_score(&self, message: &MessageMetadata, category: &EmailCategory) -> i32 {
        let mut score = 50; // Base score

        // Category-based scoring
        match category {
            EmailCategory::Financial => score += 40,
            EmailCategory::Receipt => score += 30,
            EmailCategory::Personal => score += 30,
            EmailCategory::Shipping => score += 20,
            EmailCategory::Notification => score += 10,
            EmailCategory::Newsletter => score -= 10,
            EmailCategory::Marketing => score -= 20,
            EmailCategory::Other => score += 0,
        }

        // Known service bonus
        if let Some(service_info) = KNOWN_SERVICES.get(message.sender_domain.as_str()) {
            score = score.max(service_info.priority);
        }

        // Subject-based adjustments
        let subject_lower = message.subject.to_lowercase();

        if subject_lower.contains("urgent") || subject_lower.contains("important") {
            score += 20;
        }

        if subject_lower.contains("action required") || subject_lower.contains("verify") {
            score += 15;
        }

        if subject_lower.contains("password") || subject_lower.contains("security") {
            score += 25;
        }

        if subject_lower.contains("invoice") || subject_lower.contains("payment") {
            score += 20;
        }

        // Sender-based adjustments
        if message.sender_email.contains("billing") || message.sender_email.contains("finance") {
            score += 15;
        }

        // Automated emails get lower priority
        if self.is_automated_sender(message) {
            score -= 10;
        }

        // Has unsubscribe = lower priority
        if message.has_unsubscribe {
            score -= 15;
        }

        // Marketing indicators reduce priority
        if SUBJECT_PATTERNS.marketing.is_match(&subject_lower) {
            score -= 20;
        }

        if SUBJECT_PATTERNS.unsubscribe.is_match(&subject_lower) {
            score -= 10;
        }

        // Clamp score between 0 and 100
        score.clamp(0, 100)
    }

    /// Generate label suggestion based on domain clustering
    fn generate_label(&self, message: &MessageMetadata, category: &EmailCategory) -> String {
        // Check for known services first
        if let Some(service_info) = KNOWN_SERVICES.get(message.sender_domain.as_str()) {
            return format!("auto/{}", service_info.name);
        }

        // Domain clustering logic
        let domain = &message.sender_domain;

        // Extract main domain (remove subdomains)
        let main_domain = extract_main_domain(domain);

        // Generate label based on category and domain
        let category_prefix = match category {
            EmailCategory::Newsletter => "newsletters",
            EmailCategory::Receipt => "receipts",
            EmailCategory::Notification => "notifications",
            EmailCategory::Marketing => "marketing",
            EmailCategory::Shipping => "shipping",
            EmailCategory::Financial => "financial",
            EmailCategory::Personal => "personal",
            EmailCategory::Other => "other",
        };

        // Create hierarchical label
        if main_domain.is_empty() {
            format!("auto/{}", category_prefix)
        } else {
            let domain_label = sanitize_label_name(&main_domain);
            format!("auto/{}/{}", category_prefix, domain_label)
        }
    }

    /// Determine if message should be auto-archived
    fn should_auto_archive(&self, message: &MessageMetadata, category: &EmailCategory, priority: i32) -> bool {
        // Never archive high-priority messages
        if priority >= 70 {
            return false;
        }

        // Never archive personal emails
        if matches!(category, EmailCategory::Personal) {
            return false;
        }

        // Never archive financial emails
        if matches!(category, EmailCategory::Financial) {
            return false;
        }

        // Auto-archive marketing and newsletters with low priority
        if matches!(category, EmailCategory::Marketing | EmailCategory::Newsletter) && priority < 40 {
            return true;
        }

        // Auto-archive notifications with very low priority
        if matches!(category, EmailCategory::Notification) && priority < 30 {
            return true;
        }

        false
    }

    /// Calculate confidence score
    fn calculate_confidence(&self, message: &MessageMetadata, category: &EmailCategory, is_automated: bool) -> f32 {
        let mut confidence: f32 = 0.5;

        // Known service = high confidence
        if KNOWN_SERVICES.contains_key(message.sender_domain.as_str()) {
            confidence += 0.3;
        }

        // Strong subject pattern match
        let subject_lower = message.subject.to_lowercase();
        let pattern_matches = [
            SUBJECT_PATTERNS.receipt.is_match(&subject_lower),
            SUBJECT_PATTERNS.shipping.is_match(&subject_lower),
            SUBJECT_PATTERNS.financial.is_match(&subject_lower),
            SUBJECT_PATTERNS.newsletter.is_match(&subject_lower),
            SUBJECT_PATTERNS.marketing.is_match(&subject_lower),
        ].iter().filter(|&&x| x).count();

        if pattern_matches > 0 {
            confidence += 0.2;
        }

        // Automated sender pattern
        if is_automated {
            confidence += 0.15;
        }

        // Has unsubscribe header
        if message.has_unsubscribe {
            confidence += 0.1;
        }

        // Clamp between 0.0 and 1.0
        confidence.clamp(0.0, 1.0)
    }

    /// Generate reasoning for classification
    fn generate_reasoning(&self, message: &MessageMetadata, category: &EmailCategory, is_automated: bool, priority: i32) -> String {
        let mut reasons = Vec::new();

        // Category reasoning
        reasons.push(format!("Categorized as {:?}", category));

        // Known service
        if let Some(service_info) = KNOWN_SERVICES.get(message.sender_domain.as_str()) {
            reasons.push(format!("Recognized service: {}", service_info.name));
        }

        // Automated sender
        if is_automated {
            reasons.push("Detected as automated sender".to_string());
        }

        // Subject patterns
        let subject_lower = message.subject.to_lowercase();
        if SUBJECT_PATTERNS.receipt.is_match(&subject_lower) {
            reasons.push("Subject matches receipt pattern".to_string());
        }
        if SUBJECT_PATTERNS.marketing.is_match(&subject_lower) {
            reasons.push("Subject matches marketing pattern".to_string());
        }
        if SUBJECT_PATTERNS.financial.is_match(&subject_lower) {
            reasons.push("Subject matches financial pattern".to_string());
        }

        // Priority
        reasons.push(format!("Priority score: {}/100", priority));

        reasons.join(". ")
    }

    /// Cluster messages by domain for bulk classification
    pub fn cluster_by_domain(&self, messages: &[MessageMetadata]) -> HashMap<String, Vec<String>> {
        let mut clusters: HashMap<String, Vec<String>> = HashMap::new();

        for message in messages {
            let main_domain = extract_main_domain(&message.sender_domain);
            clusters
                .entry(main_domain)
                .or_insert_with(Vec::new)
                .push(message.id.clone());
        }

        clusters
    }

    /// Get domain statistics for analysis
    pub fn analyze_domain_patterns(&self, messages: &[MessageMetadata]) -> Vec<DomainStats> {
        let clusters = self.cluster_by_domain(messages);

        let mut stats: Vec<DomainStats> = clusters
            .into_iter()
            .map(|(domain, message_ids)| {
                let sample_messages: Vec<&MessageMetadata> = messages
                    .iter()
                    .filter(|m| extract_main_domain(&m.sender_domain) == domain)
                    .take(10)
                    .collect();

                let category = if let Some(msg) = sample_messages.first() {
                    self.detect_category(msg)
                } else {
                    EmailCategory::Other
                };

                let automated_count = sample_messages
                    .iter()
                    .filter(|m| self.is_automated_sender(m))
                    .count();

                DomainStats {
                    domain: domain.clone(),
                    count: message_ids.len(),
                    suggested_category: category,
                    automation_ratio: automated_count as f32 / sample_messages.len() as f32,
                }
            })
            .collect();

        // Sort by count descending
        stats.sort_by(|a, b| b.count.cmp(&a.count));

        stats
    }
}

impl Default for EmailClassifier {
    fn default() -> Self {
        Self::new()
    }
}

/// Domain statistics for analysis
#[derive(Debug, Clone)]
pub struct DomainStats {
    pub domain: String,
    pub count: usize,
    pub suggested_category: EmailCategory,
    pub automation_ratio: f32,
}

/// Known compound TLD suffixes (second-level domains that are part of the TLD)
static COMPOUND_TLD_SUFFIXES: &[&str] = &[
    // Generic compound TLDs
    "com", "co", "org", "net", "edu", "gov", "ac", "mil",
    // Country-specific common patterns
    "or", "ne", "go", "gob", "nic",
];

/// Known compound TLDs (full patterns)
static COMPOUND_TLDS: &[&str] = &[
    // Australia
    "com.au", "net.au", "org.au", "edu.au", "gov.au", "asn.au", "id.au",
    // United Kingdom
    "co.uk", "org.uk", "me.uk", "net.uk", "ac.uk", "gov.uk", "ltd.uk", "plc.uk",
    // New Zealand
    "co.nz", "net.nz", "org.nz", "govt.nz", "ac.nz",
    // Japan
    "co.jp", "or.jp", "ne.jp", "ac.jp", "go.jp",
    // Korea
    "co.kr", "or.kr", "ne.kr", "go.kr", "ac.kr",
    // Brazil
    "com.br", "net.br", "org.br", "gov.br", "edu.br",
    // India
    "co.in", "net.in", "org.in", "gov.in", "ac.in",
    // South Africa
    "co.za", "org.za", "net.za", "gov.za", "ac.za",
    // Germany (rare but exist)
    "com.de",
    // France
    "com.fr",
    // Spain
    "com.es", "org.es", "nom.es",
    // Italy
    "com.it",
    // Mexico
    "com.mx", "org.mx", "gob.mx", "net.mx",
    // China
    "com.cn", "net.cn", "org.cn", "gov.cn", "ac.cn",
    // Hong Kong
    "com.hk", "org.hk", "net.hk", "gov.hk", "edu.hk",
    // Singapore
    "com.sg", "org.sg", "net.sg", "gov.sg", "edu.sg",
    // Taiwan
    "com.tw", "org.tw", "net.tw", "gov.tw", "edu.tw",
    // Indonesia
    "co.id", "or.id", "go.id", "ac.id",
    // Malaysia
    "com.my", "org.my", "net.my", "gov.my", "edu.my",
    // Thailand
    "co.th", "or.th", "go.th", "ac.th",
    // Philippines
    "com.ph", "org.ph", "net.ph", "gov.ph", "edu.ph",
    // Vietnam
    "com.vn", "net.vn", "org.vn", "gov.vn", "edu.vn",
    // Russia
    "com.ru", "org.ru", "net.ru",
    // Turkey
    "com.tr", "org.tr", "net.tr", "gov.tr", "edu.tr",
    // Argentina
    "com.ar", "org.ar", "net.ar", "gov.ar", "edu.ar",
    // Colombia
    "com.co", "org.co", "net.co", "gov.co", "edu.co",
    // Chile
    "com.cl",
    // Peru
    "com.pe", "org.pe", "net.pe", "gob.pe", "edu.pe",
    // Other common patterns
    "co.il", "org.il", "ac.il", // Israel
    "co.at", // Austria
];

/// Extract main domain from full domain (remove subdomains, handle compound TLDs)
fn extract_main_domain(domain: &str) -> String {
    let parts: Vec<&str> = domain.split('.').collect();

    if parts.len() < 2 {
        return domain.to_string();
    }

    // Check if this domain has a compound TLD
    let tld_parts_count = if parts.len() >= 3 {
        // Check against known compound TLDs
        let last_two = format!("{}.{}", parts[parts.len() - 2], parts[parts.len() - 1]);
        if COMPOUND_TLDS.contains(&last_two.as_str()) {
            3 // Need 3 parts: org.compound.tld
        } else if COMPOUND_TLD_SUFFIXES.contains(&parts[parts.len() - 2]) {
            // Fallback: check if second-to-last looks like a TLD category
            3
        } else {
            2 // Standard TLD
        }
    } else {
        2 // Only 2 parts, use both
    };

    if parts.len() >= tld_parts_count {
        parts[parts.len() - tld_parts_count..]
            .join(".")
    } else {
        domain.to_string()
    }
}

/// Sanitize domain name for use in label
fn sanitize_label_name(domain: &str) -> String {
    domain
        .replace('.', "-")
        .replace('@', "-at-")
        .replace('_', "-")
        .to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn create_test_message(sender_email: &str, subject: &str) -> MessageMetadata {
        let sender_domain = sender_email.split('@').nth(1).unwrap_or("example.com");

        MessageMetadata {
            id: "test-id".to_string(),
            thread_id: "thread-id".to_string(),
            sender_email: sender_email.to_string(),
            sender_domain: sender_domain.to_string(),
            sender_name: "Test Sender".to_string(),
            subject: subject.to_string(),
            recipients: vec!["user@example.com".to_string()],
            date_received: Utc::now(),
            labels: vec![],
            has_unsubscribe: false,
            is_automated: false,
        }
    }

    #[test]
    fn test_automated_sender_detection() {
        let classifier = EmailClassifier::new();

        let msg1 = create_test_message("noreply@example.com", "Test");
        assert!(classifier.is_automated_sender(&msg1));

        let msg2 = create_test_message("john@example.com", "Test");
        assert!(!classifier.is_automated_sender(&msg2));

        let msg3 = create_test_message("marketing@example.com", "Test");
        assert!(classifier.is_automated_sender(&msg3));
    }

    #[test]
    fn test_category_detection() {
        let classifier = EmailClassifier::new();

        let receipt = create_test_message("orders@amazon.com", "Your Amazon Order Receipt");
        assert_eq!(classifier.detect_category(&receipt), EmailCategory::Receipt);

        let marketing = create_test_message("deals@store.com", "50% Off Sale Today!");
        assert_eq!(classifier.detect_category(&marketing), EmailCategory::Marketing);

        let financial = create_test_message("billing@service.com", "Invoice #12345");
        assert_eq!(classifier.detect_category(&financial), EmailCategory::Financial);
    }

    #[test]
    fn test_priority_score() {
        let classifier = EmailClassifier::new();

        let financial = create_test_message("billing@bank.com", "Important: Payment Due");
        let score = classifier.calculate_priority_score(&financial, &EmailCategory::Financial);
        assert!(score > 70);

        let marketing = create_test_message("marketing@store.com", "Check out our deals");
        let score = classifier.calculate_priority_score(&marketing, &EmailCategory::Marketing);
        assert!(score < 50);
    }

    #[test]
    fn test_extract_main_domain() {
        // Standard TLDs
        assert_eq!(extract_main_domain("mail.google.com"), "google.com");
        assert_eq!(extract_main_domain("example.com"), "example.com");
        assert_eq!(extract_main_domain("sub.domain.example.com"), "example.com");

        // Compound TLDs - should include the org name, not just the TLD
        assert_eq!(extract_main_domain("amazon.com.au"), "amazon.com.au");
        assert_eq!(extract_main_domain("shop.amazon.com.au"), "amazon.com.au");
        assert_eq!(extract_main_domain("bbc.co.uk"), "bbc.co.uk");
        assert_eq!(extract_main_domain("news.bbc.co.uk"), "bbc.co.uk");
        assert_eq!(extract_main_domain("example.co.nz"), "example.co.nz");
        assert_eq!(extract_main_domain("shop.example.co.jp"), "example.co.jp");
    }

    #[test]
    fn test_sanitize_label_name() {
        assert_eq!(sanitize_label_name("example.com"), "example-com");
        assert_eq!(sanitize_label_name("user@domain.com"), "user-at-domain-com");
    }

    #[test]
    fn test_classification() {
        let classifier = EmailClassifier::new();

        let msg = create_test_message("noreply@github.com", "Pull Request Notification");
        let classification = classifier.classify(&msg).unwrap();

        assert_eq!(classification.message_id, "test-id");
        assert!(classification.confidence > 0.0);
        assert!(classification.reasoning.is_some());
    }

    #[test]
    fn test_domain_clustering() {
        let classifier = EmailClassifier::new();

        let messages = vec![
            create_test_message("user1@example.com", "Test 1"),
            create_test_message("user2@example.com", "Test 2"),
            create_test_message("admin@test.org", "Test 3"),
        ];

        let clusters = classifier.cluster_by_domain(&messages);
        assert_eq!(clusters.get("example.com").unwrap().len(), 2);
        assert_eq!(clusters.get("test.org").unwrap().len(), 1);
    }
}
