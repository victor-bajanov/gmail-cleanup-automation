use anyhow::Result;
use clap::Parser;
use gmail_automation::cli::{self, Cli, Commands};
use gmail_automation::client::GmailClient;
use gmail_automation::config::Config;
use gmail_automation::error::GmailError;
use indicatif::MultiProgress;
use std::io::Write;
use std::process;
use std::sync::Arc;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::EnvFilter;

/// A writer that prints through MultiProgress to avoid progress bar conflicts
#[derive(Clone)]
struct MultiProgressWriter {
    multi: Arc<MultiProgress>,
    buffer: Arc<std::sync::Mutex<Vec<u8>>>,
}

impl MultiProgressWriter {
    fn new(multi: Arc<MultiProgress>) -> Self {
        Self {
            multi,
            buffer: Arc::new(std::sync::Mutex::new(Vec::new())),
        }
    }
}

impl Write for MultiProgressWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut buffer = self.buffer.lock().unwrap();
        buffer.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        let mut buffer = self.buffer.lock().unwrap();
        if !buffer.is_empty() {
            let msg = String::from_utf8_lossy(&buffer);
            // Remove trailing newline for cleaner output
            let msg = msg.trim_end_matches('\n');
            if !msg.is_empty() {
                let _ = self.multi.println(msg);
            }
            buffer.clear();
        }
        Ok(())
    }
}

impl Drop for MultiProgressWriter {
    fn drop(&mut self) {
        let _ = self.flush();
    }
}

/// MakeWriter implementation for tracing
#[derive(Clone)]
struct MultiProgressMakeWriter {
    multi: Arc<MultiProgress>,
}

impl MultiProgressMakeWriter {
    fn new(multi: Arc<MultiProgress>) -> Self {
        Self { multi }
    }
}

impl<'a> MakeWriter<'a> for MultiProgressMakeWriter {
    type Writer = MultiProgressWriter;

    fn make_writer(&'a self) -> Self::Writer {
        MultiProgressWriter::new(Arc::clone(&self.multi))
    }
}

#[tokio::main]
async fn main() {
    // Exit with proper code on error
    if let Err(e) = run().await {
        eprintln!("Error: {}", e);
        eprintln!("\nFor help, run: gmail-filters --help");
        process::exit(1);
    }
}

