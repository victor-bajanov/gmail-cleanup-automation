//! Command-line interface

use clap::{Parser, Subcommand};
use tracing::{info, warn};
use crate::client::{GmailClient, ProductionGmailClient};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "gmail-filters")]
#[command(version = "0.1.0")]
#[command(about = "Automated Gmail email management system", long_about = None)]
pub struct Cli {
    /// Path to configuration file
    #[arg(short, long, default_value = "config.toml")]
    pub config: PathBuf,

    /// Path to OAuth2 credentials file
    #[arg(long, default_value = "credentials.json")]
    pub credentials: PathBuf,

    /// Path to token cache file
    #[arg(long, default_value = ".gmail-automation/token.json")]
    pub token_cache: PathBuf,

    /// Path to state file
    #[arg(long, default_value = ".gmail-automation/state.json")]
    pub state_file: PathBuf,

    /// Path to rollback log file
    #[arg(long, default_value = ".gmail-automation/rollback.json")]
    pub rollback_file: PathBuf,

    /// Verbose logging
    #[arg(short, long)]
    pub verbose: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Authenticate with Gmail API
    Auth {
        /// Force re-authentication even if token exists
        #[arg(long)]
        force: bool,
    },

    /// Run the full email management workflow
    Run {
        /// Dry run mode (don't make any changes)
        #[arg(long)]
        dry_run: bool,

        /// Only create labels, don't create filters
        #[arg(long)]
        labels_only: bool,

        /// Interactive mode - prompt before each major action
        #[arg(long)]
        interactive: bool,

        /// Interactive review mode - review clusters with keyboard shortcuts
        #[arg(long)]
        review: bool,

        /// Resume from previous interrupted run
        #[arg(long)]
        resume: bool,
    },

    /// Rollback changes from a previous run
    Rollback {
        /// Run ID to rollback (from state file)
        #[arg(short, long)]
        run_id: Option<String>,

        /// Rollback only labels
        #[arg(long)]
        labels_only: bool,

        /// Rollback only filters
        #[arg(long)]
        filters_only: bool,

        /// Force rollback without confirmation
        #[arg(long)]
        force: bool,
    },

    /// Show status of current or previous runs
    Status {
        /// Show detailed information
        #[arg(long)]
        detailed: bool,
    },

    /// Generate example configuration file
    InitConfig {
        /// Path to create config file
        #[arg(short, long, default_value = "config.toml")]
        output: PathBuf,

        /// Overwrite existing file
        #[arg(long)]
        force: bool,
    },

    /// Remove all auto-managed filters (and optionally labels) from Gmail
    Unmanage {
        /// Dry run mode (don't make any changes, just show what would be deleted)
        #[arg(long)]
        dry_run: bool,

        /// Also delete the auto-managed labels (not just filters)
        #[arg(long)]
        delete_labels: bool,

        /// Force deletion without confirmation prompt
        #[arg(long)]
        force: bool,
    },
}

use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use std::time::Duration;

/// Truncate a string to max_len characters, adding "..." if truncated
fn truncate_string(s: &str, max_len: usize) -> String {
    if s.chars().count() <= max_len {
        s.to_string()
    } else {
        format!("{}...", s.chars().take(max_len.saturating_sub(3)).collect::<String>())
    }
}

/// Progress reporter using indicatif
pub struct ProgressReporter {
    multi: MultiProgress,
    spinner_style: ProgressStyle,
    bar_style: ProgressStyle,
}

impl ProgressReporter {
    pub fn new() -> Self {
        Self::with_multi_progress(MultiProgress::new())
    }

    /// Create a ProgressReporter with a shared MultiProgress (for tracing-indicatif integration)
    pub fn with_multi_progress(multi: MultiProgress) -> Self {
        // Use {elapsed} for human-readable format (e.g., "1s", "234ms")
        let spinner_style = ProgressStyle::default_spinner()
            .template("{spinner:.green} [{elapsed:>6}] {msg}")
            .unwrap()
            .tick_chars("⠁⠂⠄⡀⢀⠠⠐⠈ ");

        let bar_style = ProgressStyle::default_bar()
            .template("[{elapsed:>6}] {bar:40.cyan/blue} {pos:>7}/{len:7} {msg}")
            .unwrap()
            .progress_chars("##-");

        Self {
            multi,
            spinner_style,
            bar_style,
        }
    }

    /// Get a clone of the underlying MultiProgress for reuse
    pub fn multi_progress(&self) -> MultiProgress {
        self.multi.clone()
    }

    pub fn add_spinner(&self, msg: &str) -> ProgressBar {
        let pb = self.multi.add(ProgressBar::new_spinner());
        pb.set_style(self.spinner_style.clone());
        pb.set_message(msg.to_string());
        pb.enable_steady_tick(Duration::from_millis(100));
        pb
    }

    pub fn add_progress_bar(&self, len: u64, msg: &str) -> ProgressBar {
        let pb = self.multi.add(ProgressBar::new(len));
        pb.set_style(self.bar_style.clone());
        pb.set_message(msg.to_string());
        pb.enable_steady_tick(Duration::from_millis(100));
        pb
    }

