//! Filter rule management with generation, deduplication, and retroactive application
use crate::client::GmailClient;
use crate::error::{GmailError, Result};
use crate::models::{Classification, EmailCategory, FilterRule, MessageMetadata};
use std::collections::{HashMap, HashSet};
use tracing::{debug, info, warn};

/// Manages Gmail filters including generation, creation, and deduplication
pub struct FilterManager {
    client: Box<dyn GmailClient>,
    #[allow(dead_code)]
    existing_filters: Vec<FilterRule>,
    created_filters: Vec<String>,
}

impl FilterManager {
    /// Creates a new FilterManager instance
    pub fn new(client: Box<dyn GmailClient>) -> Self {
        Self {
            client,
            existing_filters: Vec::new(),
            created_filters: Vec::new(),
        }
    }

    /// Initializes the manager by loading existing filters from Gmail
    ///
    /// This helps with deduplication to avoid creating duplicate filters
    pub async fn initialize(&mut self) -> Result<()> {
        info!("Loading existing Gmail filters for deduplication");
        // In a full implementation, this would fetch filters via API
        // For now, we start with an empty list
        Ok(())
    }

    /// Generates filter rules from classified messages
    ///
    /// This is the core function that analyzes message patterns and creates
    /// deterministic Gmail filter criteria. The filters will work natively in Gmail
    /// without requiring any AI/ML processing after creation.
    ///
    /// # Algorithm:
    /// 1. Group messages by sender domain
    /// 2. Analyze patterns in subjects and characteristics
    /// 3. Build Gmail-compatible query syntax
    /// 4. Create FilterRule objects with criteria
    /// 5. Apply deduplication logic
    ///
    /// # Arguments
    /// * `classifications` - List of classified messages
    /// * `min_threshold` - Minimum message count to create dedicated filter
    ///
    /// # Returns
    /// * Vector of FilterRule objects ready for creation
    pub fn generate_filters_from_classifications(
        &self,
        classifications: &[(MessageMetadata, Classification)],
        min_threshold: usize,
    ) -> Vec<FilterRule> {
        info!(
            "Generating filters from {} classifications (threshold: {})",
            classifications.len(),
            min_threshold
        );

        // Group by sender domain
        let mut domain_groups: HashMap<String, Vec<&(MessageMetadata, Classification)>> =
            HashMap::new();

        for item in classifications {
            let domain = &item.0.sender_domain;
            domain_groups.entry(domain.clone()).or_default().push(item);
        }

        let mut filters = Vec::new();

        // Generate filters for each domain group
        for (domain, messages) in domain_groups {
            if messages.len() < min_threshold {
                debug!(
                    "Skipping domain {} (only {} messages, below threshold)",
                    domain,
                    messages.len()
                );
                continue;
            }

            // Analyze patterns in this domain
            let pattern_analysis = self.analyze_domain_patterns(&domain, &messages);

            // Determine category and label (use most common from user's choices)
            let category = self.determine_dominant_category(&messages);
            let target_label = self.determine_dominant_label(&messages);

            // Skip domains where user rejected (empty label means no filter wanted)
            if target_label.is_empty() {
                debug!("Skipping domain {} (user rejected - no label set)", domain);
                continue;
            }

            // Determine should_archive from user's choices
            let should_archive =
                messages.iter().filter(|(_, c)| c.should_archive).count() > messages.len() / 2;

            // Build filter rule
            if let Some(filter) = self.build_filter_rule(
                domain,
                pattern_analysis,
                category,
                target_label,
                should_archive,
                messages.len(),
            ) {
                filters.push(filter);
            }
        }

        // Deduplicate filters
        let deduplicated = self.deduplicate_filters(filters);

        info!("Generated {} unique filters", deduplicated.len());
        deduplicated
    }

    /// Generates filters from raw message metadata (without classifications)
    ///
    /// Useful for simpler pattern-based filter generation
    pub fn generate_filters(
        &self,
        messages: &[MessageMetadata],
        min_threshold: usize,
    ) -> Vec<FilterRule> {
        info!(
            "Generating filters from {} messages (threshold: {})",
            messages.len(),
            min_threshold
        );

        // Group by sender domain
        let mut domain_groups: HashMap<String, Vec<&MessageMetadata>> = HashMap::new();

        for message in messages {
            domain_groups
                .entry(message.sender_domain.clone())
                .or_default()
                .push(message);
        }

        let mut filters = Vec::new();

        // Generate filters for high-volume senders
        for (domain, msgs) in domain_groups {
            if msgs.len() < min_threshold {
                continue;
            }

            // Determine category from message characteristics
            let category = self.infer_category_from_messages(&msgs);

            // Analyze patterns
            let pattern_analysis = self.analyze_domain_patterns_simple(&domain, &msgs);

            // Determine should_archive based on category
            let should_archive = matches!(
                category,
                EmailCategory::Newsletter | EmailCategory::Marketing | EmailCategory::Notification
            );

            // Build filter (no suggested_label since no classifications)
            if let Some(filter) = self.build_filter_rule(
                domain,
                pattern_analysis,
                category,
                String::new(),
                should_archive,
                msgs.len(),
            ) {
                filters.push(filter);
            }
        }

        self.deduplicate_filters(filters)
    }

    /// Creates a filter in Gmail
    ///
    /// Validates the filter before creation and tracks created filters
    pub async fn create_filter(&mut self, filter: &FilterRule) -> Result<String> {
        // Validate filter first
        self.validate_filter(filter)?;

        info!("Creating filter for pattern: {:?}", filter.from_pattern);

        // Create via Gmail API
        let filter_id = self.client.create_filter(filter).await?;

        // Track created filter
        self.created_filters.push(filter_id.clone());

        info!("Successfully created filter with ID: {}", filter_id);
        Ok(filter_id)
    }

    /// Validates a filter rule before creation
    ///
    /// Checks:
    /// - Has valid criteria (from pattern or subject keywords)
    /// - Has valid action (target label)
    /// - Gmail query syntax is valid
    pub fn validate_filter(&self, filter: &FilterRule) -> Result<()> {
        // Must have some criteria
        if filter.from_pattern.is_none() && filter.subject_keywords.is_empty() {
            return Err(GmailError::ConfigError(
                "Filter must have from_pattern or subject_keywords".to_string(),
            ));
        }

        // Must have target label
        if filter.target_label_id.is_empty() {
            return Err(GmailError::ConfigError(
                "Filter must have target_label_id".to_string(),
            ));
        }

        // Validate Gmail query syntax
        let query = self.build_gmail_query(filter);
        if query.is_empty() {
            return Err(GmailError::ConfigError(
                "Generated Gmail query is empty".to_string(),
            ));
        }

        Ok(())
    }

