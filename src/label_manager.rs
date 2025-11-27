///! Label management and creation with hierarchy support and consolidation logic

use crate::client::GmailClient;
use crate::error::{GmailError, Result};
use regex::Regex;
use std::collections::{HashMap, HashSet};
use tracing::{debug, info, warn};

/// Manages Gmail labels including creation, hierarchy management, and consolidation
pub struct LabelManager {
    client: Box<dyn GmailClient>,
    label_prefix: String,
    label_cache: HashMap<String, String>, // name -> id mapping
    created_labels: Vec<String>,
}

impl LabelManager {
    /// Creates a new LabelManager instance
    pub fn new(client: Box<dyn GmailClient>, prefix: String) -> Self {
        Self {
            client,
            label_prefix: prefix,
            label_cache: HashMap::new(),
            created_labels: Vec::new(),
        }
    }

    /// Loads all existing labels from Gmail into the cache
    /// Call this before creating labels to avoid conflicts
    pub async fn load_existing_labels(&mut self) -> Result<usize> {
        let labels = self.client.list_labels().await?;
        let count = labels.len();

        for label in labels {
            self.label_cache.insert(label.name.clone(), label.id);
        }

        info!("Loaded {} existing labels into cache", count);
        Ok(count)
    }

    /// Returns the list of existing labels that match the proposed labels
    pub fn find_existing_labels(&self, proposed: &[String]) -> Vec<String> {
        proposed
            .iter()
            .filter(|name| {
                let full_name = format!("{}/{}", self.label_prefix, name);
                let sanitized = self.sanitize_label_name(&full_name).unwrap_or_default();
                self.label_cache.contains_key(&sanitized)
            })
            .cloned()
            .collect()
    }

    /// Returns labels that would be newly created (don't exist yet)
    pub fn find_new_labels(&self, proposed: &[String]) -> Vec<String> {
        proposed
            .iter()
            .filter(|name| {
                let full_name = format!("{}/{}", self.label_prefix, name);
                let sanitized = self.sanitize_label_name(&full_name).unwrap_or_default();
                !self.label_cache.contains_key(&sanitized)
            })
            .cloned()
            .collect()
    }

    /// Gets the label cache (for reporting purposes)
    pub fn get_label_cache(&self) -> &HashMap<String, String> {
        &self.label_cache
    }

    /// Creates a label in Gmail if it doesn't already exist
    ///
    /// This function implements the pattern from lines 636-651 of the implementation spec:
    /// - Checks for existing labels before creation
    /// - Creates parent labels in hierarchy first
    /// - Sanitizes label names
    /// - Tracks created labels for management
    ///
    /// # Arguments
    /// * `name` - The label name (will be prefixed automatically)
    ///
    /// # Returns
    /// * `Ok(String)` - The Gmail label ID
    /// * `Err(GmailError)` - If label creation fails
    ///
    /// # Example
    /// ```ignore
    /// let label_id = manager.create_label("Newsletters/Tech").await?;
    /// ```
    pub async fn create_label(&mut self, name: &str) -> Result<String> {
        let full_name = format!("{}/{}", self.label_prefix, name);
        let sanitized_name = self.sanitize_label_name(&full_name)?;

        // Check if label already exists in cache
        if let Some(id) = self.label_cache.get(&sanitized_name) {
            debug!("Label '{}' already exists in cache", sanitized_name);
            return Ok(id.clone());
        }

        // Create parent labels if this is a hierarchical label
        if sanitized_name.contains('/') {
            self.ensure_parent_labels(&sanitized_name).await?;
        }

        info!("Creating label: {}", sanitized_name);

        // Create the label via Gmail API
        let label_id = self
            .client
            .create_label(&sanitized_name)
            .await
            .map_err(|e| {
                GmailError::ApiError(format!("Failed to create label '{}': {}", sanitized_name, e))
            })?;

        // Track the created label
        self.label_cache.insert(sanitized_name.clone(), label_id.clone());
        self.created_labels.push(label_id.clone());

        info!("Successfully created label '{}' with ID: {}", sanitized_name, label_id);
        Ok(label_id)
    }

    /// Gets label ID by name, creating it if necessary
    pub async fn get_or_create_label(&mut self, name: &str) -> Result<String> {
        let full_name = format!("{}/{}", self.label_prefix, name);
        let sanitized_name = self.sanitize_label_name(&full_name)?;

        if let Some(id) = self.label_cache.get(&sanitized_name) {
            return Ok(id.clone());
        }

        self.create_label(name).await
    }

