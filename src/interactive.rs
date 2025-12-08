//! Interactive cluster review UI for email classification
//!
//! Provides a terminal-based interface for reviewing and adjusting
//! email classifications with minimal keystrokes.

use crate::error::{GmailError, Result};
use crate::exclusions::ExclusionManager;
use crate::models::{Classification, EmailCategory, MessageMetadata};
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{self, ClearType},
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::io::{self, Write};
use std::path::PathBuf;

/// A cluster of emails from the same sender (specific email or domain)
#[derive(Debug, Clone)]
pub struct EmailCluster {
    pub sender_domain: String,
    /// For specific sender clusters, this is the email; for domain clusters, this is empty
    pub sender_email: String,
    /// If true, this matches a specific sender; if false, matches entire domain
    pub is_specific_sender: bool,
    /// For domain clusters, list of specific senders to exclude (they have their own clusters)
    pub excluded_senders: Vec<String>,
    /// Subject pattern for subject-based clusters (e.g., "QNAP NAS Notification")
    pub subject_pattern: Option<String>,
    pub message_ids: Vec<String>,
    pub suggested_category: EmailCategory,
    pub suggested_label: String,
    pub confidence: f32,
    pub sample_subjects: Vec<String>,
    pub should_archive: bool,
    /// Existing filter ID if a matching filter already exists
    pub existing_filter_id: Option<String>,
    /// Original label ID from existing filter (needs to be resolved to name)
    pub existing_filter_label_id: Option<String>,
    /// Original label from existing filter (for detecting changes)
    pub existing_filter_label: Option<String>,
    /// Original archive setting from existing filter (for detecting changes)
    pub existing_filter_archive: Option<bool>,
}

impl EmailCluster {
    pub fn email_count(&self) -> usize {
        self.message_ids.len()
    }
}

/// Decision made by user for a cluster
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterDecision {
    pub sender_domain: String,
    /// For specific sender clusters, the email address
    pub sender_email: String,
    /// If true, this matches a specific sender; if false, matches entire domain
    pub is_specific_sender: bool,
    /// For domain clusters, list of specific senders to exclude
    pub excluded_senders: Vec<String>,
    /// Subject pattern for subject-based clusters
    pub subject_pattern: Option<String>,
    pub message_ids: Vec<String>,
    pub label: String,
    pub should_archive: bool,
    pub action: DecisionAction,
    /// Existing filter ID if this cluster had a matching filter
    pub existing_filter_id: Option<String>,
    /// Whether the existing filter needs to be updated (settings changed)
    pub needs_filter_update: bool,
}

/// Type of decision action
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DecisionAction {
    Accept,
    Reject,
    Custom(String),
    Skip,
    Delete,
    /// Permanently exclude this cluster from future reviews (saved to exclusions file)
    Exclude,
}

/// Entry in the undo history
#[derive(Debug, Clone)]
struct HistoryEntry {
    index: usize,
    cluster: EmailCluster,
    decision: Option<ClusterDecision>,
}

/// Interactive review session
pub struct ReviewSession {
    clusters: Vec<EmailCluster>,
    decisions: HashMap<String, ClusterDecision>,
    current_index: usize,
    deferred_indices: Vec<usize>,
    history: Vec<HistoryEntry>,
    available_labels: Vec<String>,
    #[allow(dead_code)] // Stored for potential future use
    label_id_to_name: HashMap<String, String>,
    /// Number of clusters that have existing filters (shown first)
    existing_filter_count: usize,
    /// Exclusion manager for persistent exclusions
    exclusion_manager: ExclusionManager,
    /// Path to save exclusions file
    exclusions_path: PathBuf,
}

impl ReviewSession {
    /// Create a new review session with clusters to review
    pub fn new(clusters: Vec<EmailCluster>) -> Self {
        Self::with_label_map(clusters, HashMap::new())
    }

    /// Create a new review session with label ID to name mapping
    pub fn with_label_map(
        clusters: Vec<EmailCluster>,
        label_id_to_name: HashMap<String, String>,
    ) -> Self {
        Self::with_exclusions(
            clusters,
            label_id_to_name,
            PathBuf::from(".gmail-automation/exclusions.json"),
        )
    }

    /// Create a new review session with custom exclusions path
    pub fn with_exclusions(
        mut clusters: Vec<EmailCluster>,
        label_id_to_name: HashMap<String, String>,
        exclusions_path: PathBuf,
    ) -> Self {
        // Resolve existing filter label IDs to names
        for cluster in &mut clusters {
            if let Some(label_id) = &cluster.existing_filter_label_id {
                cluster.existing_filter_label = label_id_to_name.get(label_id).cloned();
            }
        }

        // Sort clusters: existing filters first, then new clusters
        clusters.sort_by_key(|c| if c.existing_filter_id.is_some() { 0 } else { 1 });

        // Count existing filters for Shift+S navigation
        let existing_filter_count = clusters
            .iter()
            .filter(|c| c.existing_filter_id.is_some())
            .count();

        // Extract unique suggested labels for the label picker
        let mut labels: Vec<String> = clusters.iter().map(|c| c.suggested_label.clone()).collect();
        labels.sort();
        labels.dedup();

        // Load existing exclusions (ignore errors - file may not exist)
        let exclusion_manager = ExclusionManager::load_sync(&exclusions_path)
            .unwrap_or_else(|_| ExclusionManager::new());

        Self {
            clusters,
            decisions: HashMap::new(),
            current_index: 0,
            deferred_indices: Vec::new(),
            history: Vec::new(),
            available_labels: labels,
            label_id_to_name,
            existing_filter_count,
            exclusion_manager,
            exclusions_path,
        }
    }

    /// Run the interactive review session
    pub fn run(&mut self) -> Result<Vec<ClusterDecision>> {
        if self.clusters.is_empty() {
            return Ok(Vec::new());
        }

        // Enable raw mode for instant key capture
        terminal::enable_raw_mode()
            .map_err(|e| GmailError::Unknown(format!("Failed to enable raw mode: {}", e)))?;

        let result = self.run_inner();

        // Always restore terminal
        let _ = terminal::disable_raw_mode();
        let _ = execute!(io::stdout(), cursor::Show);

        result
    }

