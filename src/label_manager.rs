//! Label management and creation with hierarchy support and consolidation logic
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
    /// Note: Cache keys are stored lowercase for case-insensitive lookups
    pub async fn load_existing_labels(&mut self) -> Result<usize> {
        let labels = self.client.list_labels().await?;
        let count = labels.len();

        for label in labels {
            // Store with lowercase key for case-insensitive lookup
            self.label_cache.insert(label.name.to_lowercase(), label.id);
        }

        info!("Loaded {} existing labels into cache", count);
        Ok(count)
    }

    /// Case-insensitive cache lookup helper
    fn cache_get(&self, name: &str) -> Option<&String> {
        self.label_cache.get(&name.to_lowercase())
    }

    /// Case-insensitive cache contains check
    fn cache_contains(&self, name: &str) -> bool {
        self.label_cache.contains_key(&name.to_lowercase())
    }

    /// Insert into cache with lowercase key
    fn cache_insert(&mut self, name: String, id: String) {
        self.label_cache.insert(name.to_lowercase(), id);
    }

    /// Returns the list of existing labels that match the proposed labels
    /// Note: proposed labels should already include the full path (e.g., "auto/receipts/amazon")
    pub fn find_existing_labels(&self, proposed: &[String]) -> Vec<String> {
        proposed
            .iter()
            .filter(|name| {
                // Labels already have full path, just sanitize for case normalization
                let sanitized = self.sanitize_label_name(name).unwrap_or_default();
                self.cache_contains(&sanitized)
            })
            .cloned()
            .collect()
    }

    /// Returns labels that would be newly created (don't exist yet)
    /// Note: proposed labels should already include the full path (e.g., "auto/receipts/amazon")
    pub fn find_new_labels(&self, proposed: &[String]) -> Vec<String> {
        proposed
            .iter()
            .filter(|name| {
                // Labels already have full path, just sanitize for case normalization
                let sanitized = self.sanitize_label_name(name).unwrap_or_default();
                !self.cache_contains(&sanitized)
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

        // Check if label already exists in cache (case-insensitive)
        if let Some(id) = self.cache_get(&sanitized_name) {
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
                GmailError::ApiError(format!(
                    "Failed to create label '{}': {}",
                    sanitized_name, e
                ))
            })?;

        // Track the created label (cache uses lowercase key)
        self.cache_insert(sanitized_name.clone(), label_id.clone());
        self.created_labels.push(label_id.clone());

        info!(
            "Successfully created label '{}' with ID: {}",
            sanitized_name, label_id
        );
        Ok(label_id)
    }

    /// Gets label ID by name, creating it if necessary
    pub async fn get_or_create_label(&mut self, name: &str) -> Result<String> {
        let full_name = format!("{}/{}", self.label_prefix, name);
        let sanitized_name = self.sanitize_label_name(&full_name)?;

        if let Some(id) = self.cache_get(&sanitized_name) {
            return Ok(id.clone());
        }

        self.create_label(name).await
    }

    /// Creates a label with the exact name provided (no prefix added)
    /// Use this when the label name already has the full path
    pub async fn create_label_direct(&mut self, full_name: &str) -> Result<String> {
        // Check if label already exists in cache (case-insensitive)
        if let Some(id) = self.cache_get(full_name) {
            debug!("Label '{}' already exists in cache", full_name);
            return Ok(id.clone());
        }

        // Create parent labels if this is a hierarchical label
        if full_name.contains('/') {
            self.ensure_parent_labels(full_name).await?;
        }

        info!("Creating label: {}", full_name);

        // Create the label via Gmail API
        let label_id = self.client.create_label(full_name).await.map_err(|e| {
            GmailError::ApiError(format!("Failed to create label '{}': {}", full_name, e))
        })?;

        // Track the created label (cache uses lowercase key)
        self.cache_insert(full_name.to_string(), label_id.clone());
        self.created_labels.push(label_id.clone());

        info!(
            "Successfully created label '{}' with ID: {}",
            full_name, label_id
        );
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

            // Check if parent already exists (case-insensitive)
            if !self.cache_contains(&parent_path) {
                debug!("Creating parent label: {}", parent_path);

                // Create parent directly (without prefix, as it's already full path)
                let label_id = self.client.create_label(&parent_path).await.map_err(|e| {
                    GmailError::ApiError(format!(
                        "Failed to create parent label '{}': {}",
                        parent_path, e
                    ))
                })?;

                self.cache_insert(parent_path.clone(), label_id.clone());
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
            if let Some(last_space) = sanitized.rfind([' ', '/']) {
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

    /// Applies labels to a batch of messages using batch API calls
    ///
    /// Groups messages by label for efficient batch operations.
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

        info!(
            "Applying labels to {} messages (remove_inbox: {})",
            total_messages, remove_inbox
        );

        // Invert the map: label_id -> Vec<message_ids> for batch operations
        let mut label_to_messages: HashMap<String, Vec<String>> = HashMap::new();
        for (message_id, label_ids) in &message_label_map {
            for label_id in label_ids {
                label_to_messages
                    .entry(label_id.clone())
                    .or_default()
                    .push(message_id.clone());
            }
        }

        let mut success_count = 0;

        // Batch apply each label to its messages
        for (label_id, message_ids) in label_to_messages {
            let remove_labels = if remove_inbox {
                vec!["INBOX".to_string()]
            } else {
                vec![]
            };

            match self
                .client
                .batch_modify_labels(
                    &message_ids,
                    std::slice::from_ref(&label_id),
                    &remove_labels,
                )
                .await
            {
                Ok(count) => {
                    debug!("Batch applied label {} to {} messages", label_id, count);
                    success_count += count;
                }
                Err(e) => {
                    warn!("Failed to batch apply label {}: {}", label_id, e);
                }
            }
        }

        // Deduplicate success count (a message might have multiple labels)
        let actual_messages = total_messages.min(success_count);

        info!(
            "Successfully applied labels to ~{} messages",
            actual_messages
        );
        Ok(actual_messages)
    }

    /// Gets the list of labels created by this manager
    pub fn get_created_labels(&self) -> &[String] {
        &self.created_labels
    }

    /// Gets label ID from cache by name (case-insensitive)
    pub fn get_label_id(&self, label_name: &str) -> Option<String> {
        self.cache_get(label_name).cloned()
    }

    /// Extracts domain from a label name (heuristic)
    ///
    /// This is a simplified heuristic - in practice you'd track the mapping
    /// from label back to original domain
    fn extract_domain_from_label(&self, label: &str) -> String {
        label
            .split('/')
            .next_back()
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
        let prefix_lower = self.label_prefix.to_lowercase();

        for label_name in self.label_cache.keys() {
            // Cache keys are lowercase, so compare with lowercase prefix
            if !label_name.starts_with(&prefix_lower) {
                continue;
            }

            let parts: Vec<&str> = label_name.split('/').collect();
            if parts.len() > 1 {
                let parent = parts[..parts.len() - 1].join("/");
                hierarchy
                    .entry(parent)
                    .or_default()
                    .push(label_name.clone());
            } else {
                hierarchy
                    .entry("ROOT".to_string())
                    .or_default()
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

    /// Find labels under the auto-managed prefix that are not used by any filter
    /// Returns Vec of (label_id, label_name) pairs
    pub fn find_orphaned_labels(
        &self,
        existing_filters: &[crate::client::ExistingFilterInfo],
        prefix: &str,
    ) -> Vec<(String, String)> {
        let prefix_lower = prefix.to_lowercase();

        // Collect all label IDs used by filters
        let used_label_ids: std::collections::HashSet<_> = existing_filters
            .iter()
            .flat_map(|f| &f.add_label_ids)
            .collect();

        // Build reverse lookup from label_id to label_name
        let id_to_name: HashMap<&String, &String> = self.label_cache
            .iter()
            .map(|(name, id)| (id, name))
            .collect();

        // Collect all required label names (directly used labels + their parent paths)
        let mut required_label_names: HashSet<String> = HashSet::new();

        for used_id in used_label_ids.iter() {
            if let Some(label_name) = id_to_name.get(used_id) {
                // Add the label itself
                required_label_names.insert(label_name.to_lowercase());

                // Add all parent paths
                // For "automanaged/receipts/amazon", add "automanaged/receipts" and "automanaged"
                let parts: Vec<&str> = label_name.split('/').collect();
                for i in 1..parts.len() {
                    let parent_path = parts[..i].join("/");
                    required_label_names.insert(parent_path.to_lowercase());
                }
            }
        }

        // Find labels starting with prefix that are not required
        self.label_cache
            .iter()
            .filter(|(name, _id)| {
                let name_lower = name.to_lowercase();
                name_lower.starts_with(&prefix_lower)
                    && !required_label_names.contains(&name_lower)
            })
            .map(|(name, id)| (id.clone(), name.clone()))
            .collect()
    }

    /// Ensure all parent hierarchy labels exist for used labels
    /// This repairs any missing parent labels that are needed for Gmail's nested display
    /// Returns Vec of (label_id, label_name) pairs of labels that were created
    pub async fn ensure_label_hierarchy(
        &mut self,
        existing_filters: &[crate::client::ExistingFilterInfo],
        prefix: &str,
    ) -> crate::error::Result<Vec<(String, String)>> {
        let prefix_lower = prefix.to_lowercase();

        // Build reverse lookup from label_id to label_name
        let id_to_name: HashMap<&String, &String> = self
            .label_cache
            .iter()
            .map(|(name, id)| (id, name))
            .collect();

        // Collect all label IDs used by filters
        let used_label_ids: HashSet<&String> = existing_filters
            .iter()
            .flat_map(|f| &f.add_label_ids)
            .collect();
        let used_label_count = used_label_ids.len();

        // Collect all required parent paths
        let mut required_parents: HashSet<String> = HashSet::new();

        for used_id in used_label_ids {
            if let Some(label_name) = id_to_name.get(used_id) {
                let label_name_lower = label_name.to_lowercase();

                // Only process labels that start with the prefix
                if !label_name_lower.starts_with(&prefix_lower) {
                    continue;
                }

                // Split by '/' and check if more than 1 level deep under prefix
                let parts: Vec<&str> = label_name.split('/').collect();

                // For each parent path (except the label itself), check if it exists
                for i in 1..parts.len() {
                    let parent_path = parts[..i].join("/");
                    let parent_path_lower = parent_path.to_lowercase();

                    // Only consider parents that start with the prefix
                    if parent_path_lower.starts_with(&prefix_lower) {
                        required_parents.insert(parent_path);
                    }
                }
            }
        }

        // Sort parents by path length (shortest first) to ensure parents exist before children
        let mut parents_sorted: Vec<String> = required_parents.into_iter().collect();
        parents_sorted.sort_by_key(|p| p.matches('/').count());

        info!(
            "Hierarchy check: {} labels used by filters, {} unique parent paths to verify",
            used_label_count,
            parents_sorted.len()
        );

        // Create missing parent labels
        let mut created_labels: Vec<(String, String)> = Vec::new();

        for parent_path in &parents_sorted {
            // Check if parent already exists in cache (case-insensitive)
            if !self.cache_contains(parent_path) {
                info!("Creating missing parent label: {}", parent_path);

                // Remove the prefix from the parent path since get_or_create_label adds it
                let parent_without_prefix = if parent_path.to_lowercase().starts_with(&prefix_lower) {
                    let prefix_len = prefix.len();
                    if parent_path.len() > prefix_len && parent_path.chars().nth(prefix_len) == Some('/') {
                        parent_path[prefix_len + 1..].to_string()
                    } else {
                        parent_path.clone()
                    }
                } else {
                    parent_path.clone()
                };

                // Create the parent label
                match self.get_or_create_label(&parent_without_prefix).await {
                    Ok(label_id) => {
                        // get_or_create_label already updates the cache
                        created_labels.push((label_id, parent_path.clone()));
                        info!("Created parent label: {}", parent_path);
                    }
                    Err(e) => {
                        warn!("Failed to create parent label '{}': {}", parent_path, e);
                    }
                }
            }
        }

        Ok(created_labels)
    }

    /// Delete orphaned labels - returns count of successfully deleted labels
    pub async fn cleanup_orphaned_labels(
        &mut self,
        orphaned: &[(String, String)],
    ) -> crate::error::Result<usize> {
        let mut deleted_count = 0;

        // Sort by name length descending to delete children before parents
        let mut sorted: Vec<_> = orphaned.to_vec();
        sorted.sort_by(|a, b| b.1.len().cmp(&a.1.len()));

        for (label_id, label_name) in sorted {
            match self.client.delete_label(&label_id).await {
                Ok(()) => {
                    info!("Deleted orphaned label: {}", label_name);
                    // Remove from cache
                    self.label_cache.retain(|_, id| id != &label_id);
                    deleted_count += 1;
                }
                Err(e) => {
                    warn!("Failed to delete label {}: {}", label_name, e);
                }
            }
        }

        Ok(deleted_count)
    }

    /// Remove a label from all messages that have it
    /// Returns the number of messages modified
    pub async fn remove_label_from_all_messages(
        &self,
        label_id: &str,
    ) -> crate::error::Result<usize> {
        // Search for messages with this label
        let query = format!("label:{}", label_id);
        let message_ids = self.client.list_message_ids(&query).await?;

        if message_ids.is_empty() {
            return Ok(0);
        }

        info!("Removing label from {} messages", message_ids.len());

        // Batch remove the label (Gmail allows up to 1000 per call)
        let labels_to_remove = vec![label_id.to_string()];
        let labels_to_add: Vec<String> = vec![];

        for chunk in message_ids.chunks(1000) {
            self.client
                .batch_modify_labels(chunk, &labels_to_add, &labels_to_remove)
                .await?;
        }

        Ok(message_ids.len())
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
                async fn delete_label(&self, label_id: &str) -> Result<()>;
                async fn create_filter(&self, filter: &crate::models::FilterRule) -> Result<String>;
                async fn list_filters(&self) -> Result<Vec<crate::client::ExistingFilterInfo>>;
                async fn delete_filter(&self, filter_id: &str) -> Result<()>;
                async fn update_filter(&self, filter_id: &str, filter: &crate::models::FilterRule) -> Result<String>;
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
                async fn delete_label(&self, label_id: &str) -> Result<()>;
                async fn create_filter(&self, filter: &crate::models::FilterRule) -> Result<String>;
                async fn list_filters(&self) -> Result<Vec<crate::client::ExistingFilterInfo>>;
                async fn delete_filter(&self, filter_id: &str) -> Result<()>;
                async fn update_filter(&self, filter_id: &str, filter: &crate::models::FilterRule) -> Result<String>;
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
                async fn delete_label(&self, label_id: &str) -> Result<()>;
                async fn create_filter(&self, filter: &crate::models::FilterRule) -> Result<String>;
                async fn list_filters(&self) -> Result<Vec<crate::client::ExistingFilterInfo>>;
                async fn delete_filter(&self, filter_id: &str) -> Result<()>;
                async fn update_filter(&self, filter_id: &str, filter: &crate::models::FilterRule) -> Result<String>;
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
                async fn delete_label(&self, label_id: &str) -> Result<()>;
                async fn create_filter(&self, filter: &crate::models::FilterRule) -> Result<String>;
                async fn list_filters(&self) -> Result<Vec<crate::client::ExistingFilterInfo>>;
                async fn delete_filter(&self, filter_id: &str) -> Result<()>;
                async fn update_filter(&self, filter_id: &str, filter: &crate::models::FilterRule) -> Result<String>;
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
        assert!(
            result.contains("AutoManaged"),
            "Expected result to contain 'AutoManaged', got: {}",
            result
        );
    }

    #[tokio::test]
    async fn test_create_labels_for_categories() {
        use async_trait::async_trait;
        use mockall::predicate::*;

        // Create a mock client
        mockall::mock! {
            pub TestGmailClient {}

            #[async_trait]
            impl crate::client::GmailClient for TestGmailClient {
                async fn list_message_ids(&self, query: &str) -> Result<Vec<String>>;
                async fn get_message(&self, id: &str) -> Result<crate::models::MessageMetadata>;
                async fn list_labels(&self) -> Result<Vec<crate::client::LabelInfo>>;
                async fn create_label(&self, name: &str) -> Result<String>;
                async fn delete_label(&self, label_id: &str) -> Result<()>;
                async fn create_filter(&self, filter: &crate::models::FilterRule) -> Result<String>;
                async fn list_filters(&self) -> Result<Vec<crate::client::ExistingFilterInfo>>;
                async fn delete_filter(&self, filter_id: &str) -> Result<()>;
                async fn update_filter(&self, filter_id: &str, filter: &crate::models::FilterRule) -> Result<String>;
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

        let mut manager = LabelManager::new(Box::new(mock_client), "AutoManaged".to_string());

        let mut categories = HashMap::new();
        categories.insert(
            "newsletters_github".to_string(),
            "Newsletters/GitHub".to_string(),
        );
        categories.insert("receipts_amazon".to_string(), "Receipts/Amazon".to_string());

        let result = manager.create_labels_for_categories(categories).await;
        assert!(result.is_ok());

        let label_map = result.unwrap();
        assert_eq!(label_map.len(), 2);
        assert_eq!(
            label_map.get("newsletters_github"),
            Some(&"label-id-1".to_string())
        );
        assert_eq!(
            label_map.get("receipts_amazon"),
            Some(&"label-id-2".to_string())
        );
    }

    #[tokio::test]
    async fn test_apply_labels_bulk() {
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
                async fn create_filter(&self, filter: &crate::models::FilterRule) -> Result<String>;
                async fn list_filters(&self) -> Result<Vec<crate::client::ExistingFilterInfo>>;
                async fn delete_filter(&self, filter_id: &str) -> Result<()>;
                async fn update_filter(&self, filter_id: &str, filter: &crate::models::FilterRule) -> Result<String>;
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

        // Expect batch_modify_labels to be called once with both messages
        mock_client
            .expect_batch_modify_labels()
            .withf(|msg_ids, add_labels, remove_labels| {
                msg_ids.len() == 2
                    && add_labels == &["label-123".to_string()]
                    && remove_labels.is_empty()
            })
            .times(1)
            .returning(|msg_ids, _, _| Ok(msg_ids.len()));

        let manager = LabelManager::new(Box::new(mock_client), "AutoManaged".to_string());

        let message_ids = vec!["msg-1".to_string(), "msg-2".to_string()];
        let result = manager.apply_labels(message_ids, "label-123", false).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 2);
    }

    #[test]
    fn test_find_orphaned_labels_preserves_hierarchy_parents() {
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
                async fn create_filter(&self, filter: &crate::models::FilterRule) -> Result<String>;
                async fn list_filters(&self) -> Result<Vec<crate::client::ExistingFilterInfo>>;
                async fn delete_filter(&self, filter_id: &str) -> Result<()>;
                async fn update_filter(&self, filter_id: &str, filter: &crate::models::FilterRule) -> Result<String>;
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
        let mut manager = LabelManager::new(Box::new(mock_client), "automanaged".to_string());

        // Setup: Create a label cache with a hierarchy
        manager.cache_insert("automanaged".to_string(), "label-id-root".to_string());
        manager.cache_insert("automanaged/receipts".to_string(), "label-id-receipts".to_string());
        manager.cache_insert("automanaged/receipts/amazon".to_string(), "label-id-amazon".to_string());

        // Create an ExistingFilterInfo that uses only the deepest label
        let filter = crate::client::ExistingFilterInfo {
            id: "filter-1".to_string(),
            query: Some("from:amazon.com".to_string()),
            from: None,
            to: None,
            subject: None,
            add_label_ids: vec!["label-id-amazon".to_string()],
            remove_label_ids: vec![],
        };

        let filters = vec![filter];

        // Call find_orphaned_labels
        let orphaned = manager.find_orphaned_labels(&filters, "automanaged");

        // Assert: Neither automanaged nor automanaged/receipts is in the orphaned list
        // They are hierarchy parents of automanaged/receipts/amazon which is used
        let orphaned_names: Vec<String> = orphaned.iter().map(|(_, name)| name.clone()).collect();

        assert!(!orphaned_names.contains(&"automanaged".to_string()),
            "Root label should not be orphaned - it's a parent of a used label");
        assert!(!orphaned_names.contains(&"automanaged/receipts".to_string()),
            "Parent label should not be orphaned - it's a parent of a used label");
        assert!(!orphaned_names.contains(&"automanaged/receipts/amazon".to_string()),
            "Used label should not be orphaned");
    }

    #[test]
    fn test_find_orphaned_labels_finds_truly_orphaned() {
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
                async fn create_filter(&self, filter: &crate::models::FilterRule) -> Result<String>;
                async fn list_filters(&self) -> Result<Vec<crate::client::ExistingFilterInfo>>;
                async fn delete_filter(&self, filter_id: &str) -> Result<()>;
                async fn update_filter(&self, filter_id: &str, filter: &crate::models::FilterRule) -> Result<String>;
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
        let mut manager = LabelManager::new(Box::new(mock_client), "automanaged".to_string());

        // Setup: Create a label cache with both used and unused labels
        manager.cache_insert("automanaged".to_string(), "label-id-root".to_string());
        manager.cache_insert("automanaged/used".to_string(), "label-id-used".to_string());
        manager.cache_insert("automanaged/unused".to_string(), "label-id-unused".to_string());

        // Create an ExistingFilterInfo that uses only automanaged/used
        let filter = crate::client::ExistingFilterInfo {
            id: "filter-1".to_string(),
            query: Some("from:example.com".to_string()),
            from: None,
            to: None,
            subject: None,
            add_label_ids: vec!["label-id-used".to_string()],
            remove_label_ids: vec![],
        };

        let filters = vec![filter];

        // Call find_orphaned_labels
        let orphaned = manager.find_orphaned_labels(&filters, "automanaged");

        // Assert: automanaged/unused IS in the orphaned list
        let orphaned_names: Vec<String> = orphaned.iter().map(|(_, name)| name.clone()).collect();

        assert!(orphaned_names.contains(&"automanaged/unused".to_string()),
            "Unused label should be orphaned - it's not a parent of any used label");

        // automanaged should not be orphaned (it's a parent of automanaged/used)
        assert!(!orphaned_names.contains(&"automanaged".to_string()),
            "Root label should not be orphaned - it's a parent of a used label");

        // automanaged/used should not be orphaned (it's actively used)
        assert!(!orphaned_names.contains(&"automanaged/used".to_string()),
            "Used label should not be orphaned");
    }
}
