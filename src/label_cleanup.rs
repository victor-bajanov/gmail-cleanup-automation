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