    fn run_inner(&mut self) -> Result<Vec<ClusterDecision>> {
        let mut stdout = io::stdout();

        loop {
            // Clear screen and display current cluster
            execute!(
                stdout,
                terminal::Clear(ClearType::All),
                cursor::MoveTo(0, 0)
            )
            .map_err(|e| GmailError::Unknown(format!("Terminal error: {}", e)))?;

            self.display_current(&mut stdout)?;
            stdout
                .flush()
                .map_err(|e| GmailError::Unknown(e.to_string()))?;

            // Wait for key input
            // Only handle Press events to avoid key bounce on Windows
            // (Windows sends Press, Repeat, and Release events for a single key press)
            if let Event::Key(key_event) = event::read()
                .map_err(|e| GmailError::Unknown(format!("Input error: {}", e)))?
            {
                if key_event.kind != KeyEventKind::Press {
                    continue;
                }
                match self.handle_key(key_event)? {
                    SessionAction::Continue => continue,
                    SessionAction::Quit => break,
                    SessionAction::Finish => {
                        return Ok(self.decisions.values().cloned().collect());
                    }
                }
            }
        }

        Ok(Vec::new())
    }

    fn display_current(&self, stdout: &mut io::Stdout) -> Result<()> {
        let total = self.clusters.len();
        let reviewed = self.decisions.len();
        let deferred = self.deferred_indices.len();

        // Dynamic box width based on terminal size (inner content width, not including borders)
        let w = get_display_width();

        // Helper macro for raw mode: \r\n needed (not just \n)
        macro_rules! out {
            ($($arg:tt)*) => {
                write!(stdout, "{}\r\n", format!($($arg)*))
                    .map_err(|e| GmailError::Unknown(e.to_string()))?
            };
        }

        // Helper to create a padded line
        let line = |content: &str| -> String {
            let chars: Vec<char> = content.chars().collect();
            let len = chars.len();
            if len >= w {
                format!("│ {} │", chars.iter().take(w).collect::<String>())
            } else {
                format!("│ {}{} │", content, " ".repeat(w - len))
            }
        };

        // Progress bar scales with width
        let progress_width = (w / 2).min(60);
        let filled = (reviewed * progress_width) / total.max(1);
        let bar: String = (0..progress_width)
            .map(|i| if i < filled { '█' } else { '░' })
            .collect();

        let top = format!("┌{}┐", "─".repeat(w + 2));
        let mid = format!("├{}┤", "─".repeat(w + 2));
        let bottom = format!("└{}┘", "─".repeat(w + 2));

        out!("{}", top);
        let new_count = total - self.existing_filter_count;
        let progress_text = if self.existing_filter_count > 0 {
            format!(
                "Progress: [{}] {:>3}/{:<3} clusters ({} existing, {} new)",
                bar, reviewed, total, self.existing_filter_count, new_count
            )
        } else {
            format!("Progress: [{}] {:>3}/{:<3} clusters", bar, reviewed, total)
        };
        out!("{}", line(&progress_text));
        out!("{}", mid);

        if self.current_index >= self.clusters.len() {
            // All done - show summary
            out!("{}", line(""));
            out!("{}", line("All clusters reviewed!"));
            out!("{}", line(""));
            out!("{}", line("Summary:"));
            out!("{}", line(&format!("  Reviewed: {:>4}", reviewed)));
            out!("{}", line(&format!("  Deferred: {:>4}", deferred)));
            out!("{}", line(""));
            out!(
                "{}",
                line("Press [W] to write changes, [Q] to quit without saving")
            );
        } else {
            let cluster = &self.clusters[self.current_index];
            let archive_status = if cluster.should_archive { "YES" } else { "NO" };

            // Build the filter query for display
            let filter_query = if let Some(subject) = &cluster.subject_pattern {
                // Subject-based cluster
                if cluster.is_specific_sender {
                    format!("from:({}) subject:({})", cluster.sender_email, subject)
                } else {
                    format!("from:(*@{}) subject:({})", cluster.sender_domain, subject)
                }
            } else if cluster.is_specific_sender {
                format!("from:({})", cluster.sender_email)
            } else if cluster.excluded_senders.is_empty() {
                format!("from:(*@{})", cluster.sender_domain)
            } else {
                let exclusions = cluster
                    .excluded_senders
                    .iter()
                    .map(|s| format!("-from:({})", s))
                    .collect::<Vec<_>>()
                    .join(" ");
                format!("from:(*@{}) {}", cluster.sender_domain, exclusions)
            };

            // Show cluster name based on type
            let cluster_name = if let Some(subject) = &cluster.subject_pattern {
                // Subject-based cluster
                format!("{} + \"{}\"", cluster.sender_email, subject)
            } else if cluster.is_specific_sender {
                format!("{} (specific sender)", cluster.sender_email)
            } else if !cluster.excluded_senders.is_empty() {
                format!(
                    "*@{} (excl. {} senders)",
                    cluster.sender_domain,
                    cluster.excluded_senders.len()
                )
            } else {
                format!("*@{}", cluster.sender_domain)
            };

            // Truncation lengths scale with width
            let name_max = w.saturating_sub(22); // "CLUSTER: " + " (XX emails)"
            let query_max = w.saturating_sub(12); // "  Query:   "
            let label_max = w.saturating_sub(12); // "  Label:   "
            let subject_max = w.saturating_sub(6); // "  • "

            out!(
                "{}",
                line(&format!(
                    "CLUSTER: {} ({} emails)",
                    truncate_str(&cluster_name, name_max),
                    cluster.email_count()
                ))
            );
            out!("{}", mid);
            out!("{}", line("Proposed filter rule:"));
            out!(
                "{}",
                line(&format!(
                    "  Query:   {}",
                    truncate_str(&filter_query, query_max)
                ))
            );
            out!(
                "{}",
                line(&format!(
                    "  Label:   {}",
                    truncate_str(&cluster.suggested_label, label_max)
                ))
            );
            out!("{}", line(&format!("  Archive: {}", archive_status)));
            out!("{}", mid);
            out!("{}", line("Sample subjects:"));

            for subject in cluster.sample_subjects.iter().take(4) {
                let truncated = truncate_str(subject, subject_max);
                out!("{}", line(&format!("  • {}", truncated)));
            }

            // Pad remaining lines if fewer than 4 subjects
            for _ in cluster.sample_subjects.len()..4 {
                out!("{}", line(""));
            }

            out!("{}", mid);

            // Show existing filter comparison if applicable
            if cluster.existing_filter_id.is_some() {
                let current_label = cluster.existing_filter_label.as_deref().unwrap_or("(none)");
                let current_archive = if cluster.existing_filter_archive.unwrap_or(false) {
                    "YES"
                } else {
                    "NO"
                };

                // Format with colors based on differences
                let (cur_label, prop_label) =
                    format_field_pair(current_label, &cluster.suggested_label, colors::RED);
                let (cur_archive, prop_archive) =
                    format_field_pair(current_archive, archive_status, colors::BLUE);

                out!(
                    "{}",
                    line("⚠ EXISTING FILTER - [S] keeps current, [Y] updates to proposed")
                );
                out!("{}", mid);
                // Note: line() doesn't account for ANSI codes in length, so we format manually
                let cur_line = format!(
                    "  Current:  Label: {:30}  Archive: {}",
                    cur_label, cur_archive
                );
                let prop_line = format!(
                    "  Proposed: Label: {:30}  Archive: {}",
                    prop_label, prop_archive
                );
                out!("{}", line(&cur_line));
                out!("{}", line(&prop_line));
                out!("{}", mid);
                out!(
                    "{}",
                    line("[Y] Update filter  [N] Keep as-is  [S] Skip (keep current)")
                );
                out!(
                    "{}",
                    line("[D] DELETE filter  [E] Exclude permanently  [?] Help")
                );
                out!(
                    "{}",
                    line("[A] Toggle archive [L] Change label  [Shift+S] Skip all existing")
                );
            } else {
                out!(
                    "{}",
                    line("[Y] Create filter  [N] No filter  [S] Skip for now")
                );
                out!(
                    "{}",
                    line("[E] Exclude permanently  [A] Toggle archive  [L] Label")
                );
                out!("{}", line("[?] Help"));
            }
        }

        out!("{}", bottom);

        Ok(())
    }

