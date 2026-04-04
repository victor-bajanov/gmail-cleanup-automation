# Label Cleanup TUI — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build a per-email triage TUI (`label-cleanup` subcommand) that lets users fix emails with 3+ AutoManaged labels by picking a winner or removing all.

**Architecture:** New `src/label_cleanup.rs` module for scan logic (find affected emails, build label maps). TUI rendering and input loop in `src/main.rs` following the existing `remediate` subcommand pattern. Two-pane layout with crossterm cursor positioning. Async apply via `tokio::spawn` with a shared status counter. Undo stack for reverting operations.

**Tech Stack:** Rust, crossterm 0.28 (already a dep), tokio (already a dep), GmailClient trait

---

### Task 1: Add `LabelCleanup` subcommand to CLI

**Files:**
- Modify: `src/cli.rs:42-143` (Commands enum)
- Modify: `src/main.rs:856` (add match arm)

**Step 1: Add the variant to Commands enum**

In `src/cli.rs`, add after the `Remediate` variant (before the closing `}`):

```rust
    /// Clean up emails with too many AutoManaged labels
    LabelCleanup,
```

**Step 2: Add empty match arm in main.rs**

In `src/main.rs`, inside the `match cli.command { ... }` block, add before the closing `}` (around line 856):

```rust
        Commands::LabelCleanup => {
            println!("Label cleanup not yet implemented");
            Ok(())
        }
```

**Step 3: Verify it compiles**

Run: `cargo build 2>&1 | tail -3`
Expected: Compiles successfully

**Step 4: Commit**

```bash
git add src/cli.rs src/main.rs
git commit -m "feat: add label-cleanup subcommand skeleton

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: Create `label_cleanup` module with scan logic

**Files:**
- Create: `src/label_cleanup.rs`
- Modify: `src/lib.rs` (add module declaration)

**Step 1: Write tests for the scan logic**

Create `src/label_cleanup.rs` with the module structure and tests:

```rust
use std::collections::HashMap;

use crate::client::{GmailClient, LabelInfo};
use crate::models::MessageMetadata;

/// An email that has 3+ AutoManaged labels and needs triage.
#[derive(Debug, Clone)]
pub struct OverlabeledEmail {
    pub message: MessageMetadata,
    /// (label_id, display_name) pairs — only the AutoManaged labels
    pub auto_labels: Vec<(String, String)>,
}

/// Result of scanning for overlabeled emails.
#[derive(Debug)]
pub struct ScanResult {
    pub emails: Vec<OverlabeledEmail>,
    pub total_auto_labels: usize,
}

/// Records a label modification so it can be undone.
#[derive(Debug, Clone)]
pub struct UndoEntry {
    pub message_id: String,
    pub added_labels: Vec<String>,
    pub removed_labels: Vec<String>,
}

/// Scan for emails with 3+ AutoManaged labels.
///
/// 1. Fetch all labels, filter to AutoManaged/* prefix.
/// 2. For each auto label, query message IDs.
/// 3. Build message_id -> label_ids map, filter to 3+ labels.
/// 4. Batch-fetch message metadata for affected emails.
/// 5. Sort by sender then date.
pub async fn scan_overlabeled(
    client: &dyn GmailClient,
    auto_prefix: &str,
) -> crate::error::Result<ScanResult> {
    // 1. Fetch all labels, filter to auto-prefix
    let all_labels = client.list_labels().await?;
    let auto_labels: Vec<&LabelInfo> = all_labels
        .iter()
        .filter(|l| l.name.to_lowercase().starts_with(&auto_prefix.to_lowercase()))
        .collect();

    let total_auto_labels = auto_labels.len();

    // 2. For each auto label, query emails
    let mut msg_labels: HashMap<String, Vec<(String, String)>> = HashMap::new();
    for label in &auto_labels {
        // Strip prefix for display name
        let display = label
            .name
            .strip_prefix(auto_prefix)
            .and_then(|s| s.strip_prefix('/'))
            .unwrap_or(&label.name)
            .to_string();

        let query = format!("label:{}", label.id);
        let message_ids = client.list_message_ids(&query).await?;
        for mid in message_ids {
            msg_labels
                .entry(mid)
                .or_default()
                .push((label.id.clone(), display.clone()));
        }
    }

    // 3. Filter to 3+ auto labels
    let affected: Vec<(String, Vec<(String, String)>)> = msg_labels
        .into_iter()
        .filter(|(_, labels)| labels.len() >= 3)
        .collect();

    if affected.is_empty() {
        return Ok(ScanResult {
            emails: vec![],
            total_auto_labels,
        });
    }

    // 4. Batch-fetch message metadata
    let message_ids: Vec<String> = affected.iter().map(|(id, _)| id.clone()).collect();
    let messages = client.fetch_messages_batch(message_ids).await?;

    let msg_map: HashMap<String, MessageMetadata> = messages
        .into_iter()
        .map(|m| (m.id.clone(), m))
        .collect();

    let mut emails: Vec<OverlabeledEmail> = affected
        .into_iter()
        .filter_map(|(id, labels)| {
            msg_map.get(&id).map(|msg| OverlabeledEmail {
                message: msg.clone(),
                auto_labels: labels,
            })
        })
        .collect();

    // 5. Sort by sender then date
    emails.sort_by(|a, b| {
        a.message
            .sender_email
            .cmp(&b.message.sender_email)
            .then(a.message.date_received.cmp(&b.message.date_received))
    });

    Ok(ScanResult {
        emails,
        total_auto_labels,
    })
}

