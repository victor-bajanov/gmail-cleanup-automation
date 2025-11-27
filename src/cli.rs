//! Command-line interface

use clap::{Parser, Subcommand};
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
}

use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use std::time::Duration;

/// Progress reporter using indicatif
pub struct ProgressReporter {
    multi: MultiProgress,
    spinner_style: ProgressStyle,
    bar_style: ProgressStyle,
}

impl ProgressReporter {
    pub fn new() -> Self {
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
            multi: MultiProgress::new(),
            spinner_style,
            bar_style,
        }
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
}

/// Planned changes for dry run mode
#[derive(Debug, Clone, Default)]
pub struct PlannedChanges {
    pub labels: Vec<String>,
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

            // Labels section
            md.push_str("### Labels to Create\n\n");
            if planned.labels.is_empty() {
                md.push_str("_No new labels would be created._\n\n");
            } else {
                for label in &planned.labels {
                    md.push_str(&format!("- `{}`\n", label));
                }
                md.push_str(&format!("\n**Total: {} labels**\n\n", planned.labels.len()));
            }

            // Filters section
            md.push_str("### Filters to Create\n\n");
            if planned.filters.is_empty() {
                md.push_str("_No filters would be created._\n\n");
            } else {
                md.push_str("| Filter Name | Gmail Query | Archive | Matches |\n");
                md.push_str("|-------------|-------------|---------|----------|\n");
                for filter in &planned.filters {
                    let archive_str = if filter.should_archive { "Yes" } else { "No" };
                    // Escape pipes in query
                    let escaped_query = filter.gmail_query.replace('|', "\\|");
                    md.push_str(&format!(
                        "| {} | `{}` | {} | {} |\n",
                        filter.name, escaped_query, archive_str, filter.estimated_matches
                    ));
                }
                md.push_str(&format!("\n**Total: {} filters**\n\n", planned.filters.len()));
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
use crate::interactive::{create_clusters, DecisionAction, ReviewSession};
use crate::label_manager::LabelManager;
use crate::models::MessageMetadata;
use crate::state::{ProcessingPhase, ProcessingState};
use chrono::Utc;
use futures::StreamExt;
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
) -> Result<Report> {
    let reporter = ProgressReporter::new();
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
        let classifier = EmailClassifier::new();

        let mut classifications = Vec::new();
        for msg in &messages {
            let classification = classifier.classify(msg)?;
            classifications.push((msg.clone(), classification));
            classify_bar.inc(1);
        }

        classify_bar.finish_with_message(format!("Classified {} emails", classifications.len()));

        state.messages_classified = classifications.len();
        state.checkpoint(&cli.state_file).await?;

        // Step 7: Interactive review (if enabled)
        if review {
            let clusters = create_clusters(
                &messages,
                &classifications,
                config.classification.minimum_emails_for_label,
            );

            if !clusters.is_empty() {
                println!("\nEntering interactive review mode...");
                println!("Found {} clusters to review (minimum {} emails each)\n",
                    clusters.len(),
                    config.classification.minimum_emails_for_label);

                let mut session = ReviewSession::new(clusters);
                let decisions = session.run()?;

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

                println!("\nReview complete. Applied {} decisions.", decisions.len());
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

        let label_spinner = reporter.add_spinner("Creating Gmail labels...");
        let mut label_manager = LabelManager::new(Box::new(client.clone()), config.labels.prefix.clone());

        // Collect unique labels from suggested_label field (includes user's review choices)
        let mut unique_labels: std::collections::HashSet<String> = std::collections::HashSet::new();
        for (_, classification) in &classifications {
            if !classification.suggested_label.is_empty() {
                unique_labels.insert(classification.suggested_label.clone());
            }
        }

        // Create labels (or collect planned labels in dry run mode)
        let mut labels_created = 0;
        let mut planned_labels: Vec<String> = Vec::new();
        for label in &unique_labels {
            if !dry_run {
                // Extract the category part after the prefix for label creation
                let label_name = label.split('/').last().unwrap_or(label);
                let label_id = label_manager.create_label(label_name).await?;
                state.labels_created.push(label_id);
                labels_created += 1;
            } else {
                planned_labels.push(label.clone());
                labels_created += 1;
            }
        }

        let label_action = if dry_run { "Would create" } else { "Created" };
        reporter.finish_spinner(&label_spinner, &format!("{} {} labels", label_action, labels_created));
        state.checkpoint(&cli.state_file).await?;

        // Step 9: Create filters (unless labels_only)
        let (filters_created, planned_filters): (usize, Vec<PlannedFilter>) = if !labels_only {
            state.phase = ProcessingPhase::CreatingFilters;
            state.save(&cli.state_file).await?;

            if interactive {
                println!("\nReady to create {} filter rules", domain_counts.len());
                if !confirm_action("Proceed with filter creation?")? {
                    return Err(GmailError::OperationCancelled("User cancelled filter creation".to_string()));
                }
            }

            let filter_spinner = reporter.add_spinner("Generating and creating filter rules...");
            let filter_manager = FilterManager::new(Box::new(client.clone()));

            let filters = filter_manager.generate_filters_from_classifications(
                &classifications,
                config.classification.minimum_emails_for_label,
            );

            // Collect planned filters for dry run report
            let mut planned_filters: Vec<PlannedFilter> = Vec::new();
            let mut filters_created = 0;
            for filter in &filters {
                if !dry_run {
                    let mut fm = FilterManager::new(Box::new(client.clone()));
                    let filter_id = fm.create_filter(filter).await?;
                    state.filters_created.push(filter_id);
                    filters_created += 1;
                } else {
                    // Build the Gmail query for display
                    let gmail_query = filter_manager.build_gmail_query(filter);
                    planned_filters.push(PlannedFilter {
                        name: filter.name.clone(),
                        from_pattern: filter.from_pattern.clone(),
                        subject_keywords: filter.subject_keywords.clone(),
                        target_label: filter.target_label_id.clone(),
                        should_archive: filter.should_archive,
                        estimated_matches: filter.estimated_matches,
                        gmail_query,
                    });
                    filters_created += 1;
                }
            }

            let filter_action = if dry_run { "Would create" } else { "Created" };
            reporter.finish_spinner(&filter_spinner, &format!("{} {} filters", filter_action, filters_created));
            state.checkpoint(&cli.state_file).await?;

            (filters_created, planned_filters)
        } else {
            (0, Vec::new())
        };

        // Step 10: Apply labels to existing messages
        state.phase = ProcessingPhase::ApplyingLabels;
        state.save(&cli.state_file).await?;

        let apply_bar = reporter.add_progress_bar(
            classifications.len() as u64,
            "Applying labels to messages...",
        );

        let mut messages_modified = 0;
        let mut messages_archived = 0;

        for (msg, classification) in &classifications {
            // Get label ID for this category
            let label_name = format!("{:?}", classification.category);

            if !dry_run {
                if let Ok(label_id) = label_manager.get_or_create_label(&label_name).await {
                    client.apply_label(&msg.id, &label_id).await?;
                    messages_modified += 1;

                    if classification.should_archive {
                        messages_archived += 1;
                    }
                }
            } else {
                tracing::info!(
                    "[DRY RUN] Would label message {} with {}",
                    msg.id,
                    label_name
                );
                messages_modified += 1;
                if classification.should_archive {
                    messages_archived += 1;
                }
            }

            apply_bar.inc(1);
        }

        apply_bar.finish_with_message(format!(
            "Applied labels to {} messages",
            messages_modified
        ));

        state.messages_modified = messages_modified;
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

        // Build planned changes for dry run mode
        let planned_changes = if dry_run {
            Some(PlannedChanges {
                labels: planned_labels.clone(),
                filters: planned_filters,
                messages_to_label: messages_modified,
                messages_to_archive: messages_archived,
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
            messages_modified,
            messages_archived,
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