    fn handle_key(&mut self, key: KeyEvent) -> Result<SessionAction> {
        // Handle Ctrl+C to quit
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return Ok(SessionAction::Quit);
        }

        match key.code {
            KeyCode::Char('y') | KeyCode::Enter => {
                if self.current_index < self.clusters.len() {
                    self.accept_current();
                    self.advance();
                }
                Ok(SessionAction::Continue)
            }
            KeyCode::Char('n') => {
                if self.current_index < self.clusters.len() {
                    self.reject_current();
                    self.advance();
                }
                Ok(SessionAction::Continue)
            }
            KeyCode::Char('a') => {
                if self.current_index < self.clusters.len() {
                    self.toggle_archive();
                }
                Ok(SessionAction::Continue)
            }
            KeyCode::Char('l') => {
                if self.current_index < self.clusters.len() {
                    self.custom_label()?;
                }
                Ok(SessionAction::Continue)
            }
            KeyCode::Char('s') | KeyCode::Char('S') => {
                if key.modifiers.contains(KeyModifiers::SHIFT) {
                    // Shift+S: Skip all remaining existing filter clusters
                    while self.current_index < self.existing_filter_count
                        && self.current_index < self.clusters.len()
                    {
                        self.skip_current();
                        self.current_index += 1;
                    }
                } else if self.current_index < self.clusters.len() {
                    self.skip_current();
                    self.advance();
                }
                Ok(SessionAction::Continue)
            }
            KeyCode::Char('d') | KeyCode::Char('D') => {
                // Delete only works for existing filters
                if self.current_index < self.clusters.len() {
                    let cluster = &self.clusters[self.current_index];
                    if cluster.existing_filter_id.is_some() {
                        self.delete_current();
                        self.advance();
                    }
                }
                Ok(SessionAction::Continue)
            }
            KeyCode::Char('e') | KeyCode::Char('E') => {
                // Exclude permanently - saves to exclusions file and treats as reject for this run
                if self.current_index < self.clusters.len() {
                    self.exclude_current()?;
                    self.advance();
                }
                Ok(SessionAction::Continue)
            }
            KeyCode::Char('u') => {
                self.undo();
                Ok(SessionAction::Continue)
            }
            KeyCode::Char('?') => {
                self.show_help()?;
                Ok(SessionAction::Continue)
            }
            KeyCode::Char('q') => Ok(SessionAction::Quit),
            KeyCode::Char('w') | KeyCode::Char('W') => {
                if self.current_index >= self.clusters.len() {
                    Ok(SessionAction::Finish)
                } else {
                    Ok(SessionAction::Continue)
                }
            }
            _ => Ok(SessionAction::Continue),
        }
    }

    /// Get a unique key for a cluster (specific sender email or domain, plus subject pattern if any)
    fn cluster_key(cluster: &EmailCluster) -> String {
        let base = if cluster.is_specific_sender {
            cluster.sender_email.clone()
        } else {
            format!("*@{}", cluster.sender_domain)
        };

        // Include subject pattern in key to differentiate subject-based clusters
        if let Some(subject) = &cluster.subject_pattern {
            format!("{}|subject:{}", base, subject)
        } else {
            base
        }
    }

    fn accept_current(&mut self) {
        if let Some(cluster) = self.clusters.get(self.current_index) {
            let key = Self::cluster_key(cluster);

            // Save to history for undo
            self.history.push(HistoryEntry {
                index: self.current_index,
                cluster: cluster.clone(),
                decision: self.decisions.get(&key).cloned(),
            });

            let decision = ClusterDecision {
                sender_domain: cluster.sender_domain.clone(),
                sender_email: cluster.sender_email.clone(),
                is_specific_sender: cluster.is_specific_sender,
                excluded_senders: cluster.excluded_senders.clone(),
                subject_pattern: cluster.subject_pattern.clone(),
                message_ids: cluster.message_ids.clone(),
                label: cluster.suggested_label.clone(),
                should_archive: cluster.should_archive,
                action: DecisionAction::Accept,
                existing_filter_id: cluster.existing_filter_id.clone(),
                needs_filter_update: false, // Accepting as-is
            };

            self.decisions.insert(key, decision);
        }
    }

    fn reject_current(&mut self) {
        if let Some(cluster) = self.clusters.get(self.current_index) {
            let key = Self::cluster_key(cluster);

            self.history.push(HistoryEntry {
                index: self.current_index,
                cluster: cluster.clone(),
                decision: self.decisions.get(&key).cloned(),
            });

            // Reject = no filter, no label - just skip this domain/sender
            let decision = ClusterDecision {
                sender_domain: cluster.sender_domain.clone(),
                sender_email: cluster.sender_email.clone(),
                is_specific_sender: cluster.is_specific_sender,
                excluded_senders: cluster.excluded_senders.clone(),
                subject_pattern: cluster.subject_pattern.clone(),
                message_ids: cluster.message_ids.clone(),
                label: String::new(), // No label
                should_archive: false,
                action: DecisionAction::Reject,
                existing_filter_id: cluster.existing_filter_id.clone(),
                needs_filter_update: cluster.existing_filter_id.is_some(), // Need to delete if exists
            };

            self.decisions.insert(key, decision);
        }
    }

    fn delete_current(&mut self) {
        if let Some(cluster) = self.clusters.get(self.current_index) {
            let key = Self::cluster_key(cluster);

            self.history.push(HistoryEntry {
                index: self.current_index,
                cluster: cluster.clone(),
                decision: self.decisions.get(&key).cloned(),
            });

            // Delete = remove the existing filter from Gmail
            let decision = ClusterDecision {
                sender_domain: cluster.sender_domain.clone(),
                sender_email: cluster.sender_email.clone(),
                is_specific_sender: cluster.is_specific_sender,
                excluded_senders: cluster.excluded_senders.clone(),
                subject_pattern: cluster.subject_pattern.clone(),
                message_ids: cluster.message_ids.clone(),
                label: String::new(),
                should_archive: false,
                action: DecisionAction::Delete,
                existing_filter_id: cluster.existing_filter_id.clone(),
                needs_filter_update: false, // Not updating, deleting
            };

            self.decisions.insert(key, decision);
        }
    }

    fn exclude_current(&mut self) -> Result<()> {
        if let Some(cluster) = self.clusters.get(self.current_index) {
            let key = Self::cluster_key(cluster);

            self.history.push(HistoryEntry {
                index: self.current_index,
                cluster: cluster.clone(),
                decision: self.decisions.get(&key).cloned(),
            });

            // Add to persistent exclusions
            self.exclusion_manager.add(key.clone(), None);

            // Save exclusions immediately
            self.exclusion_manager.save_sync(&self.exclusions_path)?;

            // Exclude = treated as reject for this run, but also saved persistently
            let decision = ClusterDecision {
                sender_domain: cluster.sender_domain.clone(),
                sender_email: cluster.sender_email.clone(),
                is_specific_sender: cluster.is_specific_sender,
                excluded_senders: cluster.excluded_senders.clone(),
                subject_pattern: cluster.subject_pattern.clone(),
                message_ids: cluster.message_ids.clone(),
                label: String::new(),
                should_archive: false,
                action: DecisionAction::Exclude,
                existing_filter_id: cluster.existing_filter_id.clone(),
                needs_filter_update: false,
            };

            self.decisions.insert(key, decision);
        }
        Ok(())
    }

    fn toggle_archive(&mut self) {
        if let Some(cluster) = self.clusters.get_mut(self.current_index) {
            cluster.should_archive = !cluster.should_archive;

            // If there's an existing filter and we're changing archive setting, mark for update
            if cluster.existing_filter_id.is_some() {
                let key = Self::cluster_key(cluster);
                if let Some(decision) = self.decisions.get_mut(&key) {
                    // Check if archive setting changed from original
                    let archive_changed = cluster
                        .existing_filter_archive
                        .map(|orig| orig != cluster.should_archive)
                        .unwrap_or(false);
                    decision.needs_filter_update = archive_changed;
                    decision.should_archive = cluster.should_archive;
                }
            }
        }
    }

    fn custom_label(&mut self) -> Result<()> {
        // Temporarily disable raw mode for inquire
        let _ = terminal::disable_raw_mode();
        let _ = execute!(io::stdout(), cursor::Show);

        let mut options = self.available_labels.clone();
        options.push("Create new...".to_string());

        let result = inquire::Select::new("Select label:", options)
            .with_page_size(10)
            .prompt();

        // Re-enable raw mode
        let _ = terminal::enable_raw_mode();
        let _ = execute!(io::stdout(), cursor::Hide);

        match result {
            Ok(selected) => {
                let label = if selected == "Create new..." {
                    // Temporarily disable raw mode again
                    let _ = terminal::disable_raw_mode();
                    let _ = execute!(io::stdout(), cursor::Show);

                    let custom = inquire::Text::new("Enter custom label:")
                        .prompt()
                        .unwrap_or_default();

                    let _ = terminal::enable_raw_mode();
                    let _ = execute!(io::stdout(), cursor::Hide);

                    if !custom.is_empty() {
                        self.available_labels.push(custom.clone());
                    }
                    custom
                } else {
                    selected
                };

                if !label.is_empty() {
                    if let Some(cluster) = self.clusters.get(self.current_index) {
                        let key = Self::cluster_key(cluster);

                        self.history.push(HistoryEntry {
                            index: self.current_index,
                            cluster: cluster.clone(),
                            decision: self.decisions.get(&key).cloned(),
                        });

                        // Check if label or archive changed from original
                        let label_changed = cluster
                            .existing_filter_label
                            .as_ref()
                            .map(|orig| orig != &label)
                            .unwrap_or(false);
                        let archive_changed = cluster
                            .existing_filter_archive
                            .map(|orig| orig != cluster.should_archive)
                            .unwrap_or(false);
                        let needs_update = cluster.existing_filter_id.is_some()
                            && (label_changed || archive_changed);

                        let decision = ClusterDecision {
                            sender_domain: cluster.sender_domain.clone(),
                            sender_email: cluster.sender_email.clone(),
                            is_specific_sender: cluster.is_specific_sender,
                            excluded_senders: cluster.excluded_senders.clone(),
                            subject_pattern: cluster.subject_pattern.clone(),
                            message_ids: cluster.message_ids.clone(),
                            label: label.clone(),
                            should_archive: cluster.should_archive,
                            action: DecisionAction::Custom(label),
                            existing_filter_id: cluster.existing_filter_id.clone(),
                            needs_filter_update: needs_update,
                        };

                        self.decisions.insert(key, decision);
                    }
                    self.advance();
                }
            }
            Err(_) => {
                // User cancelled, do nothing
            }
        }

        Ok(())
    }

    fn skip_current(&mut self) {
        if self.current_index < self.clusters.len() {
            self.deferred_indices.push(self.current_index);

            if let Some(cluster) = self.clusters.get(self.current_index) {
                let key = Self::cluster_key(cluster);

                // Save to history for undo
                self.history.push(HistoryEntry {
                    index: self.current_index,
                    cluster: cluster.clone(),
                    decision: self.decisions.get(&key).cloned(),
                });

                let decision = ClusterDecision {
                    sender_domain: cluster.sender_domain.clone(),
                    sender_email: cluster.sender_email.clone(),
                    is_specific_sender: cluster.is_specific_sender,
                    excluded_senders: cluster.excluded_senders.clone(),
                    subject_pattern: cluster.subject_pattern.clone(),
                    message_ids: cluster.message_ids.clone(),
                    label: cluster.suggested_label.clone(),
                    should_archive: cluster.should_archive,
                    action: DecisionAction::Skip,
                    existing_filter_id: cluster.existing_filter_id.clone(),
                    needs_filter_update: false, // Skipping means no changes
                };

                self.decisions.insert(key, decision);
            }
        }
    }

    fn advance(&mut self) {
        self.current_index += 1;

        // Skip clusters we've already decided on
        while self.current_index < self.clusters.len() {
            let cluster = &self.clusters[self.current_index];
            let key = Self::cluster_key(cluster);
            if !self.decisions.contains_key(&key)
                || matches!(
                    self.decisions.get(&key).map(|d| &d.action),
                    Some(DecisionAction::Skip)
                )
            {
                break;
            }
            self.current_index += 1;
        }
    }

    fn undo(&mut self) {
        if let Some(entry) = self.history.pop() {
            // Restore cluster state
            if entry.index < self.clusters.len() {
                self.clusters[entry.index] = entry.cluster.clone();
            }

            let key = Self::cluster_key(&entry.cluster);

            // Restore or remove decision
            if let Some(prev_decision) = entry.decision {
                self.decisions.insert(key, prev_decision);
            } else {
                self.decisions.remove(&key);
            }

            // Go back to that index
            self.current_index = entry.index;

            // Remove from deferred if it was there
            self.deferred_indices.retain(|&i| i != entry.index);
        }
    }

    fn show_help(&self) -> Result<()> {
        let _ = terminal::disable_raw_mode();
        let _ = execute!(
            io::stdout(),
            cursor::Show,
            terminal::Clear(ClearType::All),
            cursor::MoveTo(0, 0)
        );

        let w = get_display_width();
        let line = |content: &str| {
            let len = content.chars().count();
            if len >= w {
                println!("║ {} ║", content.chars().take(w).collect::<String>());
            } else {
                println!("║ {}{} ║", content, " ".repeat(w - len));
            }
        };
        let sep = || println!("╠{}╣", "═".repeat(w + 2));

        // Center the title
        let title = "KEYBOARD SHORTCUTS";
        let title_padding = (w.saturating_sub(title.len())) / 2;
        let centered_title = format!("{}{}", " ".repeat(title_padding), title);

        println!("╔{}╗", "═".repeat(w + 2));
        line(&centered_title);
        sep();
        line("DECISIONS (new clusters):");
        line("  Y / Enter  CREATE FILTER with shown label & archive setting");
        line("  N          NO FILTER - leave these emails alone");
        line("  S          SKIP - don't decide now, come back later");
        sep();
        line("DECISIONS (existing filters):");
        line("  Y / Enter  UPDATE filter to proposed label & archive setting");
        line("  N          KEEP AS-IS - leave existing filter unchanged");
        line("  S          SKIP - don't decide now (keeps current filter)");
        line("  D          DELETE - remove the existing filter from Gmail");
        line("  Shift+S    Skip all remaining existing filters, jump to new clusters");
        sep();
        line("EDIT BEFORE ACCEPTING:");
        line("  A          Toggle auto-archive ON/OFF");
        line("  L          Change the target label");
        sep();
        line("PERMANENT EXCLUSION:");
        line("  E          EXCLUDE permanently - never show this cluster again");
        line("             (use --ignore-exclusions to see all clusters afresh)");
        sep();
        line("NAVIGATION:");
        line("  U          Undo last decision");
        line("  ?          Show this help");
        line("  Q          Quit without saving any changes");
        line("  W          Write all changes (shown at end of review)");
        line("  Ctrl+C     Force quit immediately");
        sep();
        line("WHAT HAPPENS:");
        line("  Y creates: Gmail filter matching from:(*@domain) or from:(specific@email)");
        line("             → applies your chosen label");
        line("             → optionally archives matching emails");
        line("  N ignores: No filter or label created for this sender/domain");
        line("  D deletes: Removes the existing Gmail filter entirely");
        line("  E excludes: Saves to .gmail-automation/exclusions.json, hidden in future runs");
        println!("╚{}╝", "═".repeat(w + 2));
        println!();
        println!("Press any key to continue...");

        let _ = io::stdout().flush();

        // Wait for any key press (not release/repeat)
        let _ = terminal::enable_raw_mode();
        loop {
            if let Ok(Event::Key(key_event)) = event::read() {
                if key_event.kind == KeyEventKind::Press {
                    break;
                }
            }
        }

        Ok(())
    }
}

