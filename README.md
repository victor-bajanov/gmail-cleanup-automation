# Gmail Automation System

A high-performance, production-ready email organization system that automatically classifies, labels, and filters your Gmail inbox using rule-based pattern matching and optional machine learning.

[![Rust](https://img.shields.io/badge/rust-1.70%2B-orange.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

## Overview

This system scans your Gmail inbox, intelligently classifies emails into categories, creates hierarchical labels, and sets up automatic filters for future emails. It's designed for users overwhelmed by email clutter who want automated, intelligent organization without manual effort.

**Key Benefits:**
- Process thousands of emails in minutes
- Automatic categorization of newsletters, receipts, notifications, and more
- Zero configuration required - smart defaults work out of the box
- Safe dry-run mode to preview changes before applying
- Resume from interruptions with automatic checkpointing
- Production-grade error handling and rate limiting

---

## Features

### Core Capabilities
- **Automatic Email Classification**: Rule-based pattern matching identifies 7 email categories
- **Hierarchical Label Management**: Creates and manages nested Gmail labels with configurable prefixes
- **Smart Filter Generation**: Automatically creates Gmail filters for future email routing
- **Batch Processing**: Concurrent API requests with rate limiting (40 requests/sec by default)
- **State Management**: Checkpoint every 100 messages with resume capability
- **Progress Tracking**: Real-time progress bars and detailed execution reports

### Email Categories
The system classifies emails into these categories:

| Category | Description | Examples |
|----------|-------------|----------|
| **Newsletter** | Regular content subscriptions | TechCrunch Daily, Medium Digest |
| **Receipt** | Purchase confirmations and invoices | Amazon orders, Stripe receipts |
| **Notification** | Account alerts and updates | GitHub notifications, Slack alerts |
| **Marketing** | Promotional content | Sales, special offers, campaigns |
| **Shipping** | Delivery tracking and updates | FedEx, UPS, DHL notifications |
| **Financial** | Bank statements and transactions | Credit card bills, payment confirmations |
| **Personal** | Direct human correspondence | Friend emails, 1-on-1 messages |

### Safety Features
- **Dry-run Mode**: Preview all changes without modifying your inbox
- **Interactive Mode**: Confirm each major action before execution
- **Rollback Support**: Undo changes from previous runs (coming soon)
- **Rate Limit Protection**: Automatic backoff and retry on API limits
- **Error Recovery**: Graceful handling of transient failures

---

## Architecture

The system follows a modular architecture with clear separation of concerns:

```
┌───────────────┐
│    CLI/Main   │  Command-line interface and orchestration
└───────┬───────┘
        │
        ├──────────────┬──────────────┬──────────────┬──────────────┬──────────────┐
        ▼              ▼              ▼              ▼              ▼              ▼
┌────────────┐ ┌────────────┐ ┌────────────┐ ┌────────────┐ ┌────────────┐ ┌────────────┐
│  Scanner   │ │ Classifier │ │Interactive │ │ Exclusions │ │ Label Mgr  │ │ Filter Mgr │
│ (Messages) │ │ (Rules/ML) │ │  (Review)  │ │ (Persist)  │ │(Hierarchy) │ │ (AutoRule) │
└────────────┘ └────────────┘ └────────────┘ └────────────┘ └────────────┘ └────────────┘
        │              │              │              │              │              │
        └──────────────┴──────────────┴──────┬───────┴──────────────┴──────────────┘
                                             ▼
                                     ┌──────────────┐
                                     │ Gmail Client │  Rate limiting, retries, API calls
                                     │  (with Auth) │
                                     └──────────────┘
```

**Module Responsibilities:**
- **Scanner**: Queries Gmail for messages matching time periods and criteria
- **Classifier**: Applies pattern matching rules to categorize emails
- **Interactive Review**: Clusters emails by sender, presents review UI, saves decisions
- **Exclusions**: Manages permanently excluded clusters (persisted to JSON)
- **Label Manager**: Creates/updates hierarchical labels in Gmail
- **Filter Manager**: Generates and creates Gmail filter rules
- **Client**: Handles all Gmail API communication with rate limiting

---

## Prerequisites

### System Requirements
- **Rust**: Version 1.70 or higher
- **Operating System**: Linux, macOS, or Windows
- **Network**: Internet connection for Gmail API access

### Google Cloud Setup
You'll need a Google Cloud project with Gmail API enabled:

1. **Gmail Account**: A Google account with Gmail enabled
2. **OAuth2 Credentials**: Client ID and secret for desktop application
3. **API Access**: Gmail API enabled in Google Cloud Console

---

## Installation

### 1. Clone the Repository

```bash
git clone https://github.com/yourusername/gmail-automation.git
cd gmail-automation
```

### 2. Build the Project

```bash
cargo build --release
```

The binary will be available at `target/release/gmail-automation`.

### 3. Install System-Wide (Recommended)

```bash
cargo install --path .
```

This makes the `gmail-automation` command available globally.

### Running the Tool

After installation, you can run the tool in two ways:

| Method | Command | When to use |
|--------|---------|-------------|
| **Installed binary** | `gmail-automation <command>` | Recommended for regular use |
| **Via Cargo** | `cargo run --release -- <command>` | For development/testing |

**Examples:**
```bash
# Using installed binary (recommended)
gmail-automation auth
gmail-automation run --dry-run

# Using cargo (for developers)
cargo run --release -- auth
cargo run --release -- run --dry-run
```

> **Note for non-Rust users:** You only need Rust installed to build the project once. After running `cargo install --path .`, you can use `gmail-automation` directly without any Rust knowledge.

---

## Google Cloud Setup

### Step 1: Create a Google Cloud Project

1. Go to [Google Cloud Console](https://console.cloud.google.com/)
2. Click **Select a project** → **New Project**
3. Enter a project name (e.g., "Gmail Automation")
4. Click **Create**

### Step 2: Enable Gmail API

1. In your project, go to **APIs & Services** → **Library**
2. Search for "Gmail API"
3. Click **Gmail API** → **Enable**

### Step 3: Create OAuth2 Credentials

1. Go to **APIs & Services** → **Credentials**
2. Click **+ CREATE CREDENTIALS** → **OAuth client ID**
3. If prompted, configure the OAuth consent screen:
   - User Type: **External**
   - App name: "Gmail Automation"
   - User support email: Your email
   - Developer contact: Your email
   - Scopes: Add `https://www.googleapis.com/auth/gmail.modify`
4. Back in credentials, select **Application type**: **Desktop app**
5. Name: "Gmail Automation Client"
6. Click **Create**

### Step 4: Download Credentials

1. Click the **Download JSON** button (⬇️) next to your OAuth 2.0 Client ID
2. Save the file as `credentials.json` in the project root directory

**Important**: Keep `credentials.json` secure - it contains your OAuth client secret.

---

## Configuration

### Generate Example Configuration

```bash
gmail-automation init-config
```

This creates a `config.toml` file with sensible defaults.

### Configuration Structure

```toml
[scan]
# How many days of email history to scan (1-365)
period_days = 90

# Maximum concurrent API requests (1-50)
# Higher = faster but risks rate limits
max_concurrent_requests = 40

[classification]
# Classification mode: 'rules', 'ml', or 'hybrid'
mode = "rules"

# LLM provider for ML mode: 'openai', 'anthropic', or 'anthropic-agents'
llm_provider = "openai"

# Minimum emails from a sender to create a filter
minimum_emails_for_label = 5

# Optional: Claude Agents SDK configuration
[classification.claude_agents]
enabled = false
use_advanced_analysis = true
max_iterations = 3

[labels]
# Prefix for all created labels (e.g., "AutoManaged/Newsletter")
prefix = "AutoManaged"

# Categories to auto-archive after labeling
auto_archive_categories = ["newsletters", "notifications", "marketing"]

[execution]
# Enable dry-run mode by default (prevents changes)
dry_run = false
```

### Key Settings

| Setting | Default | Description |
|---------|---------|-------------|
| `scan.period_days` | 90 | How far back to scan (1-365 days) |
| `scan.max_concurrent_requests` | 40 | Concurrent API calls (1-50) |
| `classification.mode` | "rules" | Classification engine to use |
| `classification.minimum_emails_for_label` | 5 | Min emails to create filter |
| `labels.prefix` | "AutoManaged" | Label prefix for organization |
| `labels.auto_archive_categories` | `["newsletters", ...]` | Categories to auto-archive |

---

## Usage

### Authentication

First, authenticate with Gmail API:

```bash
gmail-automation auth
```

This opens your browser for OAuth consent. After authorization, a token is cached at `.gmail-automation/token.json`.

**Force re-authentication** (if token expires):

```bash
gmail-automation auth --force
```

### Run the Full Pipeline

Execute the complete email management workflow:

```bash
gmail-automation run
```

**What it does:**
1. Scans emails from the last 90 days (configurable)
2. Classifies each email into categories
3. **Interactive Review** - Review each sender cluster and decide: Accept, Reject, or Custom label
4. Creates hierarchical labels in Gmail
5. Generates and creates filter rules
6. Applies labels to existing messages
7. Generates a summary report

### Interactive Review Mode (Default)

By default, the tool enters an interactive review mode where you review each email cluster:

```
┌──────────────────────────────────────────────────────────────────────────────┐
│ Progress: [█████████████████████████████████████████░░░░░░░░]  53/60 clusters│
├──────────────────────────────────────────────────────────────────────────────┤
│ CLUSTER: no-reply@spotify.com (specific sender) (5 emails)                   │
├──────────────────────────────────────────────────────────────────────────────┤
│ Proposed filter rule:                                                        │
│   Query:   from:(no-reply@spotify.com)                                       │
│   Label:   AutoManaged/other/spotify-com                                     │
│   Archive: YES                                                               │
├──────────────────────────────────────────────────────────────────────────────┤
│ Sample subjects:                                                             │
│   • 🎫 AC/DC recently announced a show near you! 🎫                           │
│   • The Tiger Lillies, Amber Run live: personalized concert recommendations  │
│   • Start exploring venues on Spotify                                        │
├──────────────────────────────────────────────────────────────────────────────┤
│ [Y] Create filter  [N] No filter  [S] Skip for now                           │
│ [A] Toggle archive [L] Change label         [?] Help                         │
└──────────────────────────────────────────────────────────────────────────────┘
```

**Keyboard Shortcuts:**

| Key | Action | Description |
|-----|--------|-------------|
| **Decisions (new clusters)** |||
| `Y` / `Enter` | Create filter | Accept with shown label & archive setting |
| `N` | No filter | Leave these emails alone (no filter created) |
| `S` | Skip | Don't decide now, come back later |
| **Decisions (existing filters)** |||
| `Y` / `Enter` | Update filter | Update to proposed label & archive setting |
| `N` | Keep as-is | Leave existing filter unchanged |
| `S` | Skip | Don't decide now (keeps current filter) |
| `D` | Delete filter | Remove the existing filter from Gmail |
| `Shift+S` | Skip all existing | Jump past all existing filters to new clusters |
| **Edit before accepting** |||
| `A` | Toggle archive | Switch auto-archive ON/OFF |
| `L` | Change label | Enter a different target label |
| **Permanent exclusion** |||
| `E` | Exclude permanently | Never show this cluster again (saved to file) |
| **Navigation** |||
| `U` | Undo | Go back to previous decision |
| `?` | Help | Show keyboard shortcuts |
| `Q` | Quit | Exit without saving changes |
| `W` | Write | Save all changes (shown at end) |
| `Ctrl+C` | Force quit | Exit immediately |

**How Clusters Are Created:**

The system uses hierarchical clustering to group emails, from most specific to broadest:

```
1. Subject-based clusters (most specific)
   └─ Same sender + same subject pattern
   └─ Example: "QNAP NAS Notification" emails from victor@gmail.com

2. Sender-based clusters
   └─ Same sender email, any subject
   └─ Example: All emails from no-reply@spotify.com

3. Domain-based clusters (broadest)
   └─ Same domain, excludes senders with their own clusters
   └─ Example: *@linkedin.com (excluding specific senders above)
```

**Resulting Filter Patterns:**

| Cluster Type | Gmail Filter Query | Example |
|--------------|-------------------|---------|
| Subject + Sender | `from:(sender@domain) subject:(pattern)` | `from:(victor@gmail.com) subject:(QNAP NAS Notification)` |
| Specific Sender | `from:(sender@domain)` | `from:(no-reply@spotify.com)` |
| Domain (with exclusions) | `from:(*@domain) -from:(excluded1) -from:(excluded2)` | `from:(*@linkedin.com) -from:(jobs@linkedin.com)` |
| Domain (no exclusions) | `from:(*@domain)` | `from:(*@airbnb.com)` |

Clusters are presented in order of specificity (narrowest first), so you review the most targeted filters before broader domain-wide ones.

**Existing Filter Detection:**

When you run the tool, it automatically detects existing Gmail filters that match your email clusters. These are shown **first** in the review queue with a color-coded comparison:

```
┌──────────────────────────────────────────────────────────────────────────────┐
│ Progress: [████████░░░░░░░░░░░░]  5/20 clusters (3 existing, 17 new)         │
├──────────────────────────────────────────────────────────────────────────────┤
│ CLUSTER: *@linkedin.com (12 emails)                                          │
├──────────────────────────────────────────────────────────────────────────────┤
│ ⚠ EXISTING FILTER - [S] keeps current, [Y] updates to proposed               │
├──────────────────────────────────────────────────────────────────────────────┤
│   Current:  Label: AutoManaged/notifications    Archive: NO                  │
│   Proposed: Label: AutoManaged/marketing        Archive: YES                 │
├──────────────────────────────────────────────────────────────────────────────┤
│ [Y] Update filter  [N] Keep as-is  [S] Skip (keep current)                   │
│ [D] DELETE filter  [E] Exclude permanently  [?] Help                         │
│ [A] Toggle archive [L] Change label  [Shift+S] Skip all existing             │
└──────────────────────────────────────────────────────────────────────────────┘
```

**Color coding** (in terminal):
- **Grey**: Same value in current and proposed
- **Red**: Label differs between current and proposed
- **Blue**: Archive setting differs

Use `Shift+S` to skip all remaining existing filter clusters and jump directly to reviewing new clusters.

**Permanent Exclusions:**

Press `E` to permanently exclude a cluster from future reviews. This is useful for senders you know you'll never want to filter (e.g., personal contacts, important services).

- Exclusions are saved to `.gmail-automation/exclusions.json`
- Excluded clusters won't appear in future runs
- If you exclude a cluster with an existing filter, that filter will be deleted
- Use `--ignore-exclusions` to see all clusters again (useful for reconsidering)

```bash
# Normal run - excluded clusters are hidden
gmail-automation run

# See all clusters, including previously excluded ones
gmail-automation run --ignore-exclusions
```

**Skip the review** (auto-accept all suggestions):

```bash
gmail-automation run --no-review
```

### Dry-Run Mode (Recommended First Run)

Preview changes without modifying your inbox:

```bash
gmail-automation run --dry-run
```

This shows exactly what would happen without making any actual changes.

### Confirmation Mode

Add Y/N confirmation prompts before each major phase:

```bash
gmail-automation run --interactive
```

You'll be prompted before:
- Creating labels
- Creating filters

Note: This is different from the review mode. You can combine both:

```bash
gmail-automation run --interactive  # Review + confirmations
gmail-automation run --no-review --interactive  # No review, but confirmations
```

### Labels Only Mode

Create labels without setting up filters:

```bash
gmail-automation run --labels-only
```

Useful if you want manual control over filter creation.

### Resume from Interruption

If processing is interrupted, resume from the last checkpoint:

```bash
gmail-automation run --resume
```

The system saves state including:
- Scan progress (every 100 messages)
- Review decisions (saved to `decisions.json`)
- Created labels and filters

You can resume from any phase including label and filter creation.

### Check Status

View the status of current or previous runs:

```bash
gmail-automation status
```

**Detailed status** (shows failed messages):

```bash
gmail-automation status --detailed
```

### Command-Line Options

**Global options** (all commands):

```bash
--config <PATH>        # Path to config file (default: config.toml)
--credentials <PATH>   # Path to OAuth credentials (default: credentials.json)
--token-cache <PATH>   # Token cache location (default: .gmail-automation/token.json)
--state-file <PATH>    # State file location (default: .gmail-automation/state.json)
--verbose              # Enable debug logging
```

**Run command options** (`gmail-automation run`):

```bash
--dry-run              # Preview changes without modifying Gmail
--no-review            # Skip interactive review, auto-accept all suggestions
--interactive          # Add Y/N confirmation prompts before each phase
--labels-only          # Only create labels, skip filter creation
--resume               # Resume from previous interrupted run
--ignore-exclusions    # Show all clusters, including permanently excluded ones
```

**Example with custom paths:**

```bash
gmail-automation --config my-config.toml --credentials auth/creds.json run --dry-run
```

**Example workflow:**

```bash
# First: dry run to see what would happen
gmail-automation run --dry-run

# Then: full run with review (default)
gmail-automation run

# Or: skip review and auto-accept all
gmail-automation run --no-review
```

---

## How It Works

### Pipeline Execution Flow

```
1. Configuration Loading
   ├─ Load config.toml (or use defaults)
   └─ Validate all settings

2. Authentication
   ├─ Load cached OAuth token
   ├─ Refresh if expired
   └─ Authenticate if needed (browser flow)

3. Email Scanning
   ├─ Query Gmail for messages in time period
   ├─ Fetch message metadata (concurrent batches)
   └─ Extract sender, subject, headers

4. Classification & Clustering
   ├─ Apply rule-based pattern matching
   ├─ Identify email category
   ├─ Group emails by sender (with subject patterns)
   └─ Suggest labels and archive settings

5. Interactive Review (default, skip with --no-review)
   ├─ Present each cluster for review
   ├─ Accept/Reject/Custom decisions
   ├─ Save decisions to decisions.json
   └─ Resume-safe: decisions persist across crashes

6. Label Creation
   ├─ Generate hierarchical label structure
   ├─ Create labels in Gmail (if not exists)
   └─ Track created label IDs

7. Filter Generation
   ├─ Convert review decisions to filter rules
   ├─ Check for existing filters (idempotent)
   ├─ Create filter rules with retry logic
   └─ Apply labels retroactively to matching emails

8. Report Generation
   ├─ Calculate statistics
   ├─ Generate markdown report
   └─ Save to .gmail-automation/report-{run_id}.md
```

### Classification Algorithm

The rule-based classifier uses a scoring system:

1. **Sender Analysis**: Check for automated patterns (`noreply@`, `notifications@`, etc.)
2. **Domain Detection**: Identify commercial ESPs (SendGrid, Mailchimp, etc.)
3. **Subject Patterns**: Match keywords using regex (receipt, invoice, shipping, etc.)
4. **Header Analysis**: Check for List-Unsubscribe headers (newsletters)
5. **Known Services**: Match against database of known senders

**Scoring:** Each matched pattern adds confidence points. The category with the highest score wins.

---

## Project Structure

```
gmail-automation/
├── src/
│   ├── main.rs              # Entry point, CLI orchestration
│   ├── lib.rs               # Library exports
│   ├── cli.rs               # CLI commands and progress reporting
│   ├── config.rs            # Configuration loading and validation
│   ├── auth.rs              # OAuth2 authentication flow
│   ├── client.rs            # Gmail API client with rate limiting
│   ├── scanner.rs           # Email scanning and querying
│   ├── classifier.rs        # Rule-based classification engine
│   ├── interactive.rs       # Interactive review UI and cluster decisions
│   ├── exclusions.rs        # Persistent exclusion management
│   ├── label_manager.rs     # Label creation and hierarchy
│   ├── filter_manager.rs    # Filter rule generation and creation
│   ├── state.rs             # State management and checkpointing
│   ├── error.rs             # Error types and handling
│   └── models.rs            # Data structures and types
├── tests/
│   ├── common/
│   │   └── mod.rs           # Test utilities and mocks
│   └── integration_tests.rs # End-to-end integration tests
├── Cargo.toml               # Rust dependencies and metadata
├── config.toml              # User configuration (generated)
├── credentials.json         # OAuth2 credentials (user-provided)
└── .gmail-automation/       # Runtime data directory
    ├── token.json           # Cached OAuth token
    ├── state.json           # Processing state
    ├── decisions.json       # Saved review decisions (for resume)
    ├── exclusions.json      # Permanently excluded clusters
    └── report-*.md          # Execution reports
```

---

## Development

### Build

```bash
# Development build (faster compilation, slower runtime)
cargo build

# Release build (optimized)
cargo build --release
```

### Run Tests

```bash
# Run all tests
cargo test --all-features

# Run specific test module
cargo test scanner

# Run with output
cargo test -- --nocapture
```

### Linting

```bash
# Check code quality
cargo clippy --all-features

# Auto-fix issues
cargo clippy --all-features --fix
```

### Code Coverage

```bash
# Using tarpaulin
cargo install cargo-tarpaulin
cargo tarpaulin --all-features --out Html
```

### Format Code

```bash
cargo fmt
```

---

## Performance

### Throughput

- **Scanning**: ~500-1000 messages/minute (limited by API quota)
- **Classification**: ~10,000 messages/second (CPU-bound, no API calls)
- **Label Creation**: ~50 labels/second (API-limited)
- **Filter Creation**: ~50 filters/second (API-limited)

### API Rate Limits

Gmail API quotas (per-user, per-project):

- **Quota units**: 250 units/second
- **Per-user rate limit**: 25,000 quota units/day
- **Cost per operation**:
  - `messages.list`: 5 quota units
  - `messages.get`: 5 quota units
  - `labels.create`: 5 quota units
  - `settings.filters.create`: 5 quota units

**The system automatically respects these limits with:**
- Concurrent request limiting (default: 40 requests)
- Exponential backoff on rate limit errors (429)
- Automatic retry on transient failures

### Memory Usage

- **Typical**: 50-100 MB for processing 10,000 emails
- **Peak**: ~200 MB during concurrent batch operations
- **State file**: ~1 KB per 1,000 messages

### Checkpointing

State is automatically saved:
- Every 100 messages processed
- After each major phase (scanning, classification, labeling, filtering)
- On graceful shutdown
- Before and after API operations

---

## Safety Features

### Dry-Run Mode

The safest way to preview changes:

```bash
gmail-automation run --dry-run
```

**What it does:**
- Reads all emails and performs classification
- Shows exactly which labels would be created
- Displays which filters would be generated
- Prints what actions would be taken
- **Makes ZERO changes to your Gmail account**

### Interactive Confirmations

Get prompted before each major action:

```bash
gmail-automation run --interactive
```

You can review and approve:
1. Label creation (with category breakdown)
2. Filter creation (with rule preview)
3. Message modifications (with count)

### State Persistence

All operations are tracked in state files:

```json
{
  "run_id": "uuid",
  "phase": "ApplyingLabels",
  "messages_scanned": 5000,
  "messages_classified": 5000,
  "labels_created": ["label_id_1", "label_id_2"],
  "filters_created": ["filter_id_1"],
  "checkpoint_count": 50
}
```

### Rollback Support (Coming Soon)

Future releases will support:
- `gmail-automation rollback` - Undo last run
- `gmail-automation rollback --run-id <ID>` - Undo specific run
- `gmail-automation rollback --labels-only` - Only remove labels
- `gmail-automation rollback --filters-only` - Only remove filters

---

## Troubleshooting

### Authentication Issues

**Problem**: `Error: Authentication failed`

**Solutions:**
1. Ensure `credentials.json` is in the project root
2. Check credentials are for "Desktop app" type
3. Try force re-authentication: `gmail-automation auth --force`
4. Verify Gmail API is enabled in Google Cloud Console

---

**Problem**: `Error: Token expired`

**Solution:**
```bash
# Force re-authentication
gmail-automation auth --force
```

The token automatically refreshes, but force re-auth helps if refresh fails.

---

### Rate Limit Errors

**Problem**: `Error: Rate limit exceeded (429)`

**Solutions:**
1. Reduce concurrent requests in `config.toml`:
   ```toml
   [scan]
   max_concurrent_requests = 20  # Lower from 40
   ```
2. Wait 1-2 minutes before retrying
3. The system automatically retries with backoff

---

### Configuration Errors

**Problem**: `Error: Failed to parse config file`

**Solutions:**
1. Regenerate config: `gmail-automation init-config --force`
2. Check TOML syntax (use a validator)
3. Ensure all values are in valid ranges:
   - `period_days`: 1-365
   - `max_concurrent_requests`: 1-50
   - `minimum_emails_for_label`: >= 1

---

### Processing Interrupted

**Problem**: Process stopped midway through

**Solution:**
```bash
# Resume from last checkpoint
gmail-automation run --resume
```

The system automatically saves state every 100 messages.

---

### No Emails Found

**Problem**: Scanner reports 0 messages

**Solutions:**
1. Check `period_days` in config (default: 90)
2. Verify your inbox has emails in that time range
3. Try a longer period: `period_days = 365`
4. Check Gmail API permissions are granted

---

### Classification Seems Wrong

**Problem**: Emails categorized incorrectly

**Solutions:**
1. Review classification patterns in `src/classifier.rs`
2. Run in dry-run mode to inspect results
3. Consider adjusting `minimum_emails_for_label` threshold
4. Open an issue with example emails (anonymized)

---

### Out of Memory

**Problem**: `Error: Cannot allocate memory`

**Solutions:**
1. Reduce `max_concurrent_requests` in config
2. Process in smaller time periods (reduce `period_days`)
3. Increase system memory or use swap space
4. Close other memory-intensive applications

---

## Security Best Practices

1. **Protect credentials.json**: Never commit to version control
2. **Token security**: Keep `.gmail-automation/token.json` private
3. **Scope limitations**: Only request `gmail.modify` scope (not `gmail.readonly` + modify)
4. **OAuth consent**: Review permissions during authentication
5. **Credentials rotation**: Regenerate credentials periodically
6. **Dry-run first**: Always test with `--dry-run` before production runs

---

## Contributing

Contributions are welcome! Please:

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/amazing-feature`)
3. Commit your changes (`git commit -m 'Add amazing feature'`)
4. Push to the branch (`git push origin feature/amazing-feature`)
5. Open a Pull Request

**Before submitting:**
- Run tests: `cargo test --all-features`
- Run clippy: `cargo clippy --all-features`
- Format code: `cargo fmt`
- Update documentation if needed

---

## License

This project is licensed under the [MIT License](LICENSE).

---

## Acknowledgments

- [google-gmail1](https://github.com/Byron/google-apis-rs) - Gmail API client
- [yup-oauth2](https://github.com/dermesser/yup-oauth2) - OAuth2 implementation
- [tokio](https://tokio.rs/) - Async runtime
- Gmail API documentation and community

---

## Support

- **Issues**: [GitHub Issues](https://github.com/yourusername/gmail-automation/issues)
- **Discussions**: [GitHub Discussions](https://github.com/yourusername/gmail-automation/discussions)
- **Email**: your.email@example.com

---

## Roadmap

### Version 0.5.0
- [ ] Rollback functionality
- [ ] ML-based classification (OpenAI, Anthropic)
- [ ] Advanced pattern learning from user behavior
- [ ] Web UI for configuration and monitoring

### Version 0.6.0
- [ ] Multi-account support
- [ ] Custom rule definitions via config
- [ ] Email statistics and analytics
- [ ] Scheduled automatic runs

### Version 1.0.0
- [ ] Production deployment guide
- [ ] Docker container support
- [ ] Cloud deployment options (AWS, GCP)
- [ ] Comprehensive ML model training

---

## FAQ

**Q: Will this delete my emails?**
A: No. The system only creates labels and filters. It can archive emails (move out of inbox) but never deletes them.

**Q: Can I undo changes?**
A: Currently, you need to manually remove labels and filters. Automatic rollback is coming in v0.2.0.

**Q: How much does it cost?**
A: The software is free and open source. Google Cloud Console is free for personal use within quotas.

**Q: Does it work with G Suite / Google Workspace?**
A: Yes, as long as Gmail API is enabled for your organization.

**Q: Can I run this on a schedule?**
A: Not yet built-in. You can use cron/systemd to run periodically. Automatic scheduling is planned for v0.3.0.

**Q: Is my data private?**
A: Yes. All processing happens locally on your machine. No data is sent to third parties (except Gmail API).

**Q: What if I hit API limits?**
A: The system automatically handles rate limits with backoff and retry. You can also reduce `max_concurrent_requests`.

---

## Changelog

### [0.4.0] - 2025-12-01

**Existing Filter Management**
- Detect and display existing Gmail filters that match email clusters
- Show existing filters first in review queue with comparison display
- Color-coded diff: grey (same), red (label differs), blue (archive differs)
- Add `D` key to delete existing filters from Gmail
- Add `Shift+S` to skip all remaining existing filters and jump to new clusters
- Progress indicator shows existing/new cluster breakdown

**Permanent Exclusions**
- Add `E` key to permanently exclude clusters from future reviews
- Exclusions saved to `.gmail-automation/exclusions.json`
- Excluded clusters automatically filtered on subsequent runs
- Add `--ignore-exclusions` flag to see all clusters (including excluded)
- If excluded cluster has existing filter, it gets deleted

**Bug Fixes**
- Fix case-insensitive label lookups (409 conflict on existing labels)
- Fix Reject decisions incorrectly generating filters on resume

### [0.3.0] - 2025-11-28

**Resume & Retry Improvements**
- Save review decisions to JSON file for resume capability
- Add retry with exponential backoff for all Gmail API filter/label operations
- Enable resume from CreatingLabels and CreatingFilters phases
- Idempotent filter creation - skip duplicates on resume by checking Gmail
- Comprehensive tests for new resume/retry functionality

**Data Integrity Fixes**
- Fix decision_map key to include subject_pattern (prevents cluster collisions)
- Fix existing filter matching to check subject pattern
- Fix deduplication key to include excluded_senders, should_archive, target_label_id
- Fix matches_filter_rule() to compare subject keywords
- Fix normalize_subject() for case-insensitive prefix removal
- Fix cluster_key() to include subject pattern (prevents HashMap collisions)

**UX Improvements**
- Make interactive review mode the default (use `--no-review` to skip)
- Better error messages for resume failures showing resumable phases
- Progress bars coordinate with tracing logs

### [0.2.0] - 2025-11-27

**Performance & Reliability**
- Batch API optimization for labels and filters
- Skip creating duplicate Gmail filters with deep predicate comparison
- Batch archive emails using Gmail batchModify API
- Coordinate tracing logs with progress bars

### [0.1.0] - 2025-11-24

**Initial Release**
- Rule-based email classification
- Hierarchical label management
- Automatic filter generation
- Dry-run and interactive modes
- State management with checkpointing
- Progress tracking with real-time updates
- Comprehensive error handling
- Full test coverage

---

**Built with ❤️ in Rust**