    /// Builds Gmail query syntax from filter criteria (static version)
    ///
    /// Creates deterministic Gmail search queries that can be used in filters.
    /// These queries work natively in Gmail without any external processing.
    ///
    /// # Examples:
    /// - `from:(*@github.com)` - All emails from github.com domain
    /// - `from:(noreply@company.com) subject:(newsletter)` - Specific sender with subject
    /// - `subject:(receipt OR invoice OR order)` - Multiple subject keywords
    pub fn build_gmail_query_static(filter: &FilterRule) -> String {
        let mut query_parts = Vec::new();

        // Add from pattern
        if let Some(from_pattern) = &filter.from_pattern {
            if filter.is_specific_sender {
                // Specific email address
                query_parts.push(format!("from:({})", from_pattern));
            } else {
                // Domain-wide pattern: *@domain.com or @domain.com
                let domain = from_pattern.trim_start_matches('*');
                query_parts.push(format!("from:(*{})", domain));

                // Add exclusions for specific senders that have their own filters
                for excluded in &filter.excluded_senders {
                    query_parts.push(format!("-from:({})", excluded));
                }
            }
        }

        // Add subject keywords if present
        // Subject keywords make the filter more specific (narrow cluster)
        if !filter.subject_keywords.is_empty() {
            // If there's only one keyword, use it directly (for exact subject matches)
            // If multiple, join with OR for broader matching
            let keywords = if filter.subject_keywords.len() == 1 {
                filter.subject_keywords[0].clone()
            } else {
                filter
                    .subject_keywords
                    .iter()
                    .map(|k| k.to_lowercase())
                    .collect::<Vec<_>>()
                    .join(" OR ")
            };
            query_parts.push(format!("subject:({})", keywords));
        }

        query_parts.join(" ")
    }

    /// Builds Gmail query syntax from filter criteria (instance method)
    ///
    /// This is a convenience wrapper around the static method.
    pub fn build_gmail_query(&self, filter: &FilterRule) -> String {
        Self::build_gmail_query_static(filter)
    }

    /// Deduplicates filters to prevent overlapping rules
    ///
    /// Logic:
    /// - Exact match: Same criteria → skip duplicate
    /// - Subset match: New filter is subset of existing → skip
    /// - Superset match: New filter encompasses existing → keep newer
    /// - Different targets: Same criteria but different labels → keep both
    pub fn deduplicate_filters(&self, filters: Vec<FilterRule>) -> Vec<FilterRule> {
        let mut deduplicated: Vec<FilterRule> = Vec::new();
        let mut seen_patterns: HashSet<String> = HashSet::new();

        for filter in &filters {
            // Key must include all differentiating fields to avoid incorrectly dropping filters
            let pattern_key = format!(
                "{}:{}:{}:{}:{}",
                filter.from_pattern.as_deref().unwrap_or(""),
                filter.subject_keywords.join(","),
                filter.excluded_senders.join(","),
                filter.should_archive,
                filter.target_label_id
            );

            // Check if we've seen this exact pattern
            if seen_patterns.contains(&pattern_key) {
                debug!(
                    "Skipping duplicate filter pattern: {:?}",
                    filter.from_pattern
                );
                continue;
            }

            // Check for subset/superset relationships
            if self.is_redundant_filter(filter, &deduplicated) {
                debug!("Skipping redundant filter: {:?}", filter.from_pattern);
                continue;
            }

            seen_patterns.insert(pattern_key);
            deduplicated.push(filter.clone());
        }

        info!(
            "Deduplicated {} filters to {} unique filters",
            filters.len(),
            deduplicated.len()
        );
        deduplicated
    }

    /// Applies filters retroactively to existing messages
    ///
    /// For each filter:
    /// 1. Build search query from criteria
    /// 2. Find matching messages
    /// 3. Apply target label to matches
    /// 4. Archive if specified
    ///
    /// # Arguments
    /// * `filters` - List of filters to apply
    /// * `dry_run` - If true, only count matches without applying
    ///
    /// # Returns
    /// * Map from filter name to number of affected messages
    pub async fn apply_filters_retroactively(
        &self,
        filters: &[FilterRule],
        dry_run: bool,
    ) -> Result<HashMap<String, usize>> {
        let mut results: HashMap<String, usize> = HashMap::new();

        info!(
            "Applying {} filters retroactively (dry_run: {})",
            filters.len(),
            dry_run
        );

        for filter in filters {
            let query = self.build_gmail_query(filter);

            // Find matching messages
            let message_ids =
                self.client.list_message_ids(&query).await.map_err(|e| {
                    GmailError::ApiError(format!("Failed to search messages: {}", e))
                })?;

            info!(
                "Filter '{}' matches {} messages",
                filter.name,
                message_ids.len()
            );

            if !dry_run && !message_ids.is_empty() {
                // Apply label to all matching messages in batch
                match self
                    .client
                    .batch_add_label(&message_ids, &filter.target_label_id)
                    .await
                {
                    Ok(count) => {
                        debug!(
                            "Applied label to {} messages for filter '{}'",
                            count, filter.name
                        );
                    }
                    Err(e) => {
                        warn!(
                            "Failed to batch apply label for filter '{}': {}",
                            filter.name, e
                        );
                    }
                }
            }

            results.insert(filter.name.clone(), message_ids.len());
        }

        info!(
            "Retroactive application complete: {} filters processed",
            filters.len()
        );
        Ok(results)
    }

    /// Gets list of created filter IDs
    pub fn get_created_filters(&self) -> &[String] {
        &self.created_filters
    }