async fn run() -> Result<()> {
    // Install default crypto provider for rustls
    // This is necessary because multiple dependencies use different crypto providers
    // On non-Windows platforms, use aws-lc-rs (better performance, FIPS support)
    // On Windows, use ring (better compatibility, no NASM/CMake required)
    #[cfg(not(windows))]
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .map_err(|_| anyhow::anyhow!("Failed to install default crypto provider"))?;

    #[cfg(windows)]
    rustls::crypto::ring::default_provider()
        .install_default()
        .map_err(|_| anyhow::anyhow!("Failed to install default crypto provider"))?;

    // Parse CLI arguments
    let cli = Cli::parse();

    // Initialize tracing with level based on verbose flag
    let filter = if cli.verbose {
        EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new("gmail_automation=debug,info"))
    } else {
        EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new("gmail_automation=info,warn,error"))
    };

    // Create shared MultiProgress for coordinated progress bar + logging
    let multi_progress = Arc::new(MultiProgress::new());
    let make_writer = MultiProgressMakeWriter::new(Arc::clone(&multi_progress));

    // Set up tracing with MultiProgress writer - logs will print above progress bars
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(make_writer)
        .with_target(false)
        .with_thread_ids(false)
        .with_file(false)
        .with_line_number(false)
        .init();

    tracing::info!("Gmail automation system starting...");

    // Ensure .gmail-automation directory exists for all file operations
    tokio::fs::create_dir_all(".gmail-automation").await?;

    // Execute command
    match cli.command {
        Commands::Auth { force } => {
            tracing::info!("Authenticating with Gmail API...");

            // Ensure token cache directory exists
            if let Some(parent) = cli.token_cache.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }

            // Delete existing token if force flag is set
            if force && cli.token_cache.exists() {
                tokio::fs::remove_file(&cli.token_cache).await?;
                tracing::info!("Removed existing token cache");
            }

            // Initialize Gmail hub (will trigger OAuth flow if needed)
            let hub =
                gmail_automation::auth::initialize_gmail_hub(&cli.credentials, &cli.token_cache)
                    .await?;

            println!("Successfully authenticated with Gmail API");
            println!("Token cached at: {:?}", cli.token_cache);

            // Test the connection - must specify scope to avoid triggering additional OAuth flow
            let (_, profile) = hub
                .users()
                .get_profile("me")
                .add_scope("https://www.googleapis.com/auth/gmail.modify")
                .doit()
                .await?;
            println!(
                "Connected to account: {}",
                profile.email_address.unwrap_or_default()
            );

            Ok(())
        }

        Commands::Run {
            dry_run,
            labels_only,
            interactive,
            no_review,
            resume,
            ignore_exclusions,
            ref apply_decisions,
        } => {
            tracing::info!("Starting full pipeline run");
            if dry_run {
                println!("Running in DRY RUN mode - no changes will be made");
            }
            if labels_only {
                println!("Running in LABELS ONLY mode - filters will not be created");
            }
            if no_review {
                println!("Running with review mode DISABLED");
            }
            if ignore_exclusions {
                println!("Running with exclusions IGNORED - all clusters will be shown");
            }
            if let Some(ref path) = apply_decisions {
                println!("Applying decisions from: {:?}", path);
            }

            // Run the complete pipeline (clone the inner MultiProgress, not the Arc)
            // Review mode is enabled by default; pass !no_review
            let report = cli::run_pipeline(
                &cli,
                dry_run,
                labels_only,
                interactive,
                !no_review,
                resume,
                ignore_exclusions,
                apply_decisions.clone(),
                (*multi_progress).clone(),
            )
            .await?;

            // Display summary
            println!("\n========================================");
            println!("Pipeline Execution Summary");
            println!("========================================");
            println!("Run ID: {}", report.run_id);
            println!("Duration: {} seconds", report.duration_seconds);
            println!("Emails scanned: {}", report.emails_scanned);
            println!("Emails classified: {}", report.emails_classified);
            println!("Labels created: {}", report.labels_created);
            println!("Filters created: {}", report.filters_created);
            println!("Messages modified: {}", report.messages_modified);
            println!("Messages archived: {}", report.messages_archived);
            // Cleanup stats
            if report.hierarchy_labels_created > 0 || report.orphaned_labels_deleted > 0
                || report.filters_deleted > 0 || report.messages_cleaned > 0
            {
                println!("----------------------------------------");
                println!("Cleanup:");
                if report.hierarchy_labels_created > 0 {
                    println!("  Hierarchy labels repaired: {}", report.hierarchy_labels_created);
                }
                if report.orphaned_labels_deleted > 0 {
                    println!("  Orphaned labels deleted: {}", report.orphaned_labels_deleted);
                }
                if report.filters_deleted > 0 {
                    println!("  Filters deleted: {}", report.filters_deleted);
                }
                if report.messages_cleaned > 0 {
                    println!("  Messages cleaned: {}", report.messages_cleaned);
                }
            }
            println!("========================================");

            Ok(())
        }

        Commands::Rollback {
            run_id: _,
            labels_only: _,
            filters_only: _,
            force: _,
        } => {
            tracing::info!("Rollback command (not yet implemented)");
            println!("Rollback functionality coming soon!");
            println!("This will allow you to undo changes from a previous run.");

            // TODO: Implement rollback logic
            // 1. Load rollback log
            // 2. Remove created filters
            // 3. Remove created labels
            // 4. Remove labels from messages

            Ok(())
        }

        Commands::Status { detailed } => {
            tracing::info!("Checking status...");

            // Load current state if exists
            if cli.state_file.exists() {
                let state = gmail_automation::state::ProcessingState::load(&cli.state_file).await?;

                println!("\n========================================");
                println!("Processing State");
                println!("========================================");
                println!("Run ID: {}", state.run_id);
                println!("Started: {}", state.started_at.format("%Y-%m-%d %H:%M:%S"));
                println!("Updated: {}", state.updated_at.format("%Y-%m-%d %H:%M:%S"));
                println!("Phase: {:?}", state.phase);
                println!("Completed: {}", state.completed);
                println!("Messages scanned: {}", state.messages_scanned);
                println!("Messages classified: {}", state.messages_classified);
                println!("Labels created: {}", state.labels_created.len());
                println!("Filters created: {}", state.filters_created.len());
                println!("Messages modified: {}", state.messages_modified);
                println!("Checkpoints: {}", state.checkpoint_count);

                if detailed {
                    println!("\n--- Detailed Information ---");
                    if let Some(last_msg) = &state.last_processed_message_id {
                        println!("Last processed message: {}", last_msg);
                    }
                    if !state.failed_message_ids.is_empty() {
                        println!("\nFailed messages: {}", state.failed_message_ids.len());
                        for id in &state.failed_message_ids {
                            println!("  - {}", id);
                        }
                    }
                }
                println!("========================================");
            } else {
                println!("No active or previous runs found.");
                println!("State file: {:?}", cli.state_file);
            }

            Ok(())
        }

        Commands::InitConfig { output, force } => {
            tracing::info!("Generating example configuration file");

            // Check if file exists
            if output.exists() && !force {
                return Err(GmailError::ConfigError(format!(
                    "Configuration file already exists at {:?}. Use --force to overwrite.",
                    output
                ))
                .into());
            }

            // Create example config
            Config::create_example(&output).await?;

            println!("Created example configuration file at: {:?}", output);
            println!("\nPlease edit this file to customize your settings.");
            println!("Key settings to review:");
            println!("  - scan.period_days: How many days of email history to scan");
            println!("  - classification.mode: 'rules', 'ml', or 'hybrid'");
            println!("  - labels.prefix: Prefix for all created labels");
            println!("  - labels.auto_archive_categories: Categories to auto-archive");

            Ok(())
        }

        Commands::Unmanage {
            dry_run,
            delete_labels,
            force,
        } => {
            tracing::info!("Starting unmanage operation");

            // Set up progress reporting
            let multi = MultiProgress::new();
            let reporter = cli::ProgressReporter::with_multi_progress(multi);

            if dry_run {
                let _ = reporter
                    .multi_progress()
                    .println("Running in DRY RUN mode - no changes will be made");
            }

            // Load configuration
            let config_spinner = reporter.add_spinner("Loading configuration...");
            let config = Config::load(&cli.config).await?;
            let label_prefix = &config.labels.prefix;
            reporter.finish_spinner(
                &config_spinner,
                &format!("Configuration loaded (prefix: {})", label_prefix),
            );

            // Initialize Gmail API
            let auth_spinner = reporter.add_spinner("Authenticating with Gmail API...");
            let hub =
                gmail_automation::auth::initialize_gmail_hub(&cli.credentials, &cli.token_cache)
                    .await?;
            reporter.finish_spinner(&auth_spinner, "Gmail API authenticated");

            let client = gmail_automation::client::ProductionGmailClient::with_full_config(
                hub,
                config.scan.max_concurrent_requests,
                250.0, // quota units per second
                500.0, // quota burst capacity
                config.circuit_breaker.clone(),
            );

            // Fetch filters and labels concurrently (independent API calls)
            let fetch_spinner =
                reporter.add_spinner("Fetching existing Gmail filters and labels...");
            let (filters_result, labels_result) =
                tokio::join!(client.list_filters(), client.list_labels());

            let existing_filters = filters_result?;
            let existing_labels = labels_result?;
            let label_id_to_name: std::collections::HashMap<String, String> = existing_labels
                .iter()
                .map(|l| (l.id.clone(), l.name.clone()))
                .collect();
            reporter.finish_spinner(
                &fetch_spinner,
                &format!(
                    "Found {} filters and {} labels",
                    existing_filters.len(),
                    existing_labels.len()
                ),
            );

            // Find filters that add labels with the configured prefix (case-insensitive)
            let prefix_lower = label_prefix.to_lowercase();
            let mut filters_to_delete = Vec::new();
            for filter in &existing_filters {
                // Check if any of the filter's add_label_ids have names starting with our prefix
                for label_id in &filter.add_label_ids {
                    if let Some(label_name) = label_id_to_name.get(label_id) {
                        if label_name.to_lowercase().starts_with(&prefix_lower) {
                            filters_to_delete.push((
                                filter.id.clone(),
                                filter.query.clone(),
                                label_name.clone(),
                            ));
                            break; // Only add filter once even if it has multiple matching labels
                        }
                    }
                }
            }

            // Find labels that start with the configured prefix (case-insensitive)
            let labels_to_delete: Vec<_> = existing_labels
                .iter()
                .filter(|l| l.name.to_lowercase().starts_with(&prefix_lower))
                .collect();

            // Display what will be deleted
            let _ = reporter
                .multi_progress()
                .println("\n========================================");
            let _ = reporter
                .multi_progress()
                .println("Auto-managed items found");
            let _ = reporter
                .multi_progress()
                .println("========================================");

            if filters_to_delete.is_empty() {
                let _ = reporter
                    .multi_progress()
                    .println("\nNo auto-managed filters found.");
            } else {
                let _ = reporter.multi_progress().println(format!(
                    "\nFilters to delete ({}):",
                    filters_to_delete.len()
                ));
                for (id, query, label_name) in &filters_to_delete {
                    let query_display = query.as_ref().map(|q| q.as_str()).unwrap_or("<no query>");
                    let _ = reporter.multi_progress().println(format!(
                        "  - {} -> {} (ID: {})",
                        query_display, label_name, id
                    ));
                }
            }

            if delete_labels {
                if labels_to_delete.is_empty() {
                    let _ = reporter
                        .multi_progress()
                        .println("\nNo auto-managed labels found.");
                } else {
                    let _ = reporter
                        .multi_progress()
                        .println(format!("\nLabels to delete ({}):", labels_to_delete.len()));
                    for label in &labels_to_delete {
                        let _ = reporter
                            .multi_progress()
                            .println(format!("  - {} (ID: {})", label.name, label.id));
                    }
                }
            }

            // If nothing to delete, exit early
            if filters_to_delete.is_empty() && (!delete_labels || labels_to_delete.is_empty()) {
                let _ = reporter
                    .multi_progress()
                    .println("\nNothing to delete. Exiting.");
                return Ok(());
            }

            // Confirm deletion (unless --force or --dry-run)
            if !dry_run && !force {
                // Suspend progress bars for user input
                let _ = reporter
                    .multi_progress()
                    .println("\n⚠️  This action will permanently delete the items listed above!");
                reporter.multi_progress().suspend(|| {
                    print!("Are you sure you want to proceed? [y/N]: ");
                    let _ = std::io::Write::flush(&mut std::io::stdout());
                });

                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;

                if input.trim().to_lowercase() != "y" {
                    let _ = reporter.multi_progress().println("Aborted.");
                    return Ok(());
                }
            }

            // Delete filters
            if !filters_to_delete.is_empty() {
                if dry_run {
                    let _ = reporter.multi_progress().println(format!(
                        "\nWould delete {} filters",
                        filters_to_delete.len()
                    ));
                } else {
                    let pb = reporter
                        .add_progress_bar(filters_to_delete.len() as u64, "Deleting filters...");

                    let mut deleted = 0;
                    let mut failed = 0;
                    for (filter_id, query, _) in &filters_to_delete {
                        match client.delete_filter(filter_id).await {
                            Ok(_) => {
                                deleted += 1;
                                let query_display =
                                    query.as_ref().map(|q| q.as_str()).unwrap_or("<no query>");
                                tracing::debug!("Deleted filter: {}", query_display);
                            }
                            Err(e) => {
                                failed += 1;
                                tracing::warn!("Failed to delete filter {}: {}", filter_id, e);
                            }
                        }
                        pb.inc(1);
                    }
                    pb.finish_with_message(format!(
                        "Deleted {} filters ({} failed)",
                        deleted, failed
                    ));
                }
            }

            // Delete labels (if requested)
            if delete_labels && !labels_to_delete.is_empty() {
                if dry_run {
                    let _ = reporter
                        .multi_progress()
                        .println(format!("\nWould delete {} labels", labels_to_delete.len()));
                } else {
                    let pb = reporter
                        .add_progress_bar(labels_to_delete.len() as u64, "Deleting labels...");

                    let mut deleted = 0;
                    let mut failed = 0;
                    for label in &labels_to_delete {
                        match client.delete_label(&label.id).await {
                            Ok(_) => {
                                deleted += 1;
                                tracing::debug!("Deleted label: {}", label.name);
                            }
                            Err(e) => {
                                failed += 1;
                                tracing::warn!("Failed to delete label {}: {}", label.name, e);
                            }
                        }
                        pb.inc(1);
                    }
                    pb.finish_with_message(format!(
                        "Deleted {} labels ({} failed)",
                        deleted, failed
                    ));
                }
            }

            let _ = reporter
                .multi_progress()
                .println("\n========================================");
            if dry_run {
                let _ = reporter
                    .multi_progress()
                    .println("Dry run complete. No changes were made.");
                let _ = reporter
                    .multi_progress()
                    .println("Run without --dry-run to apply changes.");
            } else {
                let _ = reporter
                    .multi_progress()
                    .println("Unmanage operation complete!");
            }
            let _ = reporter
                .multi_progress()
                .println("========================================");

            Ok(())
        }

        Commands::Remediate { dry_run, no_apply } => {
            tracing::info!("Starting filter remediation");
            if dry_run {
                println!("Running in DRY RUN mode — no changes will be made\n");
            }

            // Load config
            let config = Config::load(&cli.config).await?;

            // Auth
            let hub = gmail_automation::auth::initialize_gmail_hub(
                &cli.credentials,
                &cli.token_cache,
            )
            .await?;

            // Create client (follow Unmanage pattern)
            let client = gmail_automation::client::ProductionGmailClient::with_full_config(
                hub,
                config.scan.max_concurrent_requests,
                250.0,
                500.0,
                config.circuit_breaker.clone(),
            );

            // Build label map
            let labels = client.list_labels().await?;
            let label_map: std::collections::HashMap<String, String> = labels
                .iter()
                .map(|l| (l.id.clone(), l.name.clone()))
                .collect();

            println!("Scanning existing filters for overlaps...");
            let groups = gmail_automation::filter_remediation::OverlapDetector::detect(
                &client, &label_map,
            )
            .await?;

            if groups.is_empty() {
                println!("No overlapping filters found. Nothing to remediate.");
                return Ok(());
            }

            println!("Found {} overlap group(s):\n", groups.len());

            let mut plan = gmail_automation::RemediationPlan::new();

            use crossterm::{
                event::{self, Event, KeyCode, KeyEventKind},
                terminal,
            };

            // Helper: read a single keypress (Escape cancels, errors break)
            fn read_key() -> Option<char> {
                terminal::enable_raw_mode().ok()?;
                let result = loop {
                    match event::read() {
                        Ok(Event::Key(key)) if key.kind == KeyEventKind::Press => {
                            match key.code {
                                KeyCode::Char(c) => break Some(c),
                                KeyCode::Esc => break None,
                                _ => continue,
                            }
                        }
                        Err(_) => break None,
                        _ => continue,
                    }
                };
                terminal::disable_raw_mode().ok();
                result
            }

            for group in groups {
                println!("\n─── {} ───", group.group_id);
                println!("  From: {}", group.from_pattern);
                println!("  Labels: {}", group.label_names.join(", "));
                println!("  Filters: {}", group.filters.len());

                let decision = match &group.resolution_type {
                    gmail_automation::ResolutionType::Consolidate {
                        keep_filter_id,
                        remove_filter_ids,
                    } => {
                        println!(
                            "  Type: Same label — consolidate (delete {} redundant)\n",
                            remove_filter_ids.len()
                        );
                        println!("  [a]ccept  [s]kip");
                        match read_key() {
                            Some('a') => {
                                println!("  → accepted");
                                gmail_automation::GroupDecision::Consolidate {
                                    keep_filter_id: keep_filter_id.clone(),
                                    remove_filter_ids: remove_filter_ids.clone(),
                                }
                            }
                            _ => {
                                println!("  → skipped");
                                gmail_automation::GroupDecision::Skip
                            }
                        }
                    }
                    gmail_automation::ResolutionType::MechanicalFix {
                        proposed_replacements,
                    } => {
                        println!(
                            "  Type: Can be fixed automatically (add subject exclusions)\n"
                        );
                        for r in proposed_replacements {
                            let query =
                                gmail_automation::FilterManager::build_gmail_query_static(r);
                            let label = label_map
                                .get(&r.target_label_id)
                                .map(|s| s.as_str())
                                .unwrap_or(&r.target_label_id);
                            println!("    → {} → {}", query, label);
                        }
                        println!();
                        println!("  [a]ccept fix  [s]kip");
                        match read_key() {
                            Some('a') => {
                                println!("  → accepted");
                                gmail_automation::GroupDecision::ReplaceWithExclusive {
                                    replacement_filters: proposed_replacements.clone(),
                                }
                            }
                            _ => {
                                println!("  → skipped");
                                gmail_automation::GroupDecision::Skip
                            }
                        }
                    }
                    gmail_automation::ResolutionType::PickWinner => {
                        println!(
                            "  Type: Different labels — pick which to keep\n"
                        );

                        for (i, f) in group.filters.iter().enumerate() {
                            let label = f
                                .add_label_ids
                                .first()
                                .and_then(|id| label_map.get(id))
                                .map(|s| s.as_str())
                                .unwrap_or("(unknown)");
                            println!("    [{}] {} (filter {})", i + 1, label, f.id);
                        }
                        println!("    [r]escan  [s]kip");

                        match read_key() {
                            Some('s') => {
                                println!("  → skipped");
                                gmail_automation::GroupDecision::Skip
                            }
                            Some('r') => {
                                println!("  → rescan");
                                gmail_automation::GroupDecision::Rescan {
                                    from_pattern: group.from_pattern.clone(),
                                }
                            }
                            Some(c) if c.is_ascii_digit() => {
                                let idx = (c as u8 - b'0') as usize;
                                if idx >= 1 && idx <= group.filters.len() {
                                    let keep_id = group.filters[idx - 1].id.clone();
                                    println!("  → keep {}", keep_id);
                                    gmail_automation::GroupDecision::KeepOne {
                                        keep_filter_id: keep_id,
                                    }
                                } else {
                                    println!("  → invalid, skipped");
                                    gmail_automation::GroupDecision::Skip
                                }
                            }
                            _ => {
                                println!("  → skipped");
                                gmail_automation::GroupDecision::Skip
                            }
                        }
                    }
                };

                plan.add(group, decision);
            }

            println!("\n{}\n", plan.summary());

            if dry_run {
                println!("Dry run complete. No changes were made.");
                return Ok(());
            }

            println!("Execute this plan? [y]es [n]o");
            let confirm = matches!(read_key(), Some('y'));

            if !confirm {
                println!("Aborted.");
                return Ok(());
            }

            println!("\nExecuting (up to 10 groups concurrently)...");
            let total = plan.groups.len();
            let result = plan.execute_with_progress(&client, |done, group_id| {
                print!("\r  [{}/{}] {:<40}", done, total, group_id);
                use std::io::Write;
                std::io::stdout().flush().ok();
            }).await?;
            println!("\r\nRemediation complete:");
            println!("  Deleted: {} filters", result.deleted.len());
            println!("  Created: {} filters", result.created.len());
            println!("  Skipped: {} groups", result.skipped);
            if !result.errors.is_empty() {
                println!("  Errors:");
                for e in &result.errors {
                    println!("    - {}", e);
                }
            }

            // --- Apply phase: swap labels on existing emails ---
            let swaps = gmail_automation::RemediationApplicator::collect_swaps(&plan);

            if swaps.is_empty() {
                println!("\nNo label changes needed for existing emails.");
            } else if no_apply {
                println!("\nSkipping label application (--no-apply).");
                println!("  {} swap(s) would affect existing emails.", swaps.len());
            } else {
                println!("\nLabel swaps for existing emails:");
                for swap in &swaps {
                    let count = client.list_message_ids(&swap.query).await.unwrap_or_default().len();
                    let remove_names: Vec<&str> = swap.remove_label_ids.iter()
                        .map(|id| label_map.get(id).map(|s| s.as_str()).unwrap_or(id))
                        .collect();
                    let add_name = label_map.get(&swap.add_label_id)
                        .map(|s| s.as_str())
                        .unwrap_or(&swap.add_label_id);
                    println!(
                        "  {} — {} → {} (~{} emails)",
                        swap.query,
                        remove_names.join(", "),
                        add_name,
                        count
                    );
                }

                println!("\nApply label changes? [y]es [n]o");
                let confirm_apply = matches!(read_key(), Some('y'));

                if confirm_apply {
                    let apply_result = gmail_automation::RemediationApplicator::apply(
                        &client, &swaps,
                    )
                    .await?;
                    println!("\nLabel application complete:");
                    println!("  Relabeled: {} messages", apply_result.messages_relabeled);
                    if apply_result.messages_failed > 0 {
                        println!("  Failed: {} messages", apply_result.messages_failed);
                    }
                    if !apply_result.errors.is_empty() {
                        for e in &apply_result.errors {
                            println!("    - {}", e);
                        }
                    }
                } else {
                    println!("Skipped label application.");
                }
            }

            Ok(())
        }
    }
}

