use anyhow::Result;
use clap::Parser;
use gmail_automation::cli::{self, Cli, Commands};
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
    rustls::crypto::aws_lc_rs::default_provider()
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
            let hub = gmail_automation::auth::initialize_gmail_hub(
                &cli.credentials,
                &cli.token_cache,
            )
            .await?;

            println!("Successfully authenticated with Gmail API");
            println!("Token cached at: {:?}", cli.token_cache);

            // Test the connection
            let (_, profile) = hub.users().get_profile("me")
                .add_scope("https://www.googleapis.com/auth/gmail.modify")
                .doit().await?;
            println!("Connected to account: {}", profile.email_address.unwrap_or_default());

            Ok(())
        }

        Commands::Run {
            dry_run,
            labels_only,
            interactive,
            review,
            resume,
        } => {
            tracing::info!("Starting full pipeline run");
            if dry_run {
                println!("Running in DRY RUN mode - no changes will be made");
            }
            if labels_only {
                println!("Running in LABELS ONLY mode - filters will not be created");
            }
            if review {
                println!("Running with INTERACTIVE REVIEW mode enabled");
            }

            // Run the complete pipeline (clone the inner MultiProgress, not the Arc)
            let report =
                cli::run_pipeline(&cli, dry_run, labels_only, interactive, review, resume, (*multi_progress).clone()).await?;

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
            println!("========================================");

            Ok(())
        }

        Commands::Rollback {
            run_id,
            labels_only,
            filters_only,
            force,
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
    }
}

/// Display error with context
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