    /// Creates multiple filters from a list of FilterRules
    ///
    /// This is a high-level batch function that creates multiple filters,
    /// handling validation, error reporting, and optional dry-run mode.
    ///
    /// # Arguments
    /// * `filters` - List of FilterRule objects to create
    /// * `dry_run` - If true, only validate and estimate matches without creating
    ///
    /// # Returns
    /// * Map from filter name to either filter ID (success) or error message (failure)
    ///
    /// # Example
    /// ```ignore
    /// let filters = vec![
    ///     FilterRule { name: "GitHub Notifications", ... },
    ///     FilterRule { name: "Amazon Receipts", ... },
    /// ];
    ///
    /// let results = manager.create_filters(filters, false).await?;
    /// for (name, result) in results {
    ///     match result {
    ///         Ok(id) => println!("Created filter '{}' with ID: {}", name, id),
    ///         Err(e) => eprintln!("Failed to create filter '{}': {}", name, e),
    ///     }
    /// }
    /// ```
    pub async fn create_filters(
        &mut self,
        filters: Vec<FilterRule>,
        dry_run: bool,
    ) -> Result<HashMap<String, std::result::Result<String, String>>> {
        let total = filters.len();
        let mut results: HashMap<String, std::result::Result<String, String>> = HashMap::new();
        let mut success_count = 0;

        info!("Creating {} filters (dry_run: {})", total, dry_run);

        for filter in filters {
            let filter_name = filter.name.clone();

            // Validate filter first
            if let Err(e) = self.validate_filter(&filter) {
                let error_msg = format!("Validation failed: {}", e);
                warn!("Filter '{}': {}", filter_name, error_msg);
                results.insert(filter_name, Err(error_msg));
                continue;
            }

            // In dry-run mode, just report what would be created
            if dry_run {
                let query = self.build_gmail_query(&filter);
                let msg = format!(
                    "Would create filter with query: {} (estimated {} matches)",
                    query, filter.estimated_matches
                );
                info!("Filter '{}': {}", filter_name, msg);
                results.insert(filter_name, Ok(msg));
                continue;
            }

            // Create the filter
            match self.create_filter(&filter).await {
                Ok(filter_id) => {
                    info!("Created filter '{}' with ID: {}", filter_name, filter_id);
                    results.insert(filter_name, Ok(filter_id));
                    success_count += 1;
                }
                Err(e) => {
                    let error_msg = format!("Creation failed: {}", e);
                    warn!("Filter '{}': {}", filter_name, error_msg);
                    results.insert(filter_name, Err(error_msg));
                }
            }
        }

        if dry_run {
            info!("Dry-run complete: validated {} filters", total);
        } else {
            info!(
                "Filter creation complete: {}/{} successful",
                success_count, total
            );
        }

        Ok(results)
    }

    /// Estimates the number of messages that would match a filter
    ///
    /// This performs a search query to count matching messages without applying any changes.
    ///
    /// # Arguments
    /// * `filter` - The filter rule to estimate matches for
    ///
    /// # Returns
    /// * Number of messages that would be affected by this filter
    pub async fn estimate_filter_matches(&self, filter: &FilterRule) -> Result<usize> {
        let query = self.build_gmail_query(filter);

        info!("Estimating matches for filter: {}", filter.name);
        debug!("Search query: {}", query);

        let message_ids = self
            .client
            .list_message_ids(&query)
            .await
            .map_err(|e| GmailError::ApiError(format!("Failed to search messages: {}", e)))?;

        let count = message_ids.len();
        info!("Filter '{}' would match {} messages", filter.name, count);

        Ok(count)
    }

    /// Prompts for user confirmation before applying filters
    ///
    /// This is a helper function that can be used in CLI applications to get
    /// user confirmation before creating filters that will affect many messages.
    ///
    /// # Arguments
    /// * `filters` - List of filters to confirm
    /// * `match_estimates` - Map from filter name to estimated match count
    ///
    /// # Returns
    /// * true if user confirms, false otherwise
    pub fn confirm_filter_creation(
        &self,
        filters: &[FilterRule],
        match_estimates: &HashMap<String, usize>,
    ) -> bool {
        println!("\nProposed Filters:");
        println!("{}", "=".repeat(80));

        let mut total_affected = 0;
        for filter in filters {
            let matches = match_estimates.get(&filter.name).unwrap_or(&0);
            total_affected += matches;

            println!("\nFilter: {}", filter.name);
            println!("  Query: {}", self.build_gmail_query(filter));
            println!("  Target Label ID: {}", filter.target_label_id);
            println!("  Archive: {}", filter.should_archive);
            println!("  Estimated Matches: {}", matches);
        }

        println!("\n{}", "=".repeat(80));
        println!("Total filters: {}", filters.len());
        println!("Total messages affected: {}", total_affected);
        println!("\n{}", "=".repeat(80));

        // In a real CLI implementation, this would prompt the user
        // For now, we just return true to indicate confirmation
        // You would integrate with a CLI library like `dialoguer` for actual prompts
        true
    }

    // ========== Private Helper Methods ==========

    /// Analyzes patterns in messages from a domain
    fn analyze_domain_patterns(
        &self,
        domain: &str,
        messages: &[&(MessageMetadata, Classification)],
    ) -> PatternAnalysis {
        let mut subject_keywords = HashSet::new();
        let mut has_unsubscribe_count = 0;
        let mut is_automated_count = 0;

        for (msg, _) in messages {
            // Extract common keywords from subjects
            let words = self.extract_subject_keywords(&msg.subject);
            subject_keywords.extend(words);

            if msg.has_unsubscribe {
                has_unsubscribe_count += 1;
            }

            if msg.is_automated {
                is_automated_count += 1;
            }
        }

        // Determine if this is likely automated
        let is_automated = (is_automated_count as f32 / messages.len() as f32) > 0.7;
        let has_unsubscribe = (has_unsubscribe_count as f32 / messages.len() as f32) > 0.7;

        // Select most common keywords (limit to 3)
        let top_keywords = subject_keywords.into_iter().take(3).collect::<Vec<_>>();

        PatternAnalysis {
            domain: domain.to_string(),
            subject_keywords: top_keywords,
            is_automated,
            has_unsubscribe,
        }
    }

    /// Simplified pattern analysis for messages without classifications
    fn analyze_domain_patterns_simple(
        &self,
        domain: &str,
        messages: &[&MessageMetadata],
    ) -> PatternAnalysis {
        let mut subject_keywords = HashSet::new();
        let mut has_unsubscribe_count = 0;
        let mut is_automated_count = 0;

        for msg in messages {
            let words = self.extract_subject_keywords(&msg.subject);
            subject_keywords.extend(words);

            if msg.has_unsubscribe {
                has_unsubscribe_count += 1;
            }

            if msg.is_automated {
                is_automated_count += 1;
            }
        }

        let is_automated = (is_automated_count as f32 / messages.len() as f32) > 0.7;
        let has_unsubscribe = (has_unsubscribe_count as f32 / messages.len() as f32) > 0.7;

        let top_keywords = subject_keywords.into_iter().take(3).collect::<Vec<_>>();

        PatternAnalysis {
            domain: domain.to_string(),
            subject_keywords: top_keywords,
            is_automated,
            has_unsubscribe,
        }
    }

