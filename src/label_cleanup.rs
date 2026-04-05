use std::collections::HashMap;
use std::io::{self, Write as IoWrite};

use crossterm::{
    cursor, queue,
    style,
    terminal::{self, Clear, ClearType},
};

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

/// Shared status for async operations.
#[derive(Debug, Default)]
pub struct ApplyStatus {
    pub applied: std::sync::atomic::AtomicUsize,
    pub in_flight: std::sync::atomic::AtomicUsize,
    pub failed: std::sync::atomic::AtomicUsize,
    pub last_action: std::sync::Mutex<Option<String>>,
}

/// Render the two-pane TUI for a single email.
///
/// Left pane shows email details and label choices.
/// Right pane (22 chars wide) shows apply status.
/// Controls at the bottom: [0] Remove all  [s] Skip  [u] Undo  [q] Quit
pub fn render_email(
    email: &OverlabeledEmail,
    current: usize,
    total: usize,
    status: &ApplyStatus,
    undo_len: usize,
) -> io::Result<()> {
    let mut out = io::stdout();
    let (cols, rows) = terminal::size().unwrap_or((80, 24));

    if cols < 40 || rows < 10 {
        queue!(out, cursor::MoveTo(0, 0), Clear(ClearType::All))?;
        queue!(out, style::Print("Terminal too small"))?;
        out.flush()?;
        return Ok(());
    }

    let right_w: u16 = 22;
    let left_w = cols.saturating_sub(right_w + 1);

    queue!(out, cursor::MoveTo(0, 0), Clear(ClearType::All))?;

    // ── Header ──
    let header = format!(
        " Email {}/{} — Label Cleanup",
        current + 1,
        total
    );
    queue!(out, cursor::MoveTo(0, 0), style::Print(&header))?;

    // ── Left pane: email details ──
    let mut row: u16 = 2;

    let from = format!("From: {}", email.message.sender_email);
    queue!(out, cursor::MoveTo(1, row), style::Print(truncate(&from, left_w as usize)))?;
    row += 1;

    let subj = format!("Subject: {}", email.message.subject);
    queue!(out, cursor::MoveTo(1, row), style::Print(truncate(&subj, left_w as usize)))?;
    row += 1;

    let date = format!("Date: {}", email.message.date_received.format("%Y-%m-%d %H:%M"));
    queue!(out, cursor::MoveTo(1, row), style::Print(truncate(&date, left_w as usize)))?;
    row += 2;

    queue!(out, cursor::MoveTo(1, row), style::Print("Labels:"))?;
    row += 1;

    for (i, (_id, display)) in email.auto_labels.iter().enumerate() {
        if let Some(key) = key_for_index(i) {
            let line = format!("  [{}] {}", key, display);
            queue!(out, cursor::MoveTo(1, row), style::Print(truncate(&line, left_w as usize)))?;
            row += 1;
        }
    }

    // ── Right pane: status ──
    let rx = left_w + 1;
    let applied = status.applied.load(std::sync::atomic::Ordering::Relaxed);
    let in_flight = status.in_flight.load(std::sync::atomic::Ordering::Relaxed);
    let failed = status.failed.load(std::sync::atomic::Ordering::Relaxed);

    queue!(out, cursor::MoveTo(rx, 2), style::Print("── Status ──"))?;
    queue!(out, cursor::MoveTo(rx, 3), style::Print(format!("Applied:   {}", applied)))?;
    queue!(out, cursor::MoveTo(rx, 4), style::Print(format!("In-flight: {}", in_flight)))?;
    queue!(out, cursor::MoveTo(rx, 5), style::Print(format!("Failed:    {}", failed)))?;
    queue!(out, cursor::MoveTo(rx, 6), style::Print(format!("Undo stack: {}", undo_len)))?;

    if let Ok(guard) = status.last_action.lock() {
        if let Some(ref action) = *guard {
            let msg = truncate(action, right_w as usize);
            queue!(out, cursor::MoveTo(rx, 8), style::Print(msg))?;
        }
    }

    // ── Controls ──
    let ctrl_row = rows.saturating_sub(2);
    let controls = "[0] Remove all  [s] Skip  [u] Undo  [q] Quit";
    queue!(out, cursor::MoveTo(1, ctrl_row), style::Print(controls))?;

    out.flush()?;
    Ok(())
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else if max > 3 {
        format!("{}...", &s[..max - 3])
    } else {
        s[..max].to_string()
    }
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