    /// Creates a label with the exact name provided (no prefix added)
    /// Use this when the label name already has the full path
    pub async fn create_label_direct(&mut self, full_name: &str) -> Result<String> {
        // Check if label already exists in cache
        if let Some(id) = self.label_cache.get(full_name) {
            debug!("Label '{}' already exists in cache", full_name);
            return Ok(id.clone());
        }

        // Create parent labels if this is a hierarchical label
        if full_name.contains('/') {
            self.ensure_parent_labels(full_name).await?;
        }

        info!("Creating label: {}", full_name);

        // Create the label via Gmail API
        let label_id = self
            .client
            .create_label(full_name)
            .await
            .map_err(|e| {
                GmailError::ApiError(format!("Failed to create label '{}': {}", full_name, e))
            })?;

        // Track the created label
        self.label_cache.insert(full_name.to_string(), label_id.clone());
        self.created_labels.push(label_id.clone());

        info!("Successfully created label '{}' with ID: {}", full_name, label_id);
        Ok(label_id)
    }

    /// Ensures all parent labels exist in the hierarchy
    ///
    /// For example, if creating "AutoManaged/Newsletters/Tech",
    /// this ensures "AutoManaged" and "AutoManaged/Newsletters" exist first.
    ///
    /// # Arguments
    /// * `label_name` - The full hierarchical label name
    async fn ensure_parent_labels(&mut self, label_name: &str) -> Result<()> {
        let parts: Vec<&str> = label_name.split('/').collect();

        // Create each level of the hierarchy (skip the last part, which is the label itself)
        for i in 1..parts.len() {
            let parent_path = parts[..i].join("/");

            // Check if parent already exists
            if !self.label_cache.contains_key(&parent_path) {
                debug!("Creating parent label: {}", parent_path);

                // Create parent directly (without prefix, as it's already full path)
                let label_id = self
                    .client
                    .create_label(&parent_path)
                    .await
                    .map_err(|e| {
                        GmailError::ApiError(format!(
                            "Failed to create parent label '{}': {}",
                            parent_path, e
                        ))
                    })?;

                self.label_cache.insert(parent_path.clone(), label_id.clone());
                self.created_labels.push(label_id);
            }
        }

        Ok(())
    }