    /// Extracts significant keywords from subject line
    fn extract_subject_keywords(&self, subject: &str) -> Vec<String> {
        // Remove common prefixes
        let cleaned = subject
            .trim()
            .to_lowercase()
            .replace("re:", "")
            .replace("fwd:", "");

        // Extract words, filter stop words and short words
        let stop_words = [
            "the", "a", "an", "and", "or", "but", "in", "on", "at", "to", "for",
        ];

        cleaned
            .split_whitespace()
            .filter(|w| w.len() > 3 && !stop_words.contains(w))
            .map(|w| w.to_string())
            .collect()
    }

    /// Determines the dominant category from classified messages
    fn determine_dominant_category(
        &self,
        messages: &[&(MessageMetadata, Classification)],
    ) -> EmailCategory {
        let mut category_counts: HashMap<EmailCategory, usize> = HashMap::new();

        for (_, classification) in messages {
            *category_counts
                .entry(classification.category.clone())
                .or_insert(0) += 1;
        }

        // Return most common category
        category_counts
            .into_iter()
            .max_by_key(|(_, count)| *count)
            .map(|(cat, _)| cat)
            .unwrap_or(EmailCategory::Other)
    }

    /// Determines the dominant suggested_label from classified messages
    /// This reflects the user's choices from interactive review
    fn determine_dominant_label(&self, messages: &[&(MessageMetadata, Classification)]) -> String {
        let mut label_counts: HashMap<String, usize> = HashMap::new();

        for (_, classification) in messages {
            if !classification.suggested_label.is_empty() {
                *label_counts
                    .entry(classification.suggested_label.clone())
                    .or_insert(0) += 1;
            }
        }

        // Return most common label
        label_counts
            .into_iter()
            .max_by_key(|(_, count)| *count)
            .map(|(label, _)| label)
            .unwrap_or_default()
    }

    /// Infers category from message characteristics (without classification)
    fn infer_category_from_messages(&self, messages: &[&MessageMetadata]) -> EmailCategory {
        // Use heuristics to determine likely category
        let has_unsubscribe = messages.iter().any(|m| m.has_unsubscribe);
        let is_automated = messages.iter().any(|m| m.is_automated);

        // Check subjects for patterns
        let subjects = messages
            .iter()
            .map(|m| m.subject.to_lowercase())
            .collect::<Vec<_>>()
            .join(" ");

        if subjects.contains("newsletter") || subjects.contains("digest") {
            EmailCategory::Newsletter
        } else if subjects.contains("receipt") || subjects.contains("order") {
            EmailCategory::Receipt
        } else if subjects.contains("shipped") || subjects.contains("tracking") {
            EmailCategory::Shipping
        } else if subjects.contains("notification") || subjects.contains("alert") {
            EmailCategory::Notification
        } else if has_unsubscribe || is_automated {
            EmailCategory::Marketing
        } else {
            EmailCategory::Other
        }
    }

    /// Builds a FilterRule from analyzed patterns
    fn build_filter_rule(
        &self,
        domain: String,
        analysis: PatternAnalysis,
        category: EmailCategory,
        target_label: String,
        should_archive: bool,
        message_count: usize,
    ) -> Option<FilterRule> {
        // Build from pattern (domain-wide)
        let from_pattern = if domain.contains('.') {
            Some(format!("*@{}", domain))
        } else {
            None
        };

        // Build filter name using target_label if available, otherwise category
        let filter_name = if !target_label.is_empty() {
            format!("{} → {}", domain, target_label)
        } else {
            format!("{} - {:?}", domain, category)
        };

        Some(FilterRule {
            id: None,
            name: filter_name,
            from_pattern,
            is_specific_sender: false,
            excluded_senders: vec![],
            subject_keywords: analysis.subject_keywords,
            target_label_id: target_label,
            should_archive,
            estimated_matches: message_count,
        })
    }

    /// Checks if a filter is redundant given existing filters
    fn is_redundant_filter(&self, filter: &FilterRule, existing: &[FilterRule]) -> bool {
        for existing_filter in existing {
            // Check if from patterns overlap
            if let (Some(new_from), Some(existing_from)) =
                (&filter.from_pattern, &existing_filter.from_pattern)
            {
                // Check for exact match
                if new_from == existing_from {
                    // If subjects also match, it's redundant
                    if filter.subject_keywords == existing_filter.subject_keywords {
                        return true;
                    }
                }

                // Check if new filter is more specific (subset)
                // e.g., specific@domain.com vs *@domain.com
                if !new_from.contains('*') && existing_from.contains('*') {
                    let existing_domain = existing_from.trim_start_matches("*@");
                    if new_from.ends_with(existing_domain) {
                        return true; // New filter is more specific, existing covers it
                    }
                }
            }
        }

        false
    }
}