enum SessionAction {
    Continue,
    Quit,
    Finish,
}

/// Get the display width for the UI box, based on terminal size
/// Returns inner content width (excluding borders)
fn get_display_width() -> usize {
    // Get terminal width, default to 120 if detection fails
    let term_width = terminal::size()
        .map(|(cols, _)| cols as usize)
        .unwrap_or(120);

    // Content width = terminal width - 4 (for "│ " and " │" borders)
    // Clamp between 80 and 160 for reasonable display
    let content_width = term_width.saturating_sub(4);
    content_width.clamp(80, 160)
}

/// Truncate a string to fit within max_len characters (UTF-8 safe)
fn truncate_str(s: &str, max_len: usize) -> String {
    if s.chars().count() <= max_len {
        s.to_string()
    } else {
        format!(
            "{}...",
            s.chars()
                .take(max_len.saturating_sub(3))
                .collect::<String>()
        )
    }
}

/// ANSI color codes for field comparison display
mod colors {
    pub const GREY: &str = "\x1b[90m";
    pub const RED: &str = "\x1b[31m";
    pub const BLUE: &str = "\x1b[34m";
    pub const RESET: &str = "\x1b[0m";
}

/// Format two field values with color based on whether they differ
/// Returns (current_colored, proposed_colored)
fn format_field_pair(current: &str, proposed: &str, differ_color: &str) -> (String, String) {
    if current == proposed {
        // Same - both grey
        (
            format!("{}{}{}", colors::GREY, current, colors::RESET),
            format!("{}{}{}", colors::GREY, proposed, colors::RESET),
        )
    } else {
        // Different - both colored
        (
            format!("{}{}{}", differ_color, current, colors::RESET),
            format!("{}{}{}", differ_color, proposed, colors::RESET),
        )
    }
}

