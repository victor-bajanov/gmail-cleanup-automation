//! Application state management for the Tauri GUI
//!
//! This module wraps the gmail_automation library types and provides
//! thread-safe state management for the Tauri application.

use crate::commands::clusters::GuiDecision;
use crate::commands::hidden_filters::{get_hidden_filter_ids, load_hidden_filters, HiddenFiltersData};
use gmail_automation::{
    Classification, Config, EmailCluster, FilterRule,
    MessageMetadata, ProcessingState, ProductionGmailClient,
};
use parking_lot::RwLock;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

/// Application state wrapper for Tauri
///
/// This struct holds all the state needed by the GUI, including:
/// - Gmail API client
/// - Configuration
/// - Current session data (clusters, decisions, filters)
pub struct AppState {
    /// Path to credentials.json
    pub credentials_path: RwLock<PathBuf>,
    /// Path to token storage directory
    pub token_dir: RwLock<PathBuf>,
    /// Path to config.toml
    pub config_path: RwLock<PathBuf>,
    /// Application configuration
    pub config: RwLock<Option<Config>>,
    /// Gmail API client (lazily initialized)
    client: RwLock<Option<Arc<ProductionGmailClient>>>,
    /// Current processing state
    pub processing_state: RwLock<Option<ProcessingState>>,
    /// Scanned messages
    pub messages: RwLock<Vec<MessageMetadata>>,
    /// Classifications
    pub classifications: RwLock<Vec<(MessageMetadata, Classification)>>,
    /// Email clusters for review
    pub clusters: RwLock<Vec<EmailCluster>>,
    /// User decisions on clusters (GUI-specific, tracks by index)
    pub gui_decisions: RwLock<Vec<GuiDecision>>,
    /// Decision history for undo
    pub gui_decision_history: RwLock<Vec<GuiDecision>>,
    /// Proposed filter rules
    pub proposed_filters: RwLock<Vec<FilterRule>>,
    /// Existing Gmail filters (fetched from API)
    pub existing_filters: RwLock<Vec<gmail_automation::client::ExistingFilterInfo>>,
    /// Label ID cache (name -> ID)
    pub label_cache: RwLock<HashMap<String, String>>,
    /// Hidden filters data (persisted to disk)
    pub hidden_filters: RwLock<HiddenFiltersData>,
    /// Detected overlap groups for remediation
    pub remediation_groups: RwLock<Vec<gmail_automation::filter_remediation::OverlapGroup>>,
    /// User decisions for remediation groups (group_id -> decision)
    pub remediation_decisions: RwLock<HashMap<String, gmail_automation::filter_remediation::GroupDecision>>,
}