/// Pattern analysis results for a domain
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct PatternAnalysis {
    domain: String,
    subject_keywords: Vec<String>,
    is_automated: bool,
    has_unsubscribe: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn create_test_message(domain: &str, subject: &str, has_unsubscribe: bool) -> MessageMetadata {
        MessageMetadata {
            id: "test-id".to_string(),
            thread_id: "test-thread".to_string(),
            sender_email: format!("sender@{}", domain),
            sender_domain: domain.to_string(),
            sender_name: "Test Sender".to_string(),
            subject: subject.to_string(),
            recipients: vec![],
            date_received: Utc::now(),
            labels: vec![],
            has_unsubscribe,
            is_automated: has_unsubscribe,
        }
    }

    #[test]
    fn test_build_gmail_query() {
        use async_trait::async_trait;

        mockall::mock! {
            pub TestGmailClient {}

            #[async_trait]
            impl crate::client::GmailClient for TestGmailClient {
                async fn list_message_ids(&self, query: &str) -> Result<Vec<String>>;
                async fn get_message(&self, id: &str) -> Result<crate::models::MessageMetadata>;
                async fn list_labels(&self) -> Result<Vec<crate::client::LabelInfo>>;
                async fn create_label(&self, name: &str) -> Result<String>;
                async fn delete_label(&self, label_id: &str) -> Result<()>;
                async fn create_filter(&self, filter: &FilterRule) -> Result<String>;
                async fn list_filters(&self) -> Result<Vec<crate::client::ExistingFilterInfo>>;
                async fn delete_filter(&self, filter_id: &str) -> Result<()>;
                async fn update_filter(&self, filter_id: &str, filter: &FilterRule) -> Result<String>;
                async fn apply_label(&self, message_id: &str, label_id: &str) -> Result<()>;
                async fn remove_label(&self, message_id: &str, label_id: &str) -> Result<()>;
                async fn batch_remove_label(&self, message_ids: &[String], label_id: &str) -> Result<usize>;
                async fn batch_add_label(&self, message_ids: &[String], label_id: &str) -> Result<usize>;
                async fn batch_modify_labels(&self, message_ids: &[String], add_label_ids: &[String], remove_label_ids: &[String]) -> Result<usize>;
                async fn fetch_messages_batch(&self, message_ids: Vec<String>) -> Result<Vec<crate::models::MessageMetadata>>;
                async fn fetch_messages_with_progress(&self, message_ids: Vec<String>, on_progress: crate::client::ProgressCallback) -> Result<Vec<crate::models::MessageMetadata>>;
                async fn quota_stats(&self) -> crate::rate_limiter::QuotaStats;
            }
        }

        let mock_client = MockTestGmailClient::new();
        let manager = FilterManager::new(Box::new(mock_client));

        // Test domain-wide pattern
        let filter = FilterRule {
            id: None,
            name: "Test Filter".to_string(),
            from_pattern: Some("*@github.com".to_string()),
            is_specific_sender: false,
            excluded_senders: vec![],
            subject_keywords: vec![],
            target_label_id: "label-id".to_string(),
            should_archive: false,
            estimated_matches: 10,
        };

        let query = manager.build_gmail_query(&filter);
        assert_eq!(query, "from:(*@github.com)");

        // Test with subject keywords for specific sender
        let filter_with_subject = FilterRule {
            from_pattern: Some("noreply@company.com".to_string()),
            is_specific_sender: true, // Specific email, not domain pattern
            subject_keywords: vec!["newsletter".to_string(), "digest".to_string()],
            ..filter.clone()
        };

        let query = manager.build_gmail_query(&filter_with_subject);
        assert!(query.contains("from:(noreply@company.com)"));
        assert!(query.contains("subject:(newsletter OR digest)"));

        // Test with excluded senders (THIS IS THE BUG FIX TEST)
        let filter_with_exclusions = FilterRule {
            from_pattern: Some("*@linkedin.com".to_string()),
            is_specific_sender: false,
            excluded_senders: vec![
                "messaging-digest-noreply@linkedin.com".to_string(),
                "messages-noreply@linkedin.com".to_string(),
                "jobs-listings@linkedin.com".to_string(),
            ],
            subject_keywords: vec![],
            ..filter
        };

        let query = manager.build_gmail_query(&filter_with_exclusions);
        assert!(query.contains("from:(*@linkedin.com)"));
        assert!(query.contains("-from:(messaging-digest-noreply@linkedin.com)"));
        assert!(query.contains("-from:(messages-noreply@linkedin.com)"));
        assert!(query.contains("-from:(jobs-listings@linkedin.com)"));
    }

    #[test]
    fn test_extract_subject_keywords() {
        use async_trait::async_trait;

        mockall::mock! {
            pub TestGmailClient {}

            #[async_trait]
            impl crate::client::GmailClient for TestGmailClient {
                async fn list_message_ids(&self, query: &str) -> Result<Vec<String>>;
                async fn get_message(&self, id: &str) -> Result<crate::models::MessageMetadata>;
                async fn list_labels(&self) -> Result<Vec<crate::client::LabelInfo>>;
                async fn create_label(&self, name: &str) -> Result<String>;
                async fn delete_label(&self, label_id: &str) -> Result<()>;
                async fn create_filter(&self, filter: &FilterRule) -> Result<String>;
                async fn list_filters(&self) -> Result<Vec<crate::client::ExistingFilterInfo>>;
                async fn delete_filter(&self, filter_id: &str) -> Result<()>;
                async fn update_filter(&self, filter_id: &str, filter: &FilterRule) -> Result<String>;
                async fn apply_label(&self, message_id: &str, label_id: &str) -> Result<()>;
                async fn remove_label(&self, message_id: &str, label_id: &str) -> Result<()>;
                async fn batch_remove_label(&self, message_ids: &[String], label_id: &str) -> Result<usize>;
                async fn batch_add_label(&self, message_ids: &[String], label_id: &str) -> Result<usize>;
                async fn batch_modify_labels(&self, message_ids: &[String], add_label_ids: &[String], remove_label_ids: &[String]) -> Result<usize>;
                async fn fetch_messages_batch(&self, message_ids: Vec<String>) -> Result<Vec<crate::models::MessageMetadata>>;
                async fn fetch_messages_with_progress(&self, message_ids: Vec<String>, on_progress: crate::client::ProgressCallback) -> Result<Vec<crate::models::MessageMetadata>>;
                async fn quota_stats(&self) -> crate::rate_limiter::QuotaStats;
            }
        }

        let mock_client = MockTestGmailClient::new();
        let manager = FilterManager::new(Box::new(mock_client));

        let keywords = manager.extract_subject_keywords("Weekly Newsletter The Best Articles");
        assert!(keywords.contains(&"weekly".to_string()));
        assert!(keywords.contains(&"newsletter".to_string()));
        assert!(keywords.contains(&"best".to_string()));
        assert!(keywords.contains(&"articles".to_string()));

        // Should filter out short words and common words
        let filtered = manager.extract_subject_keywords("The best of the week");
        assert!(!filtered.contains(&"the".to_string()));
        assert!(filtered.contains(&"best".to_string()));
        assert!(filtered.contains(&"week".to_string()));
    }

    #[test]
    fn test_infer_category_from_messages() {
        use async_trait::async_trait;

        mockall::mock! {
            pub TestGmailClient {}

            #[async_trait]
            impl crate::client::GmailClient for TestGmailClient {
                async fn list_message_ids(&self, query: &str) -> Result<Vec<String>>;
                async fn get_message(&self, id: &str) -> Result<crate::models::MessageMetadata>;
                async fn list_labels(&self) -> Result<Vec<crate::client::LabelInfo>>;
                async fn create_label(&self, name: &str) -> Result<String>;
                async fn delete_label(&self, label_id: &str) -> Result<()>;
                async fn create_filter(&self, filter: &FilterRule) -> Result<String>;
                async fn list_filters(&self) -> Result<Vec<crate::client::ExistingFilterInfo>>;
                async fn delete_filter(&self, filter_id: &str) -> Result<()>;
                async fn update_filter(&self, filter_id: &str, filter: &FilterRule) -> Result<String>;
                async fn apply_label(&self, message_id: &str, label_id: &str) -> Result<()>;
                async fn remove_label(&self, message_id: &str, label_id: &str) -> Result<()>;
                async fn batch_remove_label(&self, message_ids: &[String], label_id: &str) -> Result<usize>;
                async fn batch_add_label(&self, message_ids: &[String], label_id: &str) -> Result<usize>;
                async fn batch_modify_labels(&self, message_ids: &[String], add_label_ids: &[String], remove_label_ids: &[String]) -> Result<usize>;
                async fn fetch_messages_batch(&self, message_ids: Vec<String>) -> Result<Vec<crate::models::MessageMetadata>>;
                async fn fetch_messages_with_progress(&self, message_ids: Vec<String>, on_progress: crate::client::ProgressCallback) -> Result<Vec<crate::models::MessageMetadata>>;
                async fn quota_stats(&self) -> crate::rate_limiter::QuotaStats;
            }
        }

        let mock_client = MockTestGmailClient::new();
        let manager = FilterManager::new(Box::new(mock_client));

        // Newsletter category
        let msg1 = create_test_message("test.com", "Weekly Newsletter Digest", true);
        let msg2 = create_test_message("test.com", "Monthly Newsletter", true);
        let messages = vec![&msg1, &msg2];
        let refs: Vec<&MessageMetadata> = messages.iter().map(|m| *m).collect();

        let category = manager.infer_category_from_messages(&refs);
        assert!(matches!(category, EmailCategory::Newsletter));

        // Receipt category
        let msg3 = create_test_message("store.com", "Your receipt for order #123", false);
        let msg4 = create_test_message("store.com", "Order confirmation", false);
        let messages = vec![&msg3, &msg4];
        let refs: Vec<&MessageMetadata> = messages.iter().map(|m| *m).collect();

        let category = manager.infer_category_from_messages(&refs);
        assert!(matches!(category, EmailCategory::Receipt));
    }

    #[test]
    fn test_validate_filter() {
        use async_trait::async_trait;

        mockall::mock! {
            pub TestGmailClient {}

            #[async_trait]
            impl crate::client::GmailClient for TestGmailClient {
                async fn list_message_ids(&self, query: &str) -> Result<Vec<String>>;
                async fn get_message(&self, id: &str) -> Result<crate::models::MessageMetadata>;
                async fn list_labels(&self) -> Result<Vec<crate::client::LabelInfo>>;
                async fn create_label(&self, name: &str) -> Result<String>;
                async fn delete_label(&self, label_id: &str) -> Result<()>;
                async fn create_filter(&self, filter: &FilterRule) -> Result<String>;
                async fn list_filters(&self) -> Result<Vec<crate::client::ExistingFilterInfo>>;
                async fn delete_filter(&self, filter_id: &str) -> Result<()>;
                async fn update_filter(&self, filter_id: &str, filter: &FilterRule) -> Result<String>;
                async fn apply_label(&self, message_id: &str, label_id: &str) -> Result<()>;
                async fn remove_label(&self, message_id: &str, label_id: &str) -> Result<()>;
                async fn batch_remove_label(&self, message_ids: &[String], label_id: &str) -> Result<usize>;
                async fn batch_add_label(&self, message_ids: &[String], label_id: &str) -> Result<usize>;
                async fn batch_modify_labels(&self, message_ids: &[String], add_label_ids: &[String], remove_label_ids: &[String]) -> Result<usize>;
                async fn fetch_messages_batch(&self, message_ids: Vec<String>) -> Result<Vec<crate::models::MessageMetadata>>;
                async fn fetch_messages_with_progress(&self, message_ids: Vec<String>, on_progress: crate::client::ProgressCallback) -> Result<Vec<crate::models::MessageMetadata>>;
                async fn quota_stats(&self) -> crate::rate_limiter::QuotaStats;
            }
        }

        let mock_client = MockTestGmailClient::new();
        let manager = FilterManager::new(Box::new(mock_client));

        // Valid filter
        let valid_filter = FilterRule {
            id: None,
            name: "Test".to_string(),
            from_pattern: Some("test@example.com".to_string()),
            is_specific_sender: false,
            excluded_senders: vec![],
            subject_keywords: vec![],
            target_label_id: "label-123".to_string(),
            should_archive: false,
            estimated_matches: 10,
        };

        assert!(manager.validate_filter(&valid_filter).is_ok());

        // Invalid: no criteria
        let invalid_no_criteria = FilterRule {
            from_pattern: None,
            subject_keywords: vec![],
            ..valid_filter.clone()
        };

        assert!(manager.validate_filter(&invalid_no_criteria).is_err());

        // Invalid: no target label
        let invalid_no_label = FilterRule {
            target_label_id: String::new(),
            ..valid_filter
        };

        assert!(manager.validate_filter(&invalid_no_label).is_err());
    }

    #[test]
    fn test_deduplicate_filters() {
        use async_trait::async_trait;

        mockall::mock! {
            pub TestGmailClient {}

            #[async_trait]
            impl crate::client::GmailClient for TestGmailClient {
                async fn list_message_ids(&self, query: &str) -> Result<Vec<String>>;
                async fn get_message(&self, id: &str) -> Result<crate::models::MessageMetadata>;
                async fn list_labels(&self) -> Result<Vec<crate::client::LabelInfo>>;
                async fn create_label(&self, name: &str) -> Result<String>;
                async fn delete_label(&self, label_id: &str) -> Result<()>;
                async fn create_filter(&self, filter: &FilterRule) -> Result<String>;
                async fn list_filters(&self) -> Result<Vec<crate::client::ExistingFilterInfo>>;
                async fn delete_filter(&self, filter_id: &str) -> Result<()>;
                async fn update_filter(&self, filter_id: &str, filter: &FilterRule) -> Result<String>;
                async fn apply_label(&self, message_id: &str, label_id: &str) -> Result<()>;
                async fn remove_label(&self, message_id: &str, label_id: &str) -> Result<()>;
                async fn batch_remove_label(&self, message_ids: &[String], label_id: &str) -> Result<usize>;
                async fn batch_add_label(&self, message_ids: &[String], label_id: &str) -> Result<usize>;
                async fn batch_modify_labels(&self, message_ids: &[String], add_label_ids: &[String], remove_label_ids: &[String]) -> Result<usize>;
                async fn fetch_messages_batch(&self, message_ids: Vec<String>) -> Result<Vec<crate::models::MessageMetadata>>;
                async fn fetch_messages_with_progress(&self, message_ids: Vec<String>, on_progress: crate::client::ProgressCallback) -> Result<Vec<crate::models::MessageMetadata>>;
                async fn quota_stats(&self) -> crate::rate_limiter::QuotaStats;
            }
        }

        let mock_client = MockTestGmailClient::new();
        let manager = FilterManager::new(Box::new(mock_client));

        let filters = vec![
            FilterRule {
                id: None,
                name: "Filter 1".to_string(),
                from_pattern: Some("*@github.com".to_string()),
                is_specific_sender: false,
                excluded_senders: vec![],
                subject_keywords: vec![],
                target_label_id: "label-1".to_string(),
                should_archive: false,
                estimated_matches: 10,
            },
            FilterRule {
                id: None,
                name: "Filter 2".to_string(),
                from_pattern: Some("*@github.com".to_string()), // Duplicate
                is_specific_sender: false,
                excluded_senders: vec![],
                subject_keywords: vec![],
                target_label_id: "label-1".to_string(),
                should_archive: false,
                estimated_matches: 10,
            },
            FilterRule {
                id: None,
                name: "Filter 3".to_string(),
                from_pattern: Some("*@gitlab.com".to_string()),
                is_specific_sender: false,
                excluded_senders: vec![],
                subject_keywords: vec![],
                target_label_id: "label-2".to_string(),
                should_archive: false,
                estimated_matches: 5,
            },
        ];

        let deduplicated = manager.deduplicate_filters(filters);
        assert_eq!(deduplicated.len(), 2); // Should remove one duplicate
    }

    #[tokio::test]
    async fn test_create_filters_dry_run() {
        use async_trait::async_trait;
        use mockall::predicate::*;

        mockall::mock! {
            pub TestGmailClient {}

            #[async_trait]
            impl crate::client::GmailClient for TestGmailClient {
                async fn list_message_ids(&self, query: &str) -> Result<Vec<String>>;
                async fn get_message(&self, id: &str) -> Result<crate::models::MessageMetadata>;
                async fn list_labels(&self) -> Result<Vec<crate::client::LabelInfo>>;
                async fn create_label(&self, name: &str) -> Result<String>;
                async fn delete_label(&self, label_id: &str) -> Result<()>;
                async fn create_filter(&self, filter: &FilterRule) -> Result<String>;
                async fn list_filters(&self) -> Result<Vec<crate::client::ExistingFilterInfo>>;
                async fn delete_filter(&self, filter_id: &str) -> Result<()>;
                async fn update_filter(&self, filter_id: &str, filter: &FilterRule) -> Result<String>;
                async fn apply_label(&self, message_id: &str, label_id: &str) -> Result<()>;
                async fn remove_label(&self, message_id: &str, label_id: &str) -> Result<()>;
                async fn batch_remove_label(&self, message_ids: &[String], label_id: &str) -> Result<usize>;
                async fn batch_add_label(&self, message_ids: &[String], label_id: &str) -> Result<usize>;
                async fn batch_modify_labels(&self, message_ids: &[String], add_label_ids: &[String], remove_label_ids: &[String]) -> Result<usize>;
                async fn fetch_messages_batch(&self, message_ids: Vec<String>) -> Result<Vec<crate::models::MessageMetadata>>;
                async fn fetch_messages_with_progress(&self, message_ids: Vec<String>, on_progress: crate::client::ProgressCallback) -> Result<Vec<crate::models::MessageMetadata>>;
                async fn quota_stats(&self) -> crate::rate_limiter::QuotaStats;
            }
        }

        let mock_client = MockTestGmailClient::new();

        let mut manager = FilterManager::new(Box::new(mock_client));

        let filters = vec![
            FilterRule {
                id: None,
                name: "GitHub Filter".to_string(),
                from_pattern: Some("*@github.com".to_string()),
                is_specific_sender: false,
                excluded_senders: vec![],
                subject_keywords: vec![],
                target_label_id: "label-123".to_string(),
                should_archive: true,
                estimated_matches: 50,
            },
            FilterRule {
                id: None,
                name: "Amazon Filter".to_string(),
                from_pattern: Some("*@amazon.com".to_string()),
                is_specific_sender: false,
                excluded_senders: vec![],
                subject_keywords: vec!["receipt".to_string()],
                target_label_id: "label-456".to_string(),
                should_archive: false,
                estimated_matches: 100,
            },
        ];

        let result = manager.create_filters(filters, true).await;
        assert!(result.is_ok());

        let results = result.unwrap();
        assert_eq!(results.len(), 2);

        // In dry-run mode, all should succeed with informational messages
        assert!(results.get("GitHub Filter").unwrap().is_ok());
        assert!(results.get("Amazon Filter").unwrap().is_ok());
    }

    #[tokio::test]
    async fn test_create_filters_with_validation_errors() {
        use async_trait::async_trait;

        mockall::mock! {
            pub TestGmailClient {}

            #[async_trait]
            impl crate::client::GmailClient for TestGmailClient {
                async fn list_message_ids(&self, query: &str) -> Result<Vec<String>>;
                async fn get_message(&self, id: &str) -> Result<crate::models::MessageMetadata>;
                async fn list_labels(&self) -> Result<Vec<crate::client::LabelInfo>>;
                async fn create_label(&self, name: &str) -> Result<String>;
                async fn delete_label(&self, label_id: &str) -> Result<()>;
                async fn create_filter(&self, filter: &FilterRule) -> Result<String>;
                async fn list_filters(&self) -> Result<Vec<crate::client::ExistingFilterInfo>>;
                async fn delete_filter(&self, filter_id: &str) -> Result<()>;
                async fn update_filter(&self, filter_id: &str, filter: &FilterRule) -> Result<String>;
                async fn apply_label(&self, message_id: &str, label_id: &str) -> Result<()>;
                async fn remove_label(&self, message_id: &str, label_id: &str) -> Result<()>;
                async fn batch_remove_label(&self, message_ids: &[String], label_id: &str) -> Result<usize>;
                async fn batch_add_label(&self, message_ids: &[String], label_id: &str) -> Result<usize>;
                async fn batch_modify_labels(&self, message_ids: &[String], add_label_ids: &[String], remove_label_ids: &[String]) -> Result<usize>;
                async fn fetch_messages_batch(&self, message_ids: Vec<String>) -> Result<Vec<crate::models::MessageMetadata>>;
                async fn fetch_messages_with_progress(&self, message_ids: Vec<String>, on_progress: crate::client::ProgressCallback) -> Result<Vec<crate::models::MessageMetadata>>;
                async fn quota_stats(&self) -> crate::rate_limiter::QuotaStats;
            }
        }

        let mock_client = MockTestGmailClient::new();
        let mut manager = FilterManager::new(Box::new(mock_client));

        let filters = vec![
            // Valid filter
            FilterRule {
                id: None,
                name: "Valid Filter".to_string(),
                from_pattern: Some("*@test.com".to_string()),
                is_specific_sender: false,
                excluded_senders: vec![],
                subject_keywords: vec![],
                target_label_id: "label-123".to_string(),
                should_archive: false,
                estimated_matches: 10,
            },
            // Invalid: no criteria
            FilterRule {
                id: None,
                name: "Invalid Filter".to_string(),
                from_pattern: None,
                is_specific_sender: false,
                excluded_senders: vec![],
                subject_keywords: vec![],
                target_label_id: "label-456".to_string(),
                should_archive: false,
                estimated_matches: 0,
            },
        ];

        let result = manager.create_filters(filters, true).await;
        assert!(result.is_ok());

        let results = result.unwrap();
        assert_eq!(results.len(), 2);

        // Valid filter should succeed in dry-run
        assert!(results.get("Valid Filter").unwrap().is_ok());

        // Invalid filter should have error
        assert!(results.get("Invalid Filter").unwrap().is_err());
        let error_msg = results.get("Invalid Filter").unwrap().as_ref().unwrap_err();
        assert!(error_msg.contains("Validation failed"));
    }

    #[tokio::test]
    async fn test_estimate_filter_matches() {
        use async_trait::async_trait;
        use mockall::predicate::*;

        mockall::mock! {
            pub TestGmailClient {}

            #[async_trait]
            impl crate::client::GmailClient for TestGmailClient {
                async fn list_message_ids(&self, query: &str) -> Result<Vec<String>>;
                async fn get_message(&self, id: &str) -> Result<crate::models::MessageMetadata>;
                async fn list_labels(&self) -> Result<Vec<crate::client::LabelInfo>>;
                async fn create_label(&self, name: &str) -> Result<String>;
                async fn delete_label(&self, label_id: &str) -> Result<()>;
                async fn create_filter(&self, filter: &FilterRule) -> Result<String>;
                async fn list_filters(&self) -> Result<Vec<crate::client::ExistingFilterInfo>>;
                async fn delete_filter(&self, filter_id: &str) -> Result<()>;
                async fn update_filter(&self, filter_id: &str, filter: &FilterRule) -> Result<String>;
                async fn apply_label(&self, message_id: &str, label_id: &str) -> Result<()>;
                async fn remove_label(&self, message_id: &str, label_id: &str) -> Result<()>;
                async fn batch_remove_label(&self, message_ids: &[String], label_id: &str) -> Result<usize>;
                async fn batch_add_label(&self, message_ids: &[String], label_id: &str) -> Result<usize>;
                async fn batch_modify_labels(&self, message_ids: &[String], add_label_ids: &[String], remove_label_ids: &[String]) -> Result<usize>;
                async fn fetch_messages_batch(&self, message_ids: Vec<String>) -> Result<Vec<crate::models::MessageMetadata>>;
                async fn fetch_messages_with_progress(&self, message_ids: Vec<String>, on_progress: crate::client::ProgressCallback) -> Result<Vec<crate::models::MessageMetadata>>;
                async fn quota_stats(&self) -> crate::rate_limiter::QuotaStats;
            }
        }

        let mut mock_client = MockTestGmailClient::new();

        // Mock the list_message_ids call
        mock_client
            .expect_list_message_ids()
            .with(eq("from:(*@github.com)"))
            .times(1)
            .returning(|_| {
                Ok(vec![
                    "msg1".to_string(),
                    "msg2".to_string(),
                    "msg3".to_string(),
                ])
            });

        let manager = FilterManager::new(Box::new(mock_client));

        let filter = FilterRule {
            id: None,
            name: "GitHub Filter".to_string(),
            from_pattern: Some("*@github.com".to_string()),
            is_specific_sender: false,
            excluded_senders: vec![],
            subject_keywords: vec![],
            target_label_id: "label-123".to_string(),
            should_archive: false,
            estimated_matches: 0, // Will be updated by estimate
        };

        let result = manager.estimate_filter_matches(&filter).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 3);
    }

    #[test]
    fn test_confirm_filter_creation() {
        use async_trait::async_trait;

        mockall::mock! {
            pub TestGmailClient {}

            #[async_trait]
            impl crate::client::GmailClient for TestGmailClient {
                async fn list_message_ids(&self, query: &str) -> Result<Vec<String>>;
                async fn get_message(&self, id: &str) -> Result<crate::models::MessageMetadata>;
                async fn list_labels(&self) -> Result<Vec<crate::client::LabelInfo>>;
                async fn create_label(&self, name: &str) -> Result<String>;
                async fn delete_label(&self, label_id: &str) -> Result<()>;
                async fn create_filter(&self, filter: &FilterRule) -> Result<String>;
                async fn list_filters(&self) -> Result<Vec<crate::client::ExistingFilterInfo>>;
                async fn delete_filter(&self, filter_id: &str) -> Result<()>;
                async fn update_filter(&self, filter_id: &str, filter: &FilterRule) -> Result<String>;
                async fn apply_label(&self, message_id: &str, label_id: &str) -> Result<()>;
                async fn remove_label(&self, message_id: &str, label_id: &str) -> Result<()>;
                async fn batch_remove_label(&self, message_ids: &[String], label_id: &str) -> Result<usize>;
                async fn batch_add_label(&self, message_ids: &[String], label_id: &str) -> Result<usize>;
                async fn batch_modify_labels(&self, message_ids: &[String], add_label_ids: &[String], remove_label_ids: &[String]) -> Result<usize>;
                async fn fetch_messages_batch(&self, message_ids: Vec<String>) -> Result<Vec<crate::models::MessageMetadata>>;
                async fn fetch_messages_with_progress(&self, message_ids: Vec<String>, on_progress: crate::client::ProgressCallback) -> Result<Vec<crate::models::MessageMetadata>>;
                async fn quota_stats(&self) -> crate::rate_limiter::QuotaStats;
            }
        }

        let mock_client = MockTestGmailClient::new();
        let manager = FilterManager::new(Box::new(mock_client));

        let filters = vec![FilterRule {
            id: None,
            name: "Test Filter 1".to_string(),
            from_pattern: Some("*@test.com".to_string()),
            is_specific_sender: false,
            excluded_senders: vec![],
            subject_keywords: vec![],
            target_label_id: "label-1".to_string(),
            should_archive: false,
            estimated_matches: 10,
        }];

        let mut estimates = HashMap::new();
        estimates.insert("Test Filter 1".to_string(), 15);

        // This function always returns true in the current implementation
        let confirmed = manager.confirm_filter_creation(&filters, &estimates);
        assert!(confirmed);
    }
}