/// Create email clusters from messages and classifications
///
/// Uses hierarchical clustering with subject pattern detection:
/// 1. First, create narrow clusters for repeated subject + sender combinations
/// 2. Then create specific sender clusters (for senders with enough emails)
/// 3. Finally, create domain-wide clusters for remaining emails
///
///    This ensures automated emails with consistent subjects get their own granular filters.
pub fn create_clusters(
    _messages: &[MessageMetadata],
    classifications: &[(MessageMetadata, Classification)],
    min_emails: usize,
) -> Vec<EmailCluster> {
    let mut clusters: Vec<EmailCluster> = Vec::new();

    // Step 1: Group messages by domain
    let mut domain_map: HashMap<String, Vec<(&MessageMetadata, &Classification)>> = HashMap::new();
    for (msg, class) in classifications {
        domain_map
            .entry(msg.sender_domain.clone())
            .or_default()
            .push((msg, class));
    }

    // Step 2: For each domain, do hierarchical clustering
    for (domain, domain_msgs) in domain_map {
        // Group by specific sender email within this domain
        let mut sender_map: HashMap<String, Vec<(&MessageMetadata, &Classification)>> =
            HashMap::new();
        for (msg, class) in &domain_msgs {
            sender_map
                .entry(msg.sender_email.clone())
                .or_default()
                .push((*msg, *class));
        }

        // Track which senders/subject patterns get their own clusters
        let mut specific_senders: Vec<String> = Vec::new();
        let mut remaining_msgs: Vec<(&MessageMetadata, &Classification)> = Vec::new();

        // Step 3: For each sender, detect subject patterns and create clusters
        for (sender_email, sender_msgs) in sender_map {
            // Step 3a: Try to detect repeated subject patterns within this sender
            let subject_patterns = detect_subject_patterns(&sender_msgs, min_emails);

            let mut sender_remaining: Vec<(&MessageMetadata, &Classification)> =
                sender_msgs.clone();

            // Create clusters for subject patterns that meet threshold
            for (pattern, pattern_msgs) in subject_patterns {
                if pattern_msgs.len() >= min_emails {
                    // Create a subject-specific cluster
                    let cluster = build_cluster_with_subject(
                        &domain,
                        &sender_email,
                        true,   // is_specific_sender
                        vec![], // no exclusions
                        Some(pattern.clone()),
                        &pattern_msgs,
                    );
                    clusters.push(cluster);

                    // Remove these messages from sender_remaining
                    let pattern_ids: HashSet<String> =
                        pattern_msgs.iter().map(|(m, _)| m.id.clone()).collect();
                    sender_remaining.retain(|(m, _)| !pattern_ids.contains(&m.id));
                }
            }

            // Step 3b: If this sender still has enough remaining emails (without subject patterns),
            // create a sender-wide cluster
            if sender_remaining.len() >= min_emails {
                specific_senders.push(sender_email.clone());

                let cluster = build_cluster(
                    &domain,
                    &sender_email,
                    true,   // is_specific_sender
                    vec![], // no exclusions for specific sender clusters
                    &sender_remaining,
                );
                clusters.push(cluster);
            } else {
                // Not enough for a sender cluster, add to remaining for domain cluster
                remaining_msgs.extend(sender_remaining);
            }
        }

        // Step 4: If remaining messages meet threshold, create domain cluster with exclusions
        if remaining_msgs.len() >= min_emails {
            let cluster = build_cluster(
                &domain,
                "",                       // no specific sender
                false,                    // is domain cluster
                specific_senders.clone(), // exclude specific senders that have their own clusters
                &remaining_msgs,
            );
            clusters.push(cluster);
        }
    }

    // Sort by specificity first (narrow clusters before broad), then by email count
    clusters.sort_by(|a, b| {
        // Subject-based clusters first (most specific)
        match (&a.subject_pattern, &b.subject_pattern) {
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            _ => {
                // Then by sender specificity
                match (a.is_specific_sender, b.is_specific_sender) {
                    (true, false) => std::cmp::Ordering::Less,
                    (false, true) => std::cmp::Ordering::Greater,
                    _ => b.email_count().cmp(&a.email_count()), // Finally by count
                }
            }
        }
    });

    clusters
}