    /// Finish a spinner and clear it from the multi-progress display
    pub fn finish_spinner(&self, pb: &ProgressBar, msg: &str) {
        pb.finish_and_clear();
        println!("  ✓ {}", msg);
    }
}

impl Default for ProgressReporter {
    fn default() -> Self {
        Self::new()
    }
}

/// Report data structure
/// Planned filter to be created (for dry run reporting)
#[derive(Debug, Clone)]
pub struct PlannedFilter {
    pub name: String,
    pub from_pattern: Option<String>,
    pub subject_keywords: Vec<String>,
    pub target_label: String,
    pub should_archive: bool,
    pub estimated_matches: usize,
    pub gmail_query: String,
    /// Actual emails that match this filter query (from live API query)
    pub actual_matches: usize,
}

/// Planned changes for dry run mode
#[derive(Debug, Clone, Default)]
pub struct PlannedChanges {
    /// Labels that would be newly created
    pub new_labels: Vec<String>,
    /// Labels that already exist (won't be created)
    pub existing_labels: Vec<String>,
    pub filters: Vec<PlannedFilter>,
    pub messages_to_label: usize,
    pub messages_to_archive: usize,
}

pub struct Report {
    pub run_id: String,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub completed_at: chrono::DateTime<chrono::Utc>,
    pub duration_seconds: i64,
    pub emails_scanned: usize,
    pub emails_classified: usize,
    pub labels_created: usize,
    pub filters_created: usize,
    pub messages_modified: usize,
    pub messages_archived: usize,
    pub classification_breakdown: Vec<(String, usize, f32)>,
    pub top_senders: Vec<(String, usize, String)>,
    /// Examples per category: category -> [(sender_email, subject)]
    pub category_examples: HashMap<String, Vec<(String, String)>>,
    /// Whether this was a dry run
    pub dry_run: bool,
    /// Planned changes (only populated in dry run mode)
    pub planned_changes: Option<PlannedChanges>,
}

impl Report {
    /// Generate Markdown report
    pub fn to_markdown(&self) -> String {
        let mut md = String::new();

        if self.dry_run {
            md.push_str("# Email Management Report (DRY RUN)\n\n");
            md.push_str("> **⚠️ DRY RUN MODE** - No changes were made. This report shows what WOULD happen.\n\n");
        } else {
            md.push_str("# Email Management Report\n\n");
        }
        md.push_str(&format!(
            "Generated: {}\n\n",
            self.completed_at.format("%Y-%m-%d %H:%M:%S")
        ));

        md.push_str("## Summary\n\n");
        md.push_str(&format!("- **Run ID:** {}\n", self.run_id));
        md.push_str(&format!("- **Emails scanned:** {}\n", self.emails_scanned));
        md.push_str(&format!(
            "- **Processing time:** {} minutes {} seconds\n",
            self.duration_seconds / 60,
            self.duration_seconds % 60
        ));
        if self.dry_run {
            md.push_str("- **Mode:** Dry Run (preview only)\n");
        }
        md.push_str("\n");

        // If dry run, show planned changes prominently
        if let Some(ref planned) = self.planned_changes {
            md.push_str("## Planned Changes\n\n");
            md.push_str("The following changes would be made when running without `--dry-run`:\n\n");

            // Labels section - show new vs existing
            md.push_str("### Labels\n\n");

            if !planned.existing_labels.is_empty() {
                md.push_str("**Already exist (will be reused):**\n");
                for label in &planned.existing_labels {
                    md.push_str(&format!("- ✓ `{}`\n", label));
                }
                md.push_str("\n");
            }

            if planned.new_labels.is_empty() {
                md.push_str("**To create:** _No new labels needed._\n\n");
            } else {
                md.push_str("**To create:**\n");
                for label in &planned.new_labels {
                    md.push_str(&format!("- + `{}`\n", label));
                }
                md.push_str(&format!("\n**Total: {} new labels** ({} existing)\n\n",
                    planned.new_labels.len(), planned.existing_labels.len()));
            }

            // Filters section - show actual match counts from live query
            md.push_str("### Filters to Create\n\n");
            if planned.filters.is_empty() {
                md.push_str("_No filters would be created._\n\n");
            } else {
                md.push_str("| Filter Name | Gmail Query | Archive | Emails Matched |\n");
                md.push_str("|-------------|-------------|---------|----------------|\n");
                let mut total_to_archive = 0;
                for filter in &planned.filters {
                    let archive_str = if filter.should_archive { "Yes" } else { "No" };
                    if filter.should_archive {
                        total_to_archive += filter.actual_matches;
                    }
                    // Escape pipes in query
                    let escaped_query = filter.gmail_query.replace('|', "\\|");
                    md.push_str(&format!(
                        "| {} | `{}` | {} | {} |\n",
                        filter.name, escaped_query, archive_str, filter.actual_matches
                    ));
                }
                md.push_str(&format!("\n**Total: {} filters**\n\n", planned.filters.len()));

                if total_to_archive > 0 {
                    md.push_str(&format!("⚠️ **{} emails would be archived** (removed from inbox) by auto-archive filters.\n\n", total_to_archive));
                }
            }

            // Actions summary
            md.push_str("### Actions Summary\n\n");
            md.push_str(&format!(
                "- **Messages that would be labelled:** {}\n",
                planned.messages_to_label
            ));
            md.push_str(&format!(
                "- **Messages that would be archived:** {}\n",
                planned.messages_to_archive
            ));
            md.push_str(&format!(
                "- **Messages that would stay in inbox:** {}\n\n",
                planned.messages_to_label.saturating_sub(planned.messages_to_archive)
            ));
        }

        md.push_str("## Classification Results\n\n");
        for (category, count, percentage) in &self.classification_breakdown {
            md.push_str(&format!(
                "### {} — {} emails ({:.1}%)\n\n",
                category, count, percentage
            ));

            // Add examples for this category
            if let Some(examples) = self.category_examples.get(category) {
                md.push_str("| Sender | Subject |\n");
                md.push_str("|--------|----------|\n");
                for (sender, subject) in examples.iter().take(10) {
                    // Truncate long subjects and escape pipes (UTF-8 safe)
                    let truncated_subject = if subject.chars().count() > 60 {
                        format!("{}...", subject.chars().take(57).collect::<String>())
                    } else {
                        subject.clone()
                    };
                    let escaped_subject = truncated_subject.replace('|', "\\|");
                    md.push_str(&format!("| {} | {} |\n", sender, escaped_subject));
                }
                md.push_str("\n");
            }
        }
        md.push_str("\n");

        // Only show these summary sections for non-dry-run mode
        // (dry run already has detailed info in "Planned Changes" section)
        if !self.dry_run {
            md.push_str("## Labels Created\n\n");
            md.push_str(&format!("- **Total labels:** {}\n\n", self.labels_created));

            md.push_str("## Filters Created\n\n");
            md.push_str(&format!("- **Total filters:** {}\n\n", self.filters_created));

            md.push_str("## Actions Taken\n\n");
            md.push_str(&format!(
                "- **Messages labelled:** {}\n",
                self.messages_modified
            ));
            md.push_str(&format!(
                "- **Messages archived:** {}\n",
                self.messages_archived
            ));
            md.push_str(&format!(
                "- **Messages kept in inbox:** {}\n\n",
                self.emails_scanned - self.messages_archived
            ));
        }

        md.push_str("## Top Senders\n\n");
        for (i, (sender, count, label)) in self.top_senders.iter().enumerate() {
            md.push_str(&format!(
                "{}. **{}** ({} emails) → {}\n",
                i + 1,
                sender,
                count,
                label
            ));
        }
        md.push_str("\n");

        if self.dry_run {
            md.push_str("---\n\n");
            md.push_str("_To apply these changes, run the command again without the `--dry-run` flag._\n");
        }

        md
    }