/// Build a display key for a label choice.
/// Returns '1'-'9' for indices 0-8, 'a'-'z' for indices 9-34.
pub fn key_for_index(i: usize) -> Option<char> {
    match i {
        0..=8 => Some((b'1' + i as u8) as char),
        9..=34 => Some((b'a' + (i - 9) as u8) as char),
        _ => None,
    }
}

/// Map a keypress back to a label index.
/// '1'-'9' -> 0-8, 'a'-'z' -> 9-34.
pub fn index_for_key(c: char) -> Option<usize> {
    match c {
        '1'..='9' => Some((c as u8 - b'1') as usize),
        'a'..='z' => Some((c as u8 - b'a') as usize + 9),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_for_index() {
        assert_eq!(key_for_index(0), Some('1'));
        assert_eq!(key_for_index(8), Some('9'));
        assert_eq!(key_for_index(9), Some('a'));
        assert_eq!(key_for_index(10), Some('b'));
        assert_eq!(key_for_index(34), Some('z'));
        assert_eq!(key_for_index(35), None);
    }

    #[test]
    fn test_index_for_key() {
        assert_eq!(index_for_key('1'), Some(0));
        assert_eq!(index_for_key('9'), Some(8));
        assert_eq!(index_for_key('a'), Some(9));
        assert_eq!(index_for_key('z'), Some(34));
        assert_eq!(index_for_key('0'), None);
        assert_eq!(index_for_key('!'), None);
    }

    #[test]
    fn test_key_index_roundtrip() {
        for i in 0..35 {
            let key = key_for_index(i).unwrap();
            assert_eq!(index_for_key(key), Some(i));
        }
    }
}
```

**Step 2: Add module to lib.rs**

In `src/lib.rs`, add after the `filter_remediation` module declaration:

```rust
pub mod label_cleanup;
```

**Step 3: Run tests**

Run: `cargo test --lib label_cleanup::tests`
Expected: ALL PASS (3 tests)

**Step 4: Commit**

```bash
git add src/label_cleanup.rs src/lib.rs
git commit -m "feat: add label_cleanup module with scan logic and key mapping

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: Implement the TUI renderer

**Files:**
- Modify: `src/label_cleanup.rs` (add rendering functions)

**Step 1: Add the TUI rendering function**

Add to `src/label_cleanup.rs`:

```rust
use std::io::{self, Write as IoWrite};
use crossterm::{
    cursor, execute, queue,
    style::{self, Stylize},
    terminal::{self, Clear, ClearType},
};

/// Shared status for async operations.
#[derive(Debug, Default)]
pub struct ApplyStatus {
    pub applied: std::sync::atomic::AtomicUsize,
    pub in_flight: std::sync::atomic::AtomicUsize,
    pub failed: std::sync::atomic::AtomicUsize,
    pub last_action: std::sync::Mutex<Option<String>>,
}

/// Render the two-pane TUI for a single email.
pub fn render_email(
    stdout: &mut io::Stdout,
    email: &OverlabeledEmail,
    index: usize,
    total: usize,
    status: &ApplyStatus,
) -> io::Result<()> {
    use std::sync::atomic::Ordering;

    let (cols, rows) = terminal::size()?;
    let right_pane_width: u16 = 22;
    let left_width = cols.saturating_sub(right_pane_width + 3);

    queue!(stdout, cursor::MoveTo(0, 0), Clear(ClearType::All))?;

    // Top border
    let title = format!(" Email {}/{} ", index + 1, total);
    let left_border = format!(
        "┌─{}─{}┬─ Status {}┐",
        title,
        "─".repeat((left_width as usize).saturating_sub(title.len() + 2)),
        "─".repeat(right_pane_width.saturating_sub(10) as usize),
    );
    queue!(stdout, style::Print(&left_border), cursor::MoveToNextLine(1))?;

    let applied = status.applied.load(Ordering::Relaxed);
    let in_flight = status.in_flight.load(Ordering::Relaxed);
    let failed = status.failed.load(Ordering::Relaxed);
    let last_action = status.last_action.lock().unwrap().clone();

    // Helper to print a row with left content and right status
    let mut row: u16 = 1;
    let mut print_row = |stdout: &mut io::Stdout,
                          left: &str,
                          right: &str|
     -> io::Result<()> {
        let left_truncated = if left.len() > left_width as usize {
            &left[..left_width as usize]
        } else {
            left
        };
        queue!(
            stdout,
            cursor::MoveTo(0, row),
            style::Print(format!(
                "│ {:<width$}│ {:<rwidth$}│",
                left_truncated,
                right,
                width = left_width as usize,
                rwidth = right_pane_width as usize,
            )),
        )?;
        row += 1;
        Ok(())
    };

    // Content rows
    print_row(stdout, "", &format!("✓ {} applied", applied))?;
    print_row(
        stdout,
        &format!("From: {}", email.message.sender_email),
        &format!("⏳ {} in flight", in_flight),
    )?;
    print_row(
        stdout,
        &format!("Subject: {}", email.message.subject),
        &format!("✗ {} failed", failed),
    )?;
    print_row(
        stdout,
        &format!("Date: {}", email.message.date_received.format("%Y-%m-%d")),
        "",
    )?;

    // Last action
    let last_line = last_action.as_deref().unwrap_or("");
    print_row(stdout, "", &format!("Last: {}", last_line))?;

    // Labels header
    print_row(stdout, "Labels:", "")?;

    // Label choices
    for (i, (_label_id, display_name)) in email.auto_labels.iter().enumerate() {
        if let Some(key) = key_for_index(i) {
            print_row(
                stdout,
                &format!("  [{}] {}", key, display_name),
                "",
            )?;
        }
    }

    // Blank rows to fill space
    let used_rows = row + 2; // +2 for bottom controls + border
    let available = (rows as u16).saturating_sub(used_rows);
    for _ in 0..available {
        print_row(stdout, "", "")?;
    }

    // Controls row
    print_row(
        stdout,
        "[0] Remove all  [s] Skip  [u] Undo  [q] Quit",
        "",
    )?;

    // Bottom border
    queue!(
        stdout,
        cursor::MoveTo(0, row),
        style::Print(format!(
            "└{}┴{}┘",
            "─".repeat(left_width as usize + 2),
            "─".repeat(right_pane_width as usize + 2),
        )),
    )?;

    stdout.flush()?;
    Ok(())
}
```

**Step 2: Verify compilation**

Run: `cargo check --lib 2>&1 | tail -3`
Expected: Compiles (may have unused import warnings, that's fine)

**Step 3: Commit**

```bash
git add src/label_cleanup.rs
git commit -m "feat: add two-pane TUI renderer for label cleanup

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: Implement the main event loop with async apply and undo

**Files:**
- Modify: `src/main.rs` (replace the placeholder match arm for LabelCleanup)

**Step 1: Implement the full subcommand handler**

In `src/main.rs`, replace the `Commands::LabelCleanup` match arm with:

```rust
        Commands::LabelCleanup => {
            tracing::info!("Starting label cleanup");

            // Load config
            let config = Config::load(&cli.config).await?;

            // Auth
            let hub = gmail_automation::auth::initialize_gmail_hub(
                &cli.credentials,
                &cli.token_cache,
            )
            .await?;

            let client = Arc::new(
                gmail_automation::client::ProductionGmailClient::with_full_config(
                    hub,
                    config.scan.max_concurrent_requests,
                    250.0,
                    500.0,
                    config.circuit_breaker.clone(),
                ),
            );

            let auto_prefix = config
                .labels
                .as_ref()
                .map(|l| l.prefix.clone())
                .unwrap_or_else(|| "AutoManaged".to_string());

            println!("Scanning for emails with 3+ {} labels...", auto_prefix);

            let scan = gmail_automation::label_cleanup::scan_overlabeled(
                client.as_ref(),
                &auto_prefix,
            )
            .await?;

            if scan.emails.is_empty() {
                println!("No emails with 3+ auto labels found. Nothing to clean up.");
                return Ok(());
            }

            println!(
                "Found {} emails with 3+ auto labels (across {} auto labels total).\n",
                scan.emails.len(),
                scan.total_auto_labels,
            );

            // Enter raw mode for TUI
            use crossterm::{
                event::{self, Event, KeyCode, KeyEventKind},
                terminal,
            };
            use std::sync::atomic::Ordering;

            let status = Arc::new(gmail_automation::label_cleanup::ApplyStatus::default());
            let mut undo_stack: Vec<gmail_automation::label_cleanup::UndoEntry> = Vec::new();
            let mut current: usize = 0;
            let emails = scan.emails;
            let mut stdout = std::io::stdout();

            terminal::enable_raw_mode()?;

            loop {
                if current >= emails.len() {
                    break;
                }

                let email = &emails[current];
                gmail_automation::label_cleanup::render_email(
                    &mut stdout,
                    email,
                    current,
                    emails.len(),
                    &status,
                )?;

                // Read keypress
                let key = loop {
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

                let Some(c) = key else {
                    break; // Esc = quit
                };

                match c {
                    'q' => break,

                    's' => {
                        current += 1;
                    }

                    'u' => {
                        // Undo last action
                        if let Some(entry) = undo_stack.pop() {
                            let client_clone = client.clone();
                            let status_clone = status.clone();
                            status_clone.in_flight.fetch_add(1, Ordering::Relaxed);
                            tokio::spawn(async move {
                                // Reverse: add back removed, remove added
                                let result = client_clone
                                    .batch_modify_labels(
                                        &[entry.message_id],
                                        &entry.removed_labels,  // re-add what was removed
                                        &entry.added_labels,    // remove what was added
                                    )
                                    .await;
                                status_clone.in_flight.fetch_sub(1, Ordering::Relaxed);
                                match result {
                                    Ok(_) => {
                                        *status_clone.last_action.lock().unwrap() =
                                            Some("✓ undone".to_string());
                                    }
                                    Err(_) => {
                                        status_clone.failed.fetch_add(1, Ordering::Relaxed);
                                        *status_clone.last_action.lock().unwrap() =
                                            Some("✗ undo failed".to_string());
                                    }
                                }
                            });
                            if current > 0 {
                                current -= 1;
                            }
                        }
                    }

                    '0' => {
                        // Remove all auto labels
                        let remove_ids: Vec<String> = email
                            .auto_labels
                            .iter()
                            .map(|(id, _)| id.clone())
                            .collect();
                        let msg_id = email.message.id.clone();

                        undo_stack.push(gmail_automation::label_cleanup::UndoEntry {
                            message_id: msg_id.clone(),
                            added_labels: vec![],
                            removed_labels: remove_ids.clone(),
                        });

                        let client_clone = client.clone();
                        let status_clone = status.clone();
                        status_clone.in_flight.fetch_add(1, Ordering::Relaxed);
                        tokio::spawn(async move {
                            let result = client_clone
                                .batch_modify_labels(&[msg_id], &[], &remove_ids)
                                .await;
                            status_clone.in_flight.fetch_sub(1, Ordering::Relaxed);
                            match result {
                                Ok(_) => {
                                    status_clone.applied.fetch_add(1, Ordering::Relaxed);
                                    *status_clone.last_action.lock().unwrap() =
                                        Some("✓ removed all".to_string());
                                }
                                Err(_) => {
                                    status_clone.failed.fetch_add(1, Ordering::Relaxed);
                                    *status_clone.last_action.lock().unwrap() =
                                        Some("✗ remove failed".to_string());
                                }
                            }
                        });
                        current += 1;
                    }

                    c => {
                        // Check if it's a label selection key
                        if let Some(idx) =
                            gmail_automation::label_cleanup::index_for_key(c)
                        {
                            if idx < email.auto_labels.len() {
                                let winner_id = email.auto_labels[idx].0.clone();
                                let winner_name = email.auto_labels[idx].1.clone();
                                let remove_ids: Vec<String> = email
                                    .auto_labels
                                    .iter()
                                    .filter(|(id, _)| *id != winner_id)
                                    .map(|(id, _)| id.clone())
                                    .collect();
                                let msg_id = email.message.id.clone();

                                undo_stack.push(
                                    gmail_automation::label_cleanup::UndoEntry {
                                        message_id: msg_id.clone(),
                                        added_labels: vec![],
                                        removed_labels: remove_ids.clone(),
                                    },
                                );

                                let client_clone = client.clone();
                                let status_clone = status.clone();
                                let winner_display = winner_name.clone();
                                status_clone
                                    .in_flight
                                    .fetch_add(1, Ordering::Relaxed);
                                tokio::spawn(async move {
                                    let result = client_clone
                                        .batch_modify_labels(
                                            &[msg_id],
                                            &[],
                                            &remove_ids,
                                        )
                                        .await;
                                    status_clone
                                        .in_flight
                                        .fetch_sub(1, Ordering::Relaxed);
                                    match result {
                                        Ok(_) => {
                                            status_clone
                                                .applied
                                                .fetch_add(1, Ordering::Relaxed);
                                            *status_clone
                                                .last_action
                                                .lock()
                                                .unwrap() = Some(format!(
                                                "✓ kept {}",
                                                winner_display
                                            ));
                                        }
                                        Err(_) => {
                                            status_clone
                                                .failed
                                                .fetch_add(1, Ordering::Relaxed);
                                            *status_clone
                                                .last_action
                                                .lock()
                                                .unwrap() =
                                                Some("✗ apply failed".to_string());
                                        }
                                    }
                                });
                                current += 1;
                            }
                        }
                    }
                }
            }

            terminal::disable_raw_mode()?;

            // Wait for in-flight operations
            let in_flight = status.in_flight.load(Ordering::Relaxed);
            if in_flight > 0 {
                println!("\nWaiting for {} in-flight operation(s)...", in_flight);
                while status.in_flight.load(Ordering::Relaxed) > 0 {
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
            }

            // Summary
            println!("\nLabel cleanup complete:");
            println!(
                "  Applied: {}",
                status.applied.load(Ordering::Relaxed)
            );
            println!(
                "  Failed: {}",
                status.failed.load(Ordering::Relaxed)
            );
            println!(
                "  Skipped: {}",
                emails.len() - current.min(emails.len())
                    + current
                        .min(emails.len())
                        .saturating_sub(
                            status.applied.load(Ordering::Relaxed)
                                + status.failed.load(Ordering::Relaxed),
                        ),
            );

            Ok(())
        }
```

**Step 2: Verify compilation**

Run: `cargo build 2>&1 | tail -5`
Expected: Compiles successfully

**Step 3: Commit**

```bash
git add src/main.rs
git commit -m "feat: implement label-cleanup TUI event loop with async apply and undo

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>"
```

---

### Task 5: Wire up the config prefix and add label query by ID

**Files:**
- Modify: `src/label_cleanup.rs` (verify `label:<id>` query format works)

Gmail's `messages.list` API accepts `label:<label_id>` in queries. However,
the label ID from Gmail's API is like `Label_123456` — we need to verify this
works. The existing codebase uses `label:` queries elsewhere.

**Step 1: Check if label query format is correct**

Search the codebase for existing `label:` query usage:

Run: `grep -rn "label:" src/ --include="*.rs" | grep -v "test" | grep -v target | head -10`

Verify the format matches what we're using in `scan_overlabeled`. If Gmail
requires the label name (not ID) in queries, we need to adjust.

**Note:** Gmail API `messages.list` accepts `label:<label_id>` directly when
called via the API (not the search bar). Verify this works. If not, use
`labelIds` parameter on the API instead.

**Step 2: If adjustment needed, update `scan_overlabeled`**

If `label:<id>` doesn't work, change the query to use the label name:
```rust
let query = format!("label:{}", label.name.replace(' ', "-"));
```

Or better, add a `list_message_ids_by_label` method to `GmailClient` that
uses the `labelIds` parameter directly.

**Step 3: Run a manual test**

Run: `cargo run -- label-cleanup`
Expected: Should authenticate, scan, and either show emails or say "nothing to clean up"

**Step 4: Commit any fixes**

```bash
git add src/
git commit -m "fix: adjust label query format for label-cleanup scan

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>"
```

---

### Task 6: Polish and edge cases

**Files:**
- Modify: `src/label_cleanup.rs`
- Modify: `src/main.rs`

**Step 1: Handle terminal resize gracefully**

In the render function, if terminal is too small, show a simplified view:

```rust
    let (cols, rows) = terminal::size()?;
    if cols < 40 || rows < 10 {
        queue!(stdout, cursor::MoveTo(0, 0), Clear(ClearType::All))?;
        queue!(stdout, style::Print("Terminal too small. Resize to at least 40x10."))?;
        stdout.flush()?;
        return Ok(());
    }
```

**Step 2: Ensure raw mode is always cleaned up**

Wrap the event loop in the main handler with a guard that disables raw mode
on panic or early return. Add this before entering the loop:

```rust
            // Safety: ensure raw mode is disabled on exit
            struct RawModeGuard;
            impl Drop for RawModeGuard {
                fn drop(&mut self) {
                    let _ = terminal::disable_raw_mode();
                }
            }
            let _guard = RawModeGuard;
            terminal::enable_raw_mode()?;
```

Remove the explicit `terminal::disable_raw_mode()` call later since the
guard handles it.

**Step 3: Run clippy**

Run: `cargo clippy -- -D warnings 2>&1 | tail -10`
Expected: No warnings

**Step 4: Run tests**

Run: `cargo test --lib label_cleanup`
Expected: ALL PASS

**Step 5: Commit**

```bash
git add src/label_cleanup.rs src/main.rs
git commit -m "fix: add terminal resize guard and raw mode safety for label-cleanup

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>"
```

---

### Task 7: Add progress reporting during scan

**Files:**
- Modify: `src/label_cleanup.rs` (add progress callback)
- Modify: `src/main.rs` (pass progress bar)

**Step 1: Add progress callback to scan_overlabeled**

Change the signature to accept a progress callback:

```rust
pub async fn scan_overlabeled(
    client: &dyn GmailClient,
    auto_prefix: &str,
    on_progress: impl Fn(usize, usize, &str),  // (completed, total, label_name)
) -> crate::error::Result<ScanResult> {
```

Call `on_progress(i + 1, auto_labels.len(), &label.name)` after querying
each label.

**Step 2: Wire up progress bar in main.rs**

In the LabelCleanup handler, use `indicatif::ProgressBar`:

```rust
            let pb = indicatif::ProgressBar::new(0);
            pb.set_style(
                indicatif::ProgressStyle::default_bar()
                    .template("{msg} [{bar:30}] {pos}/{len}")
                    .unwrap(),
            );

            let scan = gmail_automation::label_cleanup::scan_overlabeled(
                client.as_ref(),
                &auto_prefix,
                |completed, total, label_name| {
                    pb.set_length(total as u64);
                    pb.set_position(completed as u64);
                    pb.set_message(format!("Scanning {}", label_name));
                },
            )
            .await?;
            pb.finish_and_clear();
```

**Step 3: Verify it works**

Run: `cargo build 2>&1 | tail -3`
Expected: Compiles

**Step 4: Commit**

```bash
git add src/label_cleanup.rs src/main.rs
git commit -m "feat: add progress bar during label-cleanup scan phase

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>"
```

---

### Task 8: Final verification

**Step 1: Run all tests**

Run: `cargo test --lib`
Expected: ALL PASS

**Step 2: Run clippy**

Run: `cargo clippy -- -D warnings`
Expected: Clean

**Step 3: Verify the help text**

Run: `cargo run -- label-cleanup --help`
Expected: Shows help for the label-cleanup subcommand

**Step 4: Commit if any fixes needed**

```bash
git add -A
git commit -m "chore: final polish for label-cleanup feature

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>"
```