/// Detect repeated subject patterns within a set of messages
/// Returns a map from subject pattern to matching messages
fn detect_subject_patterns<'a>(
    msgs: &[(&'a MessageMetadata, &'a Classification)],
    min_threshold: usize,
) -> HashMap<String, Vec<(&'a MessageMetadata, &'a Classification)>> {
    let mut pattern_map: HashMap<String, Vec<(&'a MessageMetadata, &'a Classification)>> =
        HashMap::new();

    // Group by exact subject match first
    let mut subject_groups: HashMap<String, Vec<(&'a MessageMetadata, &'a Classification)>> =
        HashMap::new();
    for item in msgs {
        let subject = normalize_subject(&item.0.subject);
        subject_groups.entry(subject).or_default().push(*item);
    }

    // Only keep subjects that appear frequently enough
    for (subject, group) in subject_groups {
        if group.len() >= min_threshold {
            pattern_map.insert(subject, group);
        }
    }

    pattern_map
}

/// Normalize subject for pattern matching
/// Removes Re:, Fwd:, Fw: prefixes (case-insensitive, with or without space)
fn normalize_subject(subject: &str) -> String {
    let prefixes = ["re:", "fwd:", "fw:"];
    let mut result = subject.trim().to_string();

    // Keep removing prefixes until none match (handles "Re: Fwd: Re: Subject")
    loop {
        let lower = result.to_lowercase();
        let mut matched = false;

        for prefix in &prefixes {
            if lower.starts_with(prefix) {
                // Remove the prefix (preserving original casing in result)
                result = result[prefix.len()..].trim_start().to_string();
                matched = true;
                break;
            }
        }

        if !matched {
            break;
        }
    }

    result
}