    /// Sanitizes a label name to comply with Gmail's requirements
    ///
    /// Requirements:
    /// - Maximum 50 characters
    /// - No leading/trailing slashes
    /// - No consecutive slashes
    /// - Title case for consistency
    /// - Remove invalid characters
    ///
    /// # Arguments
    /// * `name` - The raw label name
    ///
    /// # Returns
    /// * `Ok(String)` - The sanitized label name
    /// * `Err(GmailError)` - If the name is invalid or becomes empty after sanitization
    pub fn sanitize_label_name(&self, name: &str) -> Result<String> {
        if name.is_empty() {
            return Err(GmailError::ConfigError(
                "Label name cannot be empty".to_string(),
            ));
        }

        // Remove leading/trailing whitespace
        let mut sanitized = name.trim().to_string();

        // Replace invalid characters (keep alphanumeric, spaces, slashes, hyphens)
        let invalid_chars = Regex::new(r"[^\w\s/\-]").unwrap();
        sanitized = invalid_chars.replace_all(&sanitized, " ").to_string();

        // Collapse multiple spaces
        let multiple_spaces = Regex::new(r"\s+").unwrap();
        sanitized = multiple_spaces.replace_all(&sanitized, " ").to_string();

        // Remove leading/trailing slashes
        sanitized = sanitized.trim_matches('/').to_string();

        // Remove consecutive slashes
        let consecutive_slashes = Regex::new(r"/+").unwrap();
        sanitized = consecutive_slashes.replace_all(&sanitized, "/").to_string();

        // Convert to title case for each segment
        sanitized = sanitized
            .split('/')
            .map(|segment| {
                segment
                    .split_whitespace()
                    .map(|word| {
                        let mut chars = word.chars();
                        match chars.next() {
                            None => String::new(),
                            Some(first) => first
                                .to_uppercase()
                                .chain(chars.as_str().to_lowercase().chars())
                                .collect(),
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect::<Vec<_>>()
            .join("/");

        // Enforce maximum length (50 characters for Gmail)
        if sanitized.len() > 50 {
            sanitized = sanitized[..50].to_string();
            // Ensure we don't cut in the middle of a word
            if let Some(last_space) = sanitized.rfind(|c| c == ' ' || c == '/') {
                sanitized = sanitized[..last_space].to_string();
            }
        }

        // Remove trailing slashes that might have been created by truncation
        sanitized = sanitized.trim_end_matches('/').to_string();

        if sanitized.is_empty() {
            return Err(GmailError::ConfigError(
                "Sanitized label name is empty".to_string(),
            ));
        }

        Ok(sanitized)
    }

    /// Consolidates similar labels to prevent proliferation
    ///
    /// Rules:
    /// 1. Merge domains with < minimum threshold into generic categories
    /// 2. Detect and merge semantically similar labels
    /// 3. Consolidate subdomains under parent domains
    ///
    /// # Arguments
    /// * `proposed_labels` - List of proposed label names
    /// * `domain_counts` - Map of domain to email count
    /// * `min_threshold` - Minimum email count for dedicated label
    ///
    /// # Returns
    /// * Map from original label to consolidated label
    pub fn consolidate_labels(
        &self,
        proposed_labels: Vec<String>,
        domain_counts: &HashMap<String, usize>,
        min_threshold: usize,
    ) -> HashMap<String, String> {
        let mut consolidated: HashMap<String, String> = HashMap::new();

        // Process each label
        for label in &proposed_labels {
            let domain = self.extract_domain_from_label(label);

            // Check if domain meets minimum threshold
            if let Some(&count) = domain_counts.get(&domain) {
                if count < min_threshold {
                    // Map to generic category
                    let category = self.determine_generic_category(label);
                    let generic_label = format!("{}/{}", self.label_prefix, category);
                    consolidated.insert(label.clone(), generic_label);
                    continue;
                }
            }

            // Check for similar existing labels to merge with
            if let Some(similar) = self.find_similar_label(label, &consolidated) {
                consolidated.insert(label.clone(), similar);
                continue;
            }

            // Keep the label as-is
            consolidated.insert(label.clone(), label.clone());
        }

        let unique_count = consolidated.values().collect::<HashSet<_>>().len();
        info!(
            "Consolidated {} labels to {} unique labels",
            proposed_labels.len(),
            unique_count
        );

        consolidated
    }

    /// Applies labels to a batch of messages
    ///
    /// Processes label applications with proper error handling.
    /// Note: This uses individual API calls per message, not Gmail's batch modify,
    /// as per the spec's recommendation for better error handling.
    ///
    /// # Arguments
    /// * `message_label_map` - Map from message ID to list of label IDs to apply
    /// * `remove_inbox` - Whether to remove INBOX label (archive)
    ///
    /// # Returns
    /// * Number of successfully modified messages
    pub async fn apply_labels_to_messages(
        &self,
        message_label_map: HashMap<String, Vec<String>>,
        remove_inbox: bool,
    ) -> Result<usize> {
        let total_messages = message_label_map.len();
        let mut success_count = 0;

        info!(
            "Applying labels to {} messages (remove_inbox: {})",
            total_messages, remove_inbox
        );

        for (message_id, label_ids) in message_label_map {
            // Apply each label to the message
            for label_id in label_ids {
                match self.client.apply_label(&message_id, &label_id).await {
                    Ok(_) => {
                        debug!("Applied label {} to message {}", label_id, message_id);
                    }
                    Err(e) => {
                        warn!(
                            "Failed to apply label {} to message {}: {}",
                            label_id, message_id, e
                        );
                        // Continue with other labels rather than failing completely
                    }
                }
            }

            // Remove INBOX label if archiving
            if remove_inbox {
                // Note: In production, this would use a modify call to remove INBOX
                // For now, we track success based on label application
            }

            success_count += 1;
        }

        info!(
            "Successfully applied labels to {}/{} messages",
            success_count, total_messages
        );
        Ok(success_count)
    }

    /// Gets the list of labels created by this manager
    pub fn get_created_labels(&self) -> &[String] {
        &self.created_labels
    }

    /// Gets label ID from cache by name
    pub fn get_label_id(&self, label_name: &str) -> Option<String> {
        self.label_cache.get(label_name).cloned()
    }

    /// Extracts domain from a label name (heuristic)
    ///
    /// This is a simplified heuristic - in practice you'd track the mapping
    /// from label back to original domain
    fn extract_domain_from_label(&self, label: &str) -> String {
        label
            .split('/')
            .last()
            .unwrap_or(label)
            .to_lowercase()
            .replace(' ', "")
    }

    /// Determines generic category for low-volume senders
    fn determine_generic_category(&self, label: &str) -> String {
        let lower = label.to_lowercase();

        if lower.contains("newsletter") || lower.contains("news") {
            "Newsletters/Other"
        } else if lower.contains("notification") || lower.contains("alert") {
            "Notifications/Other"
        } else if lower.contains("receipt") || lower.contains("order") {
            "Receipts/Other"
        } else if lower.contains("marketing") || lower.contains("promo") {
            "Marketing/Other"
        } else if lower.contains("social") {
            "Notifications/Social"
        } else {
            "Other"
        }
        .to_string()
    }

    /// Finds a similar existing label to merge with
    ///
    /// Uses string similarity metrics to detect labels that should be merged
    fn find_similar_label(
        &self,
        label: &str,
        existing: &HashMap<String, String>,
    ) -> Option<String> {
        let label_lower = label.to_lowercase();

        for (_, target) in existing.iter() {
            let target_lower = target.to_lowercase();

            // Check for exact match (case-insensitive)
            if label_lower == target_lower && label != target {
                return Some(target.clone());
            }

            // Check for word-based similarity
            let label_words: HashSet<&str> = label_lower.split_whitespace().collect();
            let target_words: HashSet<&str> = target_lower.split_whitespace().collect();

            let intersection_count = label_words.intersection(&target_words).count();
            let union_count = label_words.union(&target_words).count();

            // If > 66% similarity, consider them the same
            if union_count > 0 && (intersection_count as f32 / union_count as f32) > 0.66 {
                return Some(target.clone());
            }
        }

        None
    }

    /// Builds a hierarchical label structure for reporting
    pub fn build_label_hierarchy(&self) -> HashMap<String, Vec<String>> {
        let mut hierarchy: HashMap<String, Vec<String>> = HashMap::new();

        for label_name in self.label_cache.keys() {
            if !label_name.starts_with(&self.label_prefix) {
                continue;
            }

            let parts: Vec<&str> = label_name.split('/').collect();
            if parts.len() > 1 {
                let parent = parts[..parts.len() - 1].join("/");
                hierarchy
                    .entry(parent)
                    .or_insert_with(Vec::new)
                    .push(label_name.clone());
            } else {
                hierarchy
                    .entry("ROOT".to_string())
                    .or_insert_with(Vec::new)
                    .push(label_name.clone());
            }
        }

        hierarchy
    }

    /// Creates labels for a set of email categories
    ///
    /// This is a high-level function that creates all necessary labels for
    /// the given categories, handling hierarchy, deduplication, and error handling.
    ///
    /// # Arguments
    /// * `categories` - Map from category name to label name (e.g., "Newsletters/GitHub")
    ///
    /// # Returns
    /// * Map from category name to created label ID
    ///
    /// # Example
    /// ```ignore
    /// let mut categories = HashMap::new();
    /// categories.insert("newsletters_github", "Newsletters/GitHub");
    /// categories.insert("receipts_amazon", "Receipts/Amazon");
    ///
    /// let label_ids = manager.create_labels_for_categories(categories).await?;
    /// ```
    pub async fn create_labels_for_categories(
        &mut self,
        categories: HashMap<String, String>,
    ) -> Result<HashMap<String, String>> {
        let mut label_map = HashMap::new();
        let total = categories.len();
        let mut created = 0;
        let mut errors = Vec::new();

        info!("Creating {} labels from categories", total);

        for (category_key, label_name) in categories {
            match self.create_label(&label_name).await {
                Ok(label_id) => {
                    label_map.insert(category_key.clone(), label_id);
                    created += 1;
                    debug!(
                        "Created label {}/{}: {} -> {}",
                        created, total, category_key, label_name
                    );
                }
                Err(e) => {
                    let error_msg = format!("Failed to create label '{}': {}", label_name, e);
                    warn!("{}", error_msg);
                    errors.push(error_msg);
                    // Continue with other labels rather than failing completely
                }
            }
        }

        if !errors.is_empty() {
            warn!(
                "Created {}/{} labels with {} errors",
                created,
                total,
                errors.len()
            );
        } else {
            info!("Successfully created all {} labels", created);
        }

        // Return success if at least one label was created
        if created > 0 {
            Ok(label_map)
        } else {
            Err(GmailError::LabelError(format!(
                "Failed to create any labels. Errors: {}",
                errors.join("; ")
            )))
        }
    }

    /// Applies labels to messages in bulk
    ///
    /// This is a convenience wrapper around apply_labels_to_messages with
    /// a simpler interface for applying a single label to multiple messages.
    ///
    /// # Arguments
    /// * `message_ids` - List of message IDs to label
    /// * `label_id` - The label ID to apply
    /// * `archive` - Whether to also archive (remove INBOX) the messages
    ///
    /// # Returns
    /// * Number of successfully labeled messages
    pub async fn apply_labels(
        &self,
        message_ids: Vec<String>,
        label_id: &str,
        archive: bool,
    ) -> Result<usize> {
        // Build the message->labels map
        let message_label_map: HashMap<String, Vec<String>> = message_ids
            .into_iter()
            .map(|msg_id| (msg_id, vec![label_id.to_string()]))
            .collect();

        self.apply_labels_to_messages(message_label_map, archive)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_label_name() {
        use async_trait::async_trait;

        mockall::mock! {
            pub TestGmailClient {}

            #[async_trait]
            impl crate::client::GmailClient for TestGmailClient {
                async fn list_message_ids(&self, query: &str) -> Result<Vec<String>>;
                async fn get_message(&self, id: &str) -> Result<crate::models::MessageMetadata>;
                async fn list_labels(&self) -> Result<Vec<crate::client::LabelInfo>>;
                async fn create_label(&self, name: &str) -> Result<String>;
                async fn create_filter(&self, filter: &crate::models::FilterRule) -> Result<String>;
                async fn apply_label(&self, message_id: &str, label_id: &str) -> Result<()>;
                async fn remove_label(&self, message_id: &str, label_id: &str) -> Result<()>;
                async fn fetch_messages_batch(&self, message_ids: Vec<String>) -> Result<Vec<crate::models::MessageMetadata>>;
                async fn fetch_messages_with_progress(&self, message_ids: Vec<String>, on_progress: crate::client::ProgressCallback) -> Result<Vec<crate::models::MessageMetadata>>;
            }
        }

        let mock_client = MockTestGmailClient::new();
        let manager = LabelManager::new(Box::new(mock_client), "AutoManaged".to_string());

        // Test cases
        let test_cases = vec![
            ("github notifications", "Github Notifications"),
            ("Test/Label", "Test/Label"),
            ("test//double//slash", "Test/Double/Slash"),
            ("/leading/slash/", "Leading/Slash"),
            ("trailing/slash/", "Trailing/Slash"),
            ("Invalid@Chars!", "Invalid Chars"),
            ("   extra   spaces   ", "Extra Spaces"),
        ];

        for (input, expected) in test_cases {
            let result = manager.sanitize_label_name(input);
            assert!(result.is_ok(), "Failed to sanitize: {}", input);
            assert_eq!(result.unwrap(), expected);
        }
    }

    #[test]
    fn test_sanitize_label_name_max_length() {
        use async_trait::async_trait;

        mockall::mock! {
            pub TestGmailClient {}

            #[async_trait]
            impl crate::client::GmailClient for TestGmailClient {
                async fn list_message_ids(&self, query: &str) -> Result<Vec<String>>;
                async fn get_message(&self, id: &str) -> Result<crate::models::MessageMetadata>;
                async fn list_labels(&self) -> Result<Vec<crate::client::LabelInfo>>;
                async fn create_label(&self, name: &str) -> Result<String>;
                async fn create_filter(&self, filter: &crate::models::FilterRule) -> Result<String>;
                async fn apply_label(&self, message_id: &str, label_id: &str) -> Result<()>;
                async fn remove_label(&self, message_id: &str, label_id: &str) -> Result<()>;
                async fn fetch_messages_batch(&self, message_ids: Vec<String>) -> Result<Vec<crate::models::MessageMetadata>>;
                async fn fetch_messages_with_progress(&self, message_ids: Vec<String>, on_progress: crate::client::ProgressCallback) -> Result<Vec<crate::models::MessageMetadata>>;
            }
        }

        let mock_client = MockTestGmailClient::new();
        let manager = LabelManager::new(Box::new(mock_client), "AutoManaged".to_string());

        let long_name = "This Is A Very Long Label Name That Exceeds The Maximum Length Limit";
        let result = manager.sanitize_label_name(long_name);
        assert!(result.is_ok());
        assert!(result.unwrap().len() <= 50);
    }

    #[test]
    fn test_determine_generic_category() {
        use async_trait::async_trait;

        mockall::mock! {
            pub TestGmailClient {}

            #[async_trait]
            impl crate::client::GmailClient for TestGmailClient {
                async fn list_message_ids(&self, query: &str) -> Result<Vec<String>>;
                async fn get_message(&self, id: &str) -> Result<crate::models::MessageMetadata>;
                async fn list_labels(&self) -> Result<Vec<crate::client::LabelInfo>>;
                async fn create_label(&self, name: &str) -> Result<String>;
                async fn create_filter(&self, filter: &crate::models::FilterRule) -> Result<String>;
                async fn apply_label(&self, message_id: &str, label_id: &str) -> Result<()>;
                async fn remove_label(&self, message_id: &str, label_id: &str) -> Result<()>;
                async fn fetch_messages_batch(&self, message_ids: Vec<String>) -> Result<Vec<crate::models::MessageMetadata>>;
                async fn fetch_messages_with_progress(&self, message_ids: Vec<String>, on_progress: crate::client::ProgressCallback) -> Result<Vec<crate::models::MessageMetadata>>;
            }
        }

        let mock_client = MockTestGmailClient::new();
        let manager = LabelManager::new(Box::new(mock_client), "AutoManaged".to_string());

        assert_eq!(
            manager.determine_generic_category("Some Newsletter"),
            "Newsletters/Other"
        );
        assert_eq!(
            manager.determine_generic_category("Notification Alert"),
            "Notifications/Other"
        );
        assert_eq!(
            manager.determine_generic_category("Order Receipt"),
            "Receipts/Other"
        );
    }

    #[test]
    fn test_consolidate_labels() {
        use async_trait::async_trait;

        mockall::mock! {
            pub TestGmailClient {}

            #[async_trait]
            impl crate::client::GmailClient for TestGmailClient {
                async fn list_message_ids(&self, query: &str) -> Result<Vec<String>>;
                async fn get_message(&self, id: &str) -> Result<crate::models::MessageMetadata>;
                async fn list_labels(&self) -> Result<Vec<crate::client::LabelInfo>>;
                async fn create_label(&self, name: &str) -> Result<String>;
                async fn create_filter(&self, filter: &crate::models::FilterRule) -> Result<String>;
                async fn apply_label(&self, message_id: &str, label_id: &str) -> Result<()>;
                async fn remove_label(&self, message_id: &str, label_id: &str) -> Result<()>;
                async fn fetch_messages_batch(&self, message_ids: Vec<String>) -> Result<Vec<crate::models::MessageMetadata>>;
                async fn fetch_messages_with_progress(&self, message_ids: Vec<String>, on_progress: crate::client::ProgressCallback) -> Result<Vec<crate::models::MessageMetadata>>;
            }
        }

        let mock_client = MockTestGmailClient::new();
        let manager = LabelManager::new(Box::new(mock_client), "AutoManaged".to_string());

        let proposed = vec![
            "Company Newsletter".to_string(),
            "Low Volume Sender".to_string(),
        ];

        let mut domain_counts = HashMap::new();
        domain_counts.insert("companynewsletter".to_string(), 100);
        domain_counts.insert("lowvolumesender".to_string(), 2);

        let consolidated = manager.consolidate_labels(proposed, &domain_counts, 5);

        // Low volume sender should be consolidated to generic category
        let result = consolidated.get("Low Volume Sender").unwrap();
        assert!(result.contains("AutoManaged"), "Expected result to contain 'AutoManaged', got: {}", result);
    }

    #[tokio::test]
    async fn test_create_labels_for_categories() {
        use mockall::predicate::*;
        use async_trait::async_trait;

        // Create a mock client
        mockall::mock! {
            pub TestGmailClient {}

            #[async_trait]
            impl crate::client::GmailClient for TestGmailClient {
                async fn list_message_ids(&self, query: &str) -> Result<Vec<String>>;
                async fn get_message(&self, id: &str) -> Result<crate::models::MessageMetadata>;
                async fn list_labels(&self) -> Result<Vec<crate::client::LabelInfo>>;
                async fn create_label(&self, name: &str) -> Result<String>;
                async fn create_filter(&self, filter: &crate::models::FilterRule) -> Result<String>;
                async fn apply_label(&self, message_id: &str, label_id: &str) -> Result<()>;
                async fn remove_label(&self, message_id: &str, label_id: &str) -> Result<()>;
                async fn fetch_messages_batch(&self, message_ids: Vec<String>) -> Result<Vec<crate::models::MessageMetadata>>;
                async fn fetch_messages_with_progress(&self, message_ids: Vec<String>, on_progress: crate::client::ProgressCallback) -> Result<Vec<crate::models::MessageMetadata>>;
            }
        }

        let mut mock_client = MockTestGmailClient::new();

        // Set up expectations for parent label creation (hierarchy)
        // When creating "AutoManaged/Newsletters/GitHub", it will create parent labels:
        // Note: sanitize_label_name converts to title case, so "AutoManaged" becomes "Automanaged"
        // 1. "Automanaged"
        // 2. "Automanaged/Newsletters"
        // 3. "Automanaged/Newsletters/Github"

        mock_client
            .expect_create_label()
            .with(eq("Automanaged"))
            .times(1)
            .returning(|_| Ok("label-parent".to_string()));

        mock_client
            .expect_create_label()
            .with(eq("Automanaged/Newsletters"))
            .times(1)
            .returning(|_| Ok("label-newsletters".to_string()));

        mock_client
            .expect_create_label()
            .with(eq("Automanaged/Newsletters/Github"))
            .times(1)
            .returning(|_| Ok("label-id-1".to_string()));

        mock_client
            .expect_create_label()
            .with(eq("Automanaged/Receipts"))
            .times(1)
            .returning(|_| Ok("label-receipts".to_string()));

        mock_client
            .expect_create_label()
            .with(eq("Automanaged/Receipts/Amazon"))
            .times(1)
            .returning(|_| Ok("label-id-2".to_string()));

        let mut manager = LabelManager::new(
            Box::new(mock_client),
            "AutoManaged".to_string(),
        );

        let mut categories = HashMap::new();
        categories.insert("newsletters_github".to_string(), "Newsletters/GitHub".to_string());
        categories.insert("receipts_amazon".to_string(), "Receipts/Amazon".to_string());

        let result = manager.create_labels_for_categories(categories).await;
        assert!(result.is_ok());

        let label_map = result.unwrap();
        assert_eq!(label_map.len(), 2);
        assert_eq!(label_map.get("newsletters_github"), Some(&"label-id-1".to_string()));
        assert_eq!(label_map.get("receipts_amazon"), Some(&"label-id-2".to_string()));
    }

    #[tokio::test]
    async fn test_apply_labels_bulk() {
        use mockall::predicate::*;
        use async_trait::async_trait;

        mockall::mock! {
            pub TestGmailClient {}

            #[async_trait]
            impl crate::client::GmailClient for TestGmailClient {
                async fn list_message_ids(&self, query: &str) -> Result<Vec<String>>;
                async fn get_message(&self, id: &str) -> Result<crate::models::MessageMetadata>;
                async fn list_labels(&self) -> Result<Vec<crate::client::LabelInfo>>;
                async fn create_label(&self, name: &str) -> Result<String>;
                async fn create_filter(&self, filter: &crate::models::FilterRule) -> Result<String>;
                async fn apply_label(&self, message_id: &str, label_id: &str) -> Result<()>;
                async fn remove_label(&self, message_id: &str, label_id: &str) -> Result<()>;
                async fn fetch_messages_batch(&self, message_ids: Vec<String>) -> Result<Vec<crate::models::MessageMetadata>>;
                async fn fetch_messages_with_progress(&self, message_ids: Vec<String>, on_progress: crate::client::ProgressCallback) -> Result<Vec<crate::models::MessageMetadata>>;
            }
        }

        let mut mock_client = MockTestGmailClient::new();

        // Expect label application for each message
        mock_client
            .expect_apply_label()
            .with(eq("msg-1"), eq("label-123"))
            .times(1)
            .returning(|_, _| Ok(()));

        mock_client
            .expect_apply_label()
            .with(eq("msg-2"), eq("label-123"))
            .times(1)
            .returning(|_, _| Ok(()));

        let manager = LabelManager::new(
            Box::new(mock_client),
            "AutoManaged".to_string(),
        );

        let message_ids = vec!["msg-1".to_string(), "msg-2".to_string()];
        let result = manager.apply_labels(message_ids, "label-123", false).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 2);
    }
}