/// Display error with context
#[allow(dead_code)]
fn display_error(error: &anyhow::Error) {
    eprintln!("Error: {}", error);

    // Display error chain
    let mut cause = error.source();
    while let Some(e) = cause {
        eprintln!("  Caused by: {}", e);
        cause = e.source();
    }

    // Display helpful hints based on error type
    if let Some(gmail_err) = error.downcast_ref::<GmailError>() {
        match gmail_err {
            GmailError::AuthError(_) => {
                eprintln!("\nHint: Make sure your credentials.json file is valid.");
                eprintln!("      You can download it from Google Cloud Console.");
                eprintln!("      Try running: gmail-filters auth --force");
            }
            GmailError::ApiError(_) => {
                eprintln!("\nHint: This may be a temporary API error.");
                eprintln!("      Try running the command again.");
            }
            GmailError::RateLimitError(_) => {
                eprintln!("\nHint: You've hit Gmail API rate limits.");
                eprintln!("      Wait a few seconds and try again.");
                eprintln!("      Consider reducing max_concurrent_requests in config.");
            }
            GmailError::ConfigError(_) => {
                eprintln!("\nHint: Check your configuration file for errors.");
                eprintln!("      Run: gmail-filters init-config --force");
            }
            _ => {}
        }
    }
}