/// Build a single email cluster from a set of messages (without subject pattern)
fn build_cluster(
    domain: &str,
    sender_email: &str,
    is_specific_sender: bool,
    excluded_senders: Vec<String>,
    msgs: &[(&MessageMetadata, &Classification)],
) -> EmailCluster {
    build_cluster_with_subject(
        domain,
        sender_email,
        is_specific_sender,
        excluded_senders,
        None,
        msgs,
    )
}

/// Build a single email cluster from a set of messages (with optional subject pattern)
fn build_cluster_with_subject(
    domain: &str,
    sender_email: &str,
    is_specific_sender: bool,
    excluded_senders: Vec<String>,
    subject_pattern: Option<String>,
    msgs: &[(&MessageMetadata, &Classification)],
) -> EmailCluster {
    let first = msgs.first().unwrap();
    let message_ids: Vec<String> = msgs.iter().map(|(m, _)| m.id.clone()).collect();
    let sample_subjects: Vec<String> = msgs
        .iter()
        .take(5)
        .map(|(m, _)| m.subject.clone())
        .collect();

    // Calculate average confidence
    let avg_confidence: f32 =
        msgs.iter().map(|(_, c)| c.confidence).sum::<f32>() / msgs.len() as f32;

    // Use most common category
    let mut category_counts: HashMap<EmailCategory, usize> = HashMap::new();
    for (_, c) in msgs {
        *category_counts.entry(c.category.clone()).or_insert(0) += 1;
    }
    let suggested_category = category_counts
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(cat, _)| cat)
        .unwrap_or_else(|| first.1.category.clone());

    // Use most common label
    let mut label_counts: HashMap<String, usize> = HashMap::new();
    for (_, c) in msgs {
        if !c.suggested_label.is_empty() {
            *label_counts.entry(c.suggested_label.clone()).or_insert(0) += 1;
        }
    }
    let suggested_label = label_counts
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(label, _)| label)
        .unwrap_or_else(|| first.1.suggested_label.clone());

    // Check if majority suggest archiving
    let archive_count = msgs.iter().filter(|(_, c)| c.should_archive).count();
    let should_archive = archive_count > msgs.len() / 2;

    EmailCluster {
        sender_domain: domain.to_string(),
        sender_email: sender_email.to_string(),
        is_specific_sender,
        excluded_senders,
        subject_pattern,
        message_ids,
        suggested_category,
        suggested_label,
        confidence: avg_confidence,
        sample_subjects,
        should_archive,
        existing_filter_id: None, // Will be set by caller after matching against existing filters
        existing_filter_label_id: None, // Will be set by caller after matching against existing filters
        existing_filter_label: None, // Will be set by caller after matching against existing filters
        existing_filter_archive: None, // Will be set by caller after matching against existing filters
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn create_test_message(id: &str, sender: &str, subject: &str) -> MessageMetadata {
        let domain = sender.split('@').nth(1).unwrap_or("example.com");
        MessageMetadata {
            id: id.to_string(),
            thread_id: format!("thread-{}", id),
            sender_email: sender.to_string(),
            sender_domain: domain.to_string(),
            sender_name: "Test".to_string(),
            subject: subject.to_string(),
            recipients: vec![],
            date_received: Utc::now(),
            labels: vec![],
            has_unsubscribe: false,
            is_automated: false,
        }
    }

    fn create_test_classification(msg: &MessageMetadata) -> Classification {
        Classification {
            message_id: msg.id.clone(),
            category: EmailCategory::Newsletter,
            confidence: 0.85,
            suggested_label: "auto/newsletters".to_string(),
            should_archive: true,
            reasoning: None,
        }
    }

    #[test]
    fn test_create_clusters_basic() {
        let messages = vec![
            create_test_message("1", "news@example.com", "Subject 1"),
            create_test_message("2", "news@example.com", "Subject 2"),
            create_test_message("3", "news@example.com", "Subject 3"),
            create_test_message("4", "other@different.com", "Subject 4"),
        ];

        let classifications: Vec<(MessageMetadata, Classification)> = messages
            .iter()
            .map(|m| (m.clone(), create_test_classification(m)))
            .collect();

        let clusters = create_clusters(&messages, &classifications, 2);

        assert_eq!(clusters.len(), 1); // Only example.com has >= 2 emails
        assert_eq!(clusters[0].sender_domain, "example.com");
        assert_eq!(clusters[0].email_count(), 3);
        assert!(clusters[0].subject_pattern.is_none()); // No repeated subjects
    }

    #[test]
    fn test_create_clusters_with_subject_patterns() {
        // Test hierarchical clustering: subject patterns should create narrow clusters first
        let messages = vec![
            // Automated emails with same subject from me@example.com
            create_test_message("1", "me@example.com", "QNAP NAS Notification"),
            create_test_message("2", "me@example.com", "QNAP NAS Notification"),
            create_test_message("3", "me@example.com", "QNAP NAS Notification"),
            // Different subject from same sender
            create_test_message("4", "me@example.com", "Daily Backup Report"),
            create_test_message("5", "me@example.com", "Daily Backup Report"),
            create_test_message("6", "me@example.com", "Daily Backup Report"),
            // Regular emails from same sender
            create_test_message("7", "me@example.com", "Meeting notes"),
            create_test_message("8", "me@example.com", "Project update"),
        ];

        let classifications: Vec<(MessageMetadata, Classification)> = messages
            .iter()
            .map(|m| (m.clone(), create_test_classification(m)))
            .collect();

        let clusters = create_clusters(&messages, &classifications, 3);

        // Should create 2 narrow subject-based clusters (for automated emails)
        // The remaining 2 regular emails don't meet threshold, so no sender cluster
        assert_eq!(clusters.len(), 2);

        // Subject-based clusters should come first (sorted by specificity)
        assert!(clusters[0].subject_pattern.is_some());
        assert!(clusters[1].subject_pattern.is_some());

        // Verify one cluster is for QNAP notifications
        let qnap_cluster = clusters.iter().find(|c| {
            c.subject_pattern
                .as_ref()
                .map(|s| s.contains("QNAP"))
                .unwrap_or(false)
        });
        assert!(qnap_cluster.is_some());
        assert_eq!(qnap_cluster.unwrap().email_count(), 3);

        // Verify one cluster is for backup reports
        let backup_cluster = clusters.iter().find(|c| {
            c.subject_pattern
                .as_ref()
                .map(|s| s.contains("Backup"))
                .unwrap_or(false)
        });
        assert!(backup_cluster.is_some());
        assert_eq!(backup_cluster.unwrap().email_count(), 3);
    }

    #[test]
    fn test_create_clusters_hierarchical_fallback() {
        // Test that emails without patterns fall back to sender clustering
        let messages = vec![
            // Subject pattern cluster (meets threshold)
            create_test_message("1", "alerts@service.com", "System Alert"),
            create_test_message("2", "alerts@service.com", "System Alert"),
            create_test_message("3", "alerts@service.com", "System Alert"),
            // Different subjects from same sender (should form sender cluster)
            create_test_message("4", "alerts@service.com", "Weekly Summary"),
            create_test_message("5", "alerts@service.com", "Monthly Report"),
            create_test_message("6", "alerts@service.com", "Status Update"),
        ];

        let classifications: Vec<(MessageMetadata, Classification)> = messages
            .iter()
            .map(|m| (m.clone(), create_test_classification(m)))
            .collect();

        let clusters = create_clusters(&messages, &classifications, 3);

        // Should create 2 clusters: one subject-based, one sender-based
        assert_eq!(clusters.len(), 2);

        // First should be subject-based (more specific)
        assert!(clusters[0].subject_pattern.is_some());
        assert_eq!(
            clusters[0].subject_pattern.as_ref().unwrap(),
            "System Alert"
        );
        assert_eq!(clusters[0].email_count(), 3);

        // Second should be sender-based (no subject pattern)
        assert!(clusters[1].subject_pattern.is_none());
        assert!(clusters[1].is_specific_sender);
        assert_eq!(clusters[1].email_count(), 3);
    }

    #[test]
    fn test_normalize_subject() {
        // Basic cases
        assert_eq!(normalize_subject("Re: Test"), "Test");
        assert_eq!(normalize_subject("Fwd: Important"), "Important");
        assert_eq!(normalize_subject("RE: Test"), "Test");
        assert_eq!(normalize_subject("  Test  "), "Test");
        assert_eq!(normalize_subject("FWD: Urgent"), "Urgent");

        // Case insensitive (new behavior)
        assert_eq!(normalize_subject("re: test"), "test");
        assert_eq!(normalize_subject("fwd: test"), "test");
        assert_eq!(normalize_subject("fw: test"), "test");

        // Without space after colon (new behavior)
        assert_eq!(normalize_subject("Re:Test"), "Test");
        assert_eq!(normalize_subject("RE:Alert"), "Alert");
        assert_eq!(normalize_subject("fwd:message"), "message");

        // Chained prefixes (new behavior)
        assert_eq!(normalize_subject("Re: Fwd: Re: Original"), "Original");
        assert_eq!(normalize_subject("FW: RE: FWD: Test"), "Test");

        // No prefix
        assert_eq!(normalize_subject("Normal Subject"), "Normal Subject");
        assert_eq!(
            normalize_subject("Reply to your message"),
            "Reply to your message"
        );
    }

    #[test]
    fn test_truncate_str() {
        assert_eq!(truncate_str("short", 10), "short");
        assert_eq!(truncate_str("this is a longer string", 10), "this is...");
    }

    #[test]
    fn test_cluster_decision() {
        let cluster = EmailCluster {
            sender_domain: "example.com".to_string(),
            sender_email: "test@example.com".to_string(),
            is_specific_sender: false,
            excluded_senders: vec![],
            subject_pattern: None,
            message_ids: vec!["1".to_string(), "2".to_string()],
            suggested_category: EmailCategory::Newsletter,
            suggested_label: "auto/newsletters".to_string(),
            confidence: 0.9,
            sample_subjects: vec!["Subject 1".to_string()],
            should_archive: true,
            existing_filter_id: None,
            existing_filter_label_id: None,
            existing_filter_label: None,
            existing_filter_archive: None,
        };

        assert_eq!(cluster.email_count(), 2);
    }

    #[test]
    fn test_cluster_decision_serialization() {
        let decision = ClusterDecision {
            sender_email: "test@example.com".to_string(),
            sender_domain: "example.com".to_string(),
            is_specific_sender: true,
            subject_pattern: Some("Newsletter".to_string()),
            message_ids: vec!["msg1".to_string(), "msg2".to_string()],
            label: "AutoManaged/newsletters".to_string(),
            action: DecisionAction::Accept,
            should_archive: true,
            existing_filter_id: None,
            needs_filter_update: false,
            excluded_senders: vec![],
        };

        // Serialize to JSON
        let json = serde_json::to_string(&decision).unwrap();
        assert!(json.contains("test@example.com"));
        assert!(json.contains("Newsletter"));

        // Deserialize back
        let restored: ClusterDecision = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.sender_email, "test@example.com");
        assert_eq!(restored.subject_pattern, Some("Newsletter".to_string()));
        assert!(matches!(restored.action, DecisionAction::Accept));
    }

    #[test]
    fn test_decision_action_serialization() {
        let actions = vec![
            DecisionAction::Accept,
            DecisionAction::Reject,
            DecisionAction::Skip,
            DecisionAction::Custom("MyLabel".to_string()),
        ];

        for action in actions {
            let json = serde_json::to_string(&action).unwrap();
            let restored: DecisionAction = serde_json::from_str(&json).unwrap();
            // Verify round-trip works (comparing debug strings since action has PartialEq)
            assert_eq!(format!("{:?}", action), format!("{:?}", restored));
        }
    }
}