impl AppState {
    /// Creates a new AppState with default paths
    pub fn new() -> Self {
        // Project directory is parent of src-tauri (where Cargo.toml is)
        let project_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));

        let project_gmail_dir = project_dir.join(".gmail-automation");

        let home_gmail_dir = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".gmail-automation");

        // Use project dir if credentials exist there, otherwise use home
        let gmail_dir = if project_gmail_dir.join("credentials.json").exists() {
            project_gmail_dir
        } else {
            home_gmail_dir
        };

        // Load config from project's config.toml (same as CLI)
        let config_path = project_dir.join("config.toml");
        let config = if config_path.exists() {
            // Load config synchronously at startup (blocking is OK here)
            match std::fs::read_to_string(&config_path) {
                Ok(content) => {
                    match toml::from_str::<Config>(&content) {
                        Ok(cfg) => {
                            tracing::info!("Loaded config from {:?}: max_concurrent_requests={}",
                                config_path, cfg.scan.max_concurrent_requests);
                            Some(cfg)
                        }
                        Err(e) => {
                            tracing::warn!("Failed to parse config.toml: {}, using defaults", e);
                            None
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to read config.toml: {}, using defaults", e);
                    None
                }
            }
        } else {
            tracing::info!("No config.toml found at {:?}, using defaults", config_path);
            None
        };

        // Load hidden filters from disk
        let hidden_filters_data = load_hidden_filters();
        tracing::debug!("Loaded {} hidden filters from disk", hidden_filters_data.hidden_filters.len());

        Self {
            credentials_path: RwLock::new(gmail_dir.join("credentials.json")),
            token_dir: RwLock::new(gmail_dir.clone()),
            config_path: RwLock::new(config_path),
            config: RwLock::new(config),
            client: RwLock::new(None),
            processing_state: RwLock::new(None),
            messages: RwLock::new(Vec::new()),
            classifications: RwLock::new(Vec::new()),
            clusters: RwLock::new(Vec::new()),
            gui_decisions: RwLock::new(Vec::new()),
            gui_decision_history: RwLock::new(Vec::new()),
            proposed_filters: RwLock::new(Vec::new()),
            existing_filters: RwLock::new(Vec::new()),
            label_cache: RwLock::new(HashMap::new()),
            hidden_filters: RwLock::new(hidden_filters_data),
            remediation_groups: RwLock::new(vec![]),
            remediation_decisions: RwLock::new(HashMap::new()),
        }
    }

    /// Sets the credentials path
    pub fn set_credentials_path(&self, path: PathBuf) {
        *self.credentials_path.write() = path;
    }

    /// Gets the credentials path
    pub fn credentials_path(&self) -> PathBuf {
        self.credentials_path.read().clone()
    }

    /// Gets the token file path
    pub fn token_path(&self) -> PathBuf {
        self.token_dir.read().join("token.json")
    }

    /// Gets the config file path
    pub fn config_path(&self) -> PathBuf {
        self.config_path.read().clone()
    }

    /// Sets the configuration
    pub fn set_config(&self, config: Config) {
        *self.config.write() = Some(config);
    }

    /// Gets the configuration
    pub fn get_config(&self) -> Option<Config> {
        self.config.read().clone()
    }

    /// Sets the Gmail client
    pub fn set_client(&self, client: ProductionGmailClient) {
        *self.client.write() = Some(Arc::new(client));
    }

    /// Gets a reference to the Gmail client
    pub fn get_client(&self) -> Option<Arc<ProductionGmailClient>> {
        self.client.read().clone()
    }

    /// Checks if the client is initialized
    pub fn has_client(&self) -> bool {
        self.client.read().is_some()
    }

    /// Adds scanned messages
    pub fn add_messages(&self, new_messages: Vec<MessageMetadata>) {
        let mut messages = self.messages.write();
        messages.extend(new_messages);
    }

    /// Gets all scanned messages
    pub fn get_messages(&self) -> Vec<MessageMetadata> {
        self.messages.read().clone()
    }

    /// Clears scanned messages
    pub fn clear_messages(&self) {
        self.messages.write().clear();
    }

    /// Sets classifications
    pub fn set_classifications(&self, classifications: Vec<(MessageMetadata, Classification)>) {
        *self.classifications.write() = classifications;
    }

    /// Gets classifications
    pub fn get_classifications(&self) -> Vec<(MessageMetadata, Classification)> {
        self.classifications.read().clone()
    }

    /// Sets email clusters
    pub fn set_clusters(&self, clusters: Vec<EmailCluster>) {
        *self.clusters.write() = clusters;
    }

    /// Gets email clusters
    pub fn get_clusters(&self) -> Vec<EmailCluster> {
        self.clusters.read().clone()
    }

    /// Adds a GUI decision
    pub fn add_gui_decision(&self, decision: GuiDecision) {
        let mut decisions = self.gui_decisions.write();
        let mut history = self.gui_decision_history.write();

        // Add to history for undo
        history.push(decision.clone());
        decisions.push(decision);
    }

    /// Undoes the last GUI decision
    pub fn undo_last_gui_decision(&self) -> Option<GuiDecision> {
        let mut decisions = self.gui_decisions.write();
        let mut history = self.gui_decision_history.write();

        if let Some(undone) = history.pop() {
            // Find and remove from decisions
            if let Some(pos) = decisions
                .iter()
                .position(|d| d.cluster_index == undone.cluster_index)
            {
                decisions.remove(pos);
            }
            Some(undone)
        } else {
            None
        }
    }

    /// Gets all GUI decisions
    pub fn get_gui_decisions(&self) -> Vec<GuiDecision> {
        self.gui_decisions.read().clone()
    }

    /// Clears all GUI decisions
    pub fn clear_gui_decisions(&self) {
        self.gui_decisions.write().clear();
        self.gui_decision_history.write().clear();
    }

    /// Sets proposed filters
    pub fn set_proposed_filters(&self, filters: Vec<FilterRule>) {
        *self.proposed_filters.write() = filters;
    }

    /// Gets proposed filters
    pub fn get_proposed_filters(&self) -> Vec<FilterRule> {
        self.proposed_filters.read().clone()
    }

    /// Sets existing filters
    pub fn set_existing_filters(&self, filters: Vec<gmail_automation::client::ExistingFilterInfo>) {
        *self.existing_filters.write() = filters;
    }

    /// Gets existing filters
    pub fn get_existing_filters(&self) -> Vec<gmail_automation::client::ExistingFilterInfo> {
        self.existing_filters.read().clone()
    }

    /// Caches a label ID
    pub fn cache_label(&self, name: String, id: String) {
        self.label_cache.write().insert(name, id);
    }

    /// Gets a cached label ID
    pub fn get_cached_label(&self, name: &str) -> Option<String> {
        self.label_cache.read().get(name).cloned()
    }

    /// Gets the set of hidden filter IDs (for overlap filtering)
    pub fn get_hidden_filter_ids(&self) -> HashSet<String> {
        get_hidden_filter_ids(&self.hidden_filters.read())
    }

    /// Gets mutable access to hidden filters data for modifications
    pub fn with_hidden_filters_mut<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut HiddenFiltersData) -> R,
    {
        let mut data = self.hidden_filters.write();
        f(&mut data)
    }

    /// Clears all session data (for starting fresh)
    pub fn clear_session(&self) {
        self.clear_messages();
        self.classifications.write().clear();
        self.clusters.write().clear();
        self.clear_gui_decisions();
        self.proposed_filters.write().clear();
    }

    /// Gets session statistics
    pub fn get_stats(&self) -> SessionStats {
        SessionStats {
            message_count: self.messages.read().len(),
            cluster_count: self.clusters.read().len(),
            decision_count: self.gui_decisions.read().len(),
            proposed_filter_count: self.proposed_filters.read().len(),
            existing_filter_count: self.existing_filters.read().len(),
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

/// Session statistics
#[derive(Debug, Clone, serde::Serialize)]
pub struct SessionStats {
    pub message_count: usize,
    pub cluster_count: usize,
    pub decision_count: usize,
    pub proposed_filter_count: usize,
    pub existing_filter_count: usize,
}

/// Helper module for getting home directory
mod dirs {
    use std::path::PathBuf;

    pub fn home_dir() -> Option<PathBuf> {
        std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from)
    }
}