    /// Save report to file
    pub async fn save(&self, path: &std::path::Path) -> std::io::Result<()> {
        let markdown = self.to_markdown();
        tokio::fs::write(path, markdown).await?;
        Ok(())
    }
}

use crate::auth;
use crate::classifier::EmailClassifier;
use crate::config::Config;
use crate::error::{GmailError, Result};
use crate::filter_manager::FilterManager;
use crate::interactive::{create_clusters, ClusterDecision, DecisionAction, ReviewSession};
use crate::label_manager::LabelManager;
use crate::models::{FilterRule, MessageMetadata};
use crate::state::{ProcessingPhase, ProcessingState};
use chrono::Utc;
use std::collections::HashMap;
use std::io::{self, Write};
use std::sync::Arc;

/// Main orchestration function that runs the complete email management pipeline
///
/// This function coordinates all modules to:
/// 1. Scan historical emails
/// 2. Classify them using rules or ML
/// 3. (Optional) Interactive review of clusters
/// 4. Create labels with hierarchy
/// 5. Generate and create filter rules
/// 6. Apply labels to existing messages
/// 7. Generate summary report
///
/// # Arguments
/// * `cli` - CLI arguments containing configuration paths
/// * `dry_run` - If true, don't make any changes (read-only mode)
/// * `labels_only` - If true, only create labels, skip filters
/// * `interactive` - If true, prompt user before major actions
/// * `review` - If true, enter interactive cluster review mode
/// * `resume` - If true, resume from previous state
///
/// # Returns
/// * `Ok(Report)` - Execution report with statistics
/// * `Err(GmailError)` - If any step fails
pub async fn run_pipeline(
    cli: &Cli,
    dry_run: bool,
    labels_only: bool,
    interactive: bool,
    review: bool,
    resume: bool,
    multi_progress: MultiProgress,
) -> Result<Report> {
    let mut reporter = ProgressReporter::with_multi_progress(multi_progress);
    let started_at = Utc::now();

    // Step 1: Load configuration
    let config_spinner = reporter.add_spinner("Loading configuration...");
    let mut config = Config::load(&cli.config).await?;
    if dry_run {
        config.execution.dry_run = true;
    }
    reporter.finish_spinner(&config_spinner, &format!("Configuration loaded from {:?}", cli.config));

    // Step 2: Initialize Gmail API
    let auth_spinner = reporter.add_spinner("Authenticating with Gmail API...");
    let hub = auth::initialize_gmail_hub(&cli.credentials, &cli.token_cache).await?;
    reporter.finish_spinner(&auth_spinner, "Gmail API authenticated successfully");

    // Step 3: Create client with rate limiting
    let client = Arc::new(ProductionGmailClient::new(hub, config.scan.max_concurrent_requests));

    // Step 4: Load or create processing state
    let mut state = if resume {
        ProcessingState::load(&cli.state_file).await?
    } else {
        ProcessingState::new()
    };

    let run_id = state.run_id.clone();
    tracing::info!("Starting pipeline run: {}", run_id);

    // Step 5: Scan emails
    if !resume || matches!(state.phase, ProcessingPhase::Scanning) {
        state.phase = ProcessingPhase::Scanning;
        state.save(&cli.state_file).await?;

        let scan_spinner = reporter.add_spinner("Scanning emails from inbox...");

        // Build query for time period
        let period = chrono::Duration::days(config.scan.period_days as i64);
        let after_date = (Utc::now() - period).format("%Y/%m/%d").to_string();
        let query = format!("after:{}", after_date);

        tracing::info!("Scanning emails with query: {}", query);

        // List message IDs
        let message_ids = client.list_message_ids(&query).await?;
        let total_messages = message_ids.len();

        reporter.finish_spinner(&scan_spinner, &format!("Found {} messages to process", total_messages));

        // Fetch message metadata with progress bar
        let fetch_bar = reporter.add_progress_bar(total_messages as u64, "Fetching message metadata...");
        let fetch_bar_clone = fetch_bar.clone();

        let progress_callback: crate::client::ProgressCallback = Arc::new(move || {
            fetch_bar_clone.inc(1);
        });

        let messages = client.fetch_messages_with_progress(message_ids, progress_callback).await?;

        fetch_bar.finish_with_message(format!("Fetched {} message metadata records", messages.len()));

        state.messages_scanned = messages.len();
        state.checkpoint(&cli.state_file).await?;

        // Step 6: Classify emails
        state.phase = ProcessingPhase::Classifying;
        state.save(&cli.state_file).await?;

        let classify_bar = reporter.add_progress_bar(messages.len() as u64, "Classifying emails...");
        let classifier = EmailClassifier::new(config.labels.prefix.clone());

        let mut classifications = Vec::new();
        for msg in &messages {
            let classification = classifier.classify(msg)?;
            classifications.push((msg.clone(), classification));
            classify_bar.inc(1);
        }

        classify_bar.finish_with_message(format!("Classified {} emails", classifications.len()));

        state.messages_classified = classifications.len();
        state.checkpoint(&cli.state_file).await?;

        // Step 6.5: Load existing filters early (for matching against clusters)
        let existing_filters_spinner = reporter.add_spinner("Loading existing Gmail filters...");
        let existing_filters = client.list_filters().await.unwrap_or_else(|e| {
            warn!("Failed to load existing filters: {}", e);
            Vec::new()
        });
        reporter.finish_spinner(&existing_filters_spinner, &format!("Found {} existing filters", existing_filters.len()));

        // Step 7: Interactive review (if enabled)
        // Track review decisions for filter generation
        let mut review_decisions: Vec<ClusterDecision> = Vec::new();

        if review {
            let mut clusters = create_clusters(
                &messages,
                &classifications,
                config.classification.minimum_emails_for_label,
            );

            // Match clusters against existing filters
            // Note: We can only check the from pattern now; label matching happens later
            // after labels are created and we have label IDs
            for cluster in &mut clusters {
                // Build a temporary from pattern to match
                let from_pattern = if cluster.is_specific_sender {
                    cluster.sender_email.clone()
                } else {
                    format!("*@{}", cluster.sender_domain)
                };

                // Try to find a matching existing filter
                for existing in &existing_filters {
                    // Check if the from pattern matches
                    let pattern_matches = match &existing.query {
                        Some(existing_query) => {
                            let existing_normalized = existing_query.to_lowercase().trim().to_string();
                            let new_normalized = from_pattern.to_lowercase().trim().to_string();

                            // Gmail uses "from:(*@domain.com)" or "from:(email@domain.com)" format
                            // Extract the actual pattern from the query
                            let existing_clean = existing_normalized
                                .replace("from:(", "")
                                .replace(")", "")
                                .trim()
                                .to_string();

                            existing_clean == new_normalized ||
                            existing_normalized.contains(&format!("from:({})", new_normalized))
                        }
                        None => false,
                    };

                    if pattern_matches {
                        cluster.existing_filter_id = Some(existing.id.clone());

                        // Store the existing label ID (first one if multiple)
                        cluster.existing_filter_label_id = existing.add_label_ids.first().cloned();

                        // Store the existing archive setting (check if INBOX is removed)
                        cluster.existing_filter_archive = Some(
                            existing.remove_label_ids.iter().any(|l| l == "INBOX")
                        );

                        break; // Found a match, stop looking
                    }
                }
            }

            if !clusters.is_empty() {
                // Load existing labels to build label ID -> name mapping for the review UI
                let label_load_spinner = reporter.add_spinner("Loading existing labels...");
                let mut temp_label_manager = LabelManager::new(Box::new(client.clone()), config.labels.prefix.clone());
                let _ = temp_label_manager.load_existing_labels().await?;

                // Build label ID -> name mapping (strip prefix for display)
                let label_id_to_name: HashMap<String, String> = temp_label_manager
                    .get_label_cache()
                    .iter()
                    .map(|(name, id)| (id.clone(), name.clone()))
                    .collect();

                reporter.finish_spinner(&label_load_spinner, &format!("Loaded {} existing labels", label_id_to_name.len()));

                // Save the MultiProgress before dropping reporter (for reuse after interactive mode)
                let multi = reporter.multi_progress();
                // Clear MultiProgress before entering interactive mode to prevent redraw issues
                drop(reporter);

                println!("\nEntering interactive review mode...");
                println!("Found {} clusters to review (minimum {} emails each)\n",
                    clusters.len(),
                    config.classification.minimum_emails_for_label);

                let mut session = ReviewSession::with_label_map(clusters, label_id_to_name);
                let decisions = session.run()?;

                // Create new reporter after interactive mode (reuse same MultiProgress for tracing coordination)
                reporter = ProgressReporter::with_multi_progress(multi);

                // Apply user decisions to classifications
                for decision in &decisions {
                    if matches!(decision.action, DecisionAction::Skip) {
                        continue;
                    }

                    // Update classifications for messages in this cluster
                    for (msg, class) in &mut classifications {
                        if decision.message_ids.contains(&msg.id) {
                            class.suggested_label = decision.label.clone();
                            class.should_archive = decision.should_archive;
                        }
                    }
                }

                // Print review decisions summary
                println!("\n### User Review Decisions\n");
                for decision in &decisions {
                    let action_str = match &decision.action {
                        DecisionAction::Accept => "Accept",
                        DecisionAction::Reject => "Reject",
                        DecisionAction::Custom(_) => "Custom",
                        DecisionAction::Skip => "Skip",
                    };

                    let sender_pattern = if decision.is_specific_sender {
                        format!("from:({})", decision.sender_email)
                    } else if decision.excluded_senders.is_empty() {
                        format!("from:(*@{})", decision.sender_domain)
                    } else {
                        format!("from:(*@{}) excluding {} senders",
                            decision.sender_domain,
                            decision.excluded_senders.len())
                    };

                    let label_info = if !decision.label.is_empty() {
                        format!(" -> Label: {}", decision.label)
                    } else {
                        String::new()
                    };

                    let archive_info = if decision.should_archive {
                        " [Archive: Yes]"
                    } else {
                        ""
                    };

                    println!("  [{}] {}{}{}",
                        action_str,
                        sender_pattern,
                        label_info,
                        archive_info);
                }

                // Store decisions for filter generation (only accepted ones)
                review_decisions = decisions.into_iter()
                    .filter(|d| matches!(d.action, DecisionAction::Accept | DecisionAction::Custom(_)))
                    .collect();

                println!("\nReview complete. {} filters will be created.", review_decisions.len());
            } else {
                println!("\nNo clusters meet minimum size threshold for review.");
            }
        }

        // Step 8: Analyze classifications
        let analysis_spinner = reporter.add_spinner("Analyzing email patterns...");
        let mut category_counts: HashMap<String, usize> = HashMap::new();
        let mut domain_counts: HashMap<String, Vec<&MessageMetadata>> = HashMap::new();

        for (msg, classification) in &classifications {
            let category = format!("{:?}", classification.category);
            *category_counts.entry(category).or_insert(0) += 1;

            domain_counts
                .entry(msg.sender_domain.clone())
                .or_insert_with(Vec::new)
                .push(msg);
        }

        reporter.finish_spinner(&analysis_spinner, "Email pattern analysis complete");

        // Step 8: Create labels
        state.phase = ProcessingPhase::CreatingLabels;
        state.save(&cli.state_file).await?;

        if interactive {
            println!("\nReady to create labels. Categories found:");
            for (category, count) in &category_counts {
                println!("  - {}: {} emails", category, count);
            }
            if !confirm_action("Proceed with label creation?")? {
                return Err(GmailError::OperationCancelled("User cancelled label creation".to_string()));
            }
        }

        let label_spinner = reporter.add_spinner("Loading existing labels...");
        let mut label_manager = LabelManager::new(Box::new(client.clone()), config.labels.prefix.clone());

        // Always load existing labels to check for conflicts
        let existing_label_count = label_manager.load_existing_labels().await?;
        reporter.finish_spinner(&label_spinner, &format!("Found {} existing labels", existing_label_count));

        // Collect unique labels - from review decisions if available, otherwise from classifications
        let mut unique_labels: std::collections::HashSet<String> = std::collections::HashSet::new();

        // If review mode was used, also include labels from existing filters that were matched
        let mut existing_filter_labels: std::collections::HashSet<String> = std::collections::HashSet::new();
        if review {
            for decision in &review_decisions {
                // Collect existing filter label IDs so we can resolve them
                if let Some(label_id) = &decision.existing_filter_id {
                    for existing in &existing_filters {
                        if &existing.id == label_id {
                            if let Some(first_label_id) = existing.add_label_ids.first() {
                                existing_filter_labels.insert(first_label_id.clone());
                            }
                        }
                    }
                }
            }
        }

        if !review_decisions.is_empty() {
            // When review mode was used, only create labels for accepted clusters
            for decision in &review_decisions {
                if !decision.label.is_empty() {
                    unique_labels.insert(decision.label.clone());
                }
            }
        } else {
            // Fallback: collect from classifications for domains above threshold
            let threshold = config.classification.minimum_emails_for_label;
            let domains_above_threshold: std::collections::HashSet<String> = domain_counts
                .iter()
                .filter(|(_, msgs)| msgs.len() >= threshold)
                .map(|(domain, _)| domain.clone())
                .collect();

            for (msg, classification) in &classifications {
                if !classification.suggested_label.is_empty()
                    && domains_above_threshold.contains(&msg.sender_domain)
                {
                    unique_labels.insert(classification.suggested_label.clone());
                }
            }
        }

        // Determine which labels already exist vs need to be created
        let unique_labels_vec: Vec<String> = unique_labels.iter().cloned().collect();
        let _existing_labels = label_manager.find_existing_labels(&unique_labels_vec);
        let _new_labels = label_manager.find_new_labels(&unique_labels_vec);

        let label_spinner = reporter.add_spinner("Creating Gmail labels...");

        // Create labels (or collect planned labels in dry run mode)
        // Also build a map from label name -> label ID for filter creation
        let mut labels_created = 0;
        let mut labels_skipped = 0;
        let mut planned_labels: Vec<String> = Vec::new();
        let mut existing_label_names: Vec<String> = Vec::new();
        let mut label_name_to_id: HashMap<String, String> = HashMap::new();

        for label in &unique_labels {
            // The label from suggested_label already has full path like "auto/other/domain"
            // We need to create it directly without adding another prefix
            let sanitized = label_manager.sanitize_label_name(label).unwrap_or_default();

            // Check if label already exists in cache
            if let Some(existing_id) = label_manager.get_label_cache().get(&sanitized) {
                labels_skipped += 1;
                existing_label_names.push(label.clone());
                label_name_to_id.insert(label.clone(), existing_id.clone());
                continue;
            }

            if !dry_run {
                // Create the label directly (it already has the full path)
                let label_id = label_manager.create_label_direct(&sanitized).await?;
                state.labels_created.push(label_id.clone());
                label_name_to_id.insert(label.clone(), label_id);
                labels_created += 1;
            } else {
                planned_labels.push(label.clone());
                labels_created += 1;
            }
        }

        let label_action = if dry_run { "Would create" } else { "Created" };
        let skip_msg = if labels_skipped > 0 {
            format!(" ({} already exist)", labels_skipped)
        } else {
            String::new()
        };
        reporter.finish_spinner(&label_spinner, &format!("{} {} labels{}", label_action, labels_created, skip_msg));
        state.checkpoint(&cli.state_file).await?;

        // Step 9: Create filters (unless labels_only)
        let (filters_created, planned_filters, total_labeled_count): (usize, Vec<PlannedFilter>, usize) = if !labels_only {
            state.phase = ProcessingPhase::CreatingFilters;
            state.save(&cli.state_file).await?;

            if interactive {
                println!("\nReady to create {} filter rules", domain_counts.len());
                if !confirm_action("Proceed with filter creation?")? {
                    return Err(GmailError::OperationCancelled("User cancelled filter creation".to_string()));
                }
            }

            let mut filter_manager = FilterManager::new(Box::new(client.clone()));

            // Generate filters: from review decisions if available, otherwise from classifications
            let filters: Vec<FilterRule> = if !review_decisions.is_empty() {
                // Convert user decisions directly to filter rules (respects Accept/Reject choices)
                review_decisions.iter().map(|d| {
                    let from_pattern = if d.is_specific_sender {
                        Some(d.sender_email.clone())
                    } else {
                        Some(format!("*@{}", d.sender_domain))
                    };

                    // Build filter name including subject pattern if present
                    let filter_name = if let Some(subject) = &d.subject_pattern {
                        format!("{} + \"{}\" → {}", d.sender_email, subject, d.label)
                    } else if d.is_specific_sender {
                        format!("{} → {}", d.sender_email, d.label)
                    } else {
                        format!("{} → {}", d.sender_domain, d.label)
                    };

                    // If there's a subject pattern, use it as a subject keyword
                    let subject_keywords = if let Some(subject) = &d.subject_pattern {
                        vec![subject.clone()]
                    } else {
                        vec![]
                    };

                    FilterRule {
                        id: None,
                        name: filter_name,
                        from_pattern,
                        is_specific_sender: d.is_specific_sender,
                        excluded_senders: d.excluded_senders.clone(),
                        subject_keywords,
                        target_label_id: d.label.clone(),
                        should_archive: d.should_archive,
                        estimated_matches: d.message_ids.len(),
                    }
                }).collect()
            } else {
                // No review, generate from classifications
                filter_manager.generate_filters_from_classifications(
                    &classifications,
                    config.classification.minimum_emails_for_label,
                )
            };

            // Use progress bar instead of spinner since we're iterating
            let filter_bar = reporter.add_progress_bar(
                filters.len() as u64,
                if dry_run { "Validating filter rules..." } else { "Creating filter rules..." }
            );

            // Collect planned filters for dry run report
            let mut planned_filters: Vec<PlannedFilter> = Vec::new();
            let mut filters_created = 0;
            let mut filters_skipped = 0;
            let mut total_labeled = 0;
            let mut total_archived = 0;

            // Build a map from decisions for quick lookup of existing filter info
            let mut decision_map: HashMap<String, &ClusterDecision> = HashMap::new();
            for decision in &review_decisions {
                let key = if decision.is_specific_sender {
                    decision.sender_email.clone()
                } else {
                    format!("*@{}", decision.sender_domain)
                };
                decision_map.insert(key, decision);
            }

            for filter in &filters {
                filter_bar.set_message(format!("Processing: {}", truncate_string(&filter.name, 40)));
                // Build the Gmail query
                let gmail_query = filter_manager.build_gmail_query(filter);

                if !dry_run {
                    // Look up the actual label ID from the label name
                    let label_id = label_name_to_id.get(&filter.target_label_id)
                        .ok_or_else(|| GmailError::LabelError(
                            format!("Label ID not found for label: {}", filter.target_label_id)
                        ))?;

                    // Create a modified filter with the actual label ID
                    let mut filter_with_id = filter.clone();
                    filter_with_id.target_label_id = label_id.clone();

                    // Check if this filter came from a decision with an existing filter
                    let decision_key = if filter.is_specific_sender {
                        filter.from_pattern.clone().unwrap_or_default()
                    } else {
                        filter.from_pattern.clone().unwrap_or_default()
                    };

                    let decision = decision_map.get(&decision_key);
                    let existing_filter_id = decision.and_then(|d| d.existing_filter_id.as_ref());
                    let needs_update = decision.map(|d| d.needs_filter_update).unwrap_or(false);

                    // Handle filter creation/update/deletion
                    if let Some(existing_id) = existing_filter_id {
                        // Filter already exists
                        if matches!(decision.map(|d| &d.action), Some(DecisionAction::Reject)) {
                            // User rejected - delete the existing filter
                            info!("Deleting existing filter '{}' (ID: {}) - user rejected", filter.name, existing_id);
                            match client.delete_filter(existing_id).await {
                                Ok(_) => info!("Successfully deleted filter"),
                                Err(e) => warn!("Failed to delete filter: {}", e),
                            }
                            filters_skipped += 1;
                        } else if needs_update {
                            // User changed settings - update the filter
                            info!("Updating existing filter '{}' (ID: {}) with new settings", filter.name, existing_id);
                            match client.update_filter(existing_id, &filter_with_id).await {
                                Ok(new_id) => {
                                    state.filters_created.push(new_id);
                                    filters_created += 1;
                                }
                                Err(e) => warn!("Failed to update filter '{}': {}", filter.name, e),
                            }
                        } else {
                            // Filter exists and matches - skip creation but still apply retroactively
                            info!("Filter '{}' already exists (ID: {}), skipping creation", filter.name, existing_id);
                            filters_skipped += 1;
                        }
                    } else {
                        // No existing filter - create new one
                        let filter_id = filter_manager.create_filter(&filter_with_id).await?;
                        state.filters_created.push(filter_id);
                        filters_created += 1;
                    }

                    // ALWAYS apply labels retroactively, even if filter already exists
                    let matching_ids = client.list_message_ids(&gmail_query).await?;
                    if !matching_ids.is_empty() {
                        let count = matching_ids.len();
                        if filter.should_archive {
                            // Add label + remove INBOX in one batch call
                            info!("Labeling and archiving {} emails for filter '{}'", count, filter.name);
                            match client.batch_modify_labels(
                                &matching_ids,
                                &[label_id.clone()],
                                &["INBOX".to_string()],
                            ).await {
                                Ok(modified) => {
                                    total_labeled += modified;
                                    total_archived += modified;
                                }
                                Err(e) => warn!("Failed to label/archive emails for filter '{}': {}", filter.name, e),
                            }
                        } else {
                            // Just add label
                            info!("Labeling {} emails for filter '{}'", count, filter.name);
                            match client.batch_add_label(&matching_ids, label_id).await {
                                Ok(modified) => total_labeled += modified,
                                Err(e) => warn!("Failed to label emails for filter '{}': {}", filter.name, e),
                            }
                        }
                    }
                } else {
                    // Dry run: query the API to get actual match count (read-only)
                    let matching_ids = client.list_message_ids(&gmail_query).await?;
                    let actual_matches = matching_ids.len();

                    planned_filters.push(PlannedFilter {
                        name: filter.name.clone(),
                        from_pattern: filter.from_pattern.clone(),
                        subject_keywords: filter.subject_keywords.clone(),
                        target_label: filter.target_label_id.clone(),
                        should_archive: filter.should_archive,
                        estimated_matches: filter.estimated_matches,
                        gmail_query,
                        actual_matches,
                    });
                    filters_created += 1;
                }
                filter_bar.inc(1);
            }

            let filter_action = if dry_run { "Would create" } else { "Created" };
            let skip_msg = if filters_skipped > 0 {
                format!(", {} already existed", filters_skipped)
            } else {
                String::new()
            };
            let retroactive_msg = if !dry_run && (total_labeled > 0 || total_archived > 0) {
                let parts: Vec<String> = [
                    if total_labeled > 0 { Some(format!("labeled {}", total_labeled)) } else { None },
                    if total_archived > 0 { Some(format!("archived {}", total_archived)) } else { None },
                ].into_iter().flatten().collect();
                format!(" ({})", parts.join(", "))
            } else {
                String::new()
            };
            filter_bar.finish_with_message(format!("{} {} filters{}{}", filter_action, filters_created, skip_msg, retroactive_msg));
            state.checkpoint(&cli.state_file).await?;

            (filters_created, planned_filters, total_labeled)
        } else {
            (0, Vec::new(), 0)
        };

        // Step 10: Labels already applied during filter creation (using Gmail query search)
        // This catches ALL matching emails, not just recent ones
        state.phase = ProcessingPhase::ApplyingLabels;
        state.messages_modified = total_labeled_count;
        state.phase = ProcessingPhase::Complete;
        state.completed = true;
        state.save(&cli.state_file).await?;

        // Step 11: Generate report
        let completed_at = Utc::now();
        let duration_seconds = (completed_at - started_at).num_seconds();

        let mut classification_breakdown = Vec::new();
        let total = classifications.len() as f32;
        for (category, count) in category_counts {
            let percentage = (count as f32 / total) * 100.0;
            classification_breakdown.push((category, count, percentage));
        }
        classification_breakdown.sort_by(|a, b| b.1.cmp(&a.1));

        let mut top_senders = Vec::new();
        let mut domain_list: Vec<_> = domain_counts.into_iter().collect();
        domain_list.sort_by(|a, b| b.1.len().cmp(&a.1.len()));
        for (domain, msgs) in domain_list.iter().take(10) {
            if let Some(msg) = msgs.first() {
                let label = format!("{:?}", classifications
                    .iter()
                    .find(|(m, _)| m.id == msg.id)
                    .map(|(_, c)| &c.category)
                    .unwrap_or(&crate::models::EmailCategory::Other));
                top_senders.push((domain.clone(), msgs.len(), label));
            }
        }

        // Build category examples (up to 10 per category)
        let mut category_examples: HashMap<String, Vec<(String, String)>> = HashMap::new();
        for (msg, classification) in &classifications {
            let category = format!("{:?}", classification.category);
            let examples = category_examples.entry(category).or_insert_with(Vec::new);
            if examples.len() < 10 {
                examples.push((msg.sender_email.clone(), msg.subject.clone()));
            }
        }

        // Calculate message counts for report
        let (messages_labeled, messages_archived_count) = if dry_run {
            // For dry run, estimate from classifications
            let to_label = classifications.iter()
                .filter(|(_, c)| !c.suggested_label.is_empty())
                .count();
            let to_archive = classifications.iter()
                .filter(|(_, c)| c.should_archive)
                .count();
            (to_label, to_archive)
        } else {
            // Use actual counts from filter creation
            (total_labeled_count, state.messages_modified) // total_labeled_count tracks both
        };

        // Build planned changes for dry run mode
        let planned_changes = if dry_run {
            Some(PlannedChanges {
                new_labels: planned_labels.clone(),
                existing_labels: existing_label_names.clone(),
                filters: planned_filters,
                messages_to_label: messages_labeled,
                messages_to_archive: messages_archived_count,
            })
        } else {
            None
        };

        let report = Report {
            run_id: run_id.clone(),
            started_at,
            completed_at,
            duration_seconds,
            emails_scanned: state.messages_scanned,
            emails_classified: state.messages_classified,
            labels_created: if dry_run { labels_created } else { state.labels_created.len() },
            filters_created: if dry_run { filters_created } else { state.filters_created.len() },
            messages_modified: messages_labeled,
            messages_archived: messages_archived_count,
            classification_breakdown,
            top_senders,
            category_examples,
            dry_run,
            planned_changes,
        };

        // Save report
        let report_path = cli.state_file.with_file_name(format!("report-{}.md", run_id));
        report.save(&report_path).await
            .map_err(|e| GmailError::Unknown(format!("Failed to save report: {}", e)))?;

        tracing::info!("Report saved to {:?}", report_path);
        if dry_run {
            println!("\nDry run completed! No changes were made.");
            println!("Review the report to see what would happen: {:?}", report_path);
        } else {
            println!("\nPipeline completed successfully!");
            println!("Report saved to: {:?}", report_path);
        }

        Ok(report)
    } else {
        Err(GmailError::StateError(
            "Cannot resume from this state. Start a new run instead.".to_string(),
        ))
    }
}

/// Prompt user for confirmation
fn confirm_action(prompt: &str) -> Result<bool> {
    print!("{} [y/N]: ", prompt);
    io::stdout().flush().map_err(|e| GmailError::Unknown(e.to_string()))?;

    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .map_err(|e| GmailError::Unknown(e.to_string()))?;

    Ok(input.trim().to_lowercase() == "y")
}
