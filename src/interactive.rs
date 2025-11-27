//! Interactive cluster review UI for email classification
//!
//! Provides a terminal-based interface for reviewing and adjusting
//! email classifications with minimal keystrokes.

use crate::error::{GmailError, Result};
use crate::models::{Classification, EmailCategory, MessageMetadata};
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{self, ClearType},
};
use std::collections::HashMap;
use std::io::{self, Write};

/// A cluster of emails from the same sender domain
#[derive(Debug, Clone)]
pub struct EmailCluster {
    pub sender_domain: String,
    pub sender_email: String,
    pub message_ids: Vec<String>,
    pub suggested_category: EmailCategory,
    pub suggested_label: String,
    pub confidence: f32,
    pub sample_subjects: Vec<String>,
    pub should_archive: bool,
}

impl EmailCluster {
    pub fn email_count(&self) -> usize {
        self.message_ids.len()
    }
}

/// Decision made by user for a cluster
#[derive(Debug, Clone)]
pub struct ClusterDecision {
    pub sender_domain: String,
    pub message_ids: Vec<String>,
    pub label: String,
    pub should_archive: bool,
    pub action: DecisionAction,
}

/// Type of decision action
#[derive(Debug, Clone, PartialEq)]
pub enum DecisionAction {
    Accept,
    Reject,
    Custom(String),
    Skip,
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
}

impl ReviewSession {
    /// Create a new review session with clusters to review
    pub fn new(clusters: Vec<EmailCluster>) -> Self {
        // Extract unique suggested labels for the label picker
        let mut labels: Vec<String> = clusters
            .iter()
            .map(|c| c.suggested_label.clone())
            .collect();
        labels.sort();
        labels.dedup();

        Self {
            clusters,
            decisions: HashMap::new(),
            current_index: 0,
            deferred_indices: Vec::new(),
            history: Vec::new(),
            available_labels: labels,
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
            execute!(stdout, terminal::Clear(ClearType::All), cursor::MoveTo(0, 0))
                .map_err(|e| GmailError::Unknown(format!("Terminal error: {}", e)))?;

            self.display_current(&mut stdout)?;
            stdout.flush().map_err(|e| GmailError::Unknown(e.to_string()))?;

            // Wait for key input
            if let Event::Key(key_event) = event::read()
                .map_err(|e| GmailError::Unknown(format!("Input error: {}", e)))?
            {
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

        // Box width (inner content width, not including borders)
        const W: usize = 60;

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
            if len >= W {
                format!("│ {} │", chars.iter().take(W).collect::<String>())
            } else {
                format!("│ {}{} │", content, " ".repeat(W - len))
            }
        };

        // Progress bar
        let progress_width = 30;
        let filled = (reviewed * progress_width) / total.max(1);
        let bar: String = (0..progress_width)
            .map(|i| if i < filled { '█' } else { '░' })
            .collect();

        let top    = format!("┌{}┐", "─".repeat(W + 2));
        let mid    = format!("├{}┤", "─".repeat(W + 2));
        let bottom = format!("└{}┘", "─".repeat(W + 2));

        out!("{}", top);
        out!("{}", line(&format!("Progress: [{}] {:>3}/{:<3} clusters", bar, reviewed, total)));
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
            out!("{}", line("Press [W] to write changes, [Q] to quit without saving"));
        } else {
            let cluster = &self.clusters[self.current_index];
            let archive_status = if cluster.should_archive { "ON" } else { "OFF" };
            let confidence_pct = (cluster.confidence * 100.0) as u32;
            let category_str = format!("{:?}", cluster.suggested_category);

            out!("{}", line(&format!("CLUSTER: {}", truncate_str(&cluster.sender_domain, 48))));
            out!("{}", line(&format!("Emails: {:>4} | Suggested: {} ({}%)", cluster.email_count(), category_str, confidence_pct)));
            out!("{}", line(&format!("Archive: {}", archive_status)));
            out!("{}", line(""));
            out!("{}", line("Sample subjects:"));

            for subject in cluster.sample_subjects.iter().take(5) {
                let truncated = truncate_str(subject, 56);
                out!("{}", line(&format!("  • {}", truncated)));
            }

            // Pad remaining lines if fewer than 5 subjects
            for _ in cluster.sample_subjects.len()..5 {
                out!("{}", line(""));
            }

            out!("{}", line(""));
            out!("{}", line("[Y]es  [N]o  [A]rchive  [L]abel  [S]kip  [U]ndo  [?]Help"));
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
            KeyCode::Char('s') => {
                if self.current_index < self.clusters.len() {
                    self.skip_current();
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
            KeyCode::Char('q') => {
                Ok(SessionAction::Quit)
            }
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

    fn accept_current(&mut self) {
        if let Some(cluster) = self.clusters.get(self.current_index) {
            // Save to history for undo
            self.history.push(HistoryEntry {
                index: self.current_index,
                cluster: cluster.clone(),
                decision: self.decisions.get(&cluster.sender_domain).cloned(),
            });

            let decision = ClusterDecision {
                sender_domain: cluster.sender_domain.clone(),
                message_ids: cluster.message_ids.clone(),
                label: cluster.suggested_label.clone(),
                should_archive: cluster.should_archive,
                action: DecisionAction::Accept,
            };

            self.decisions.insert(cluster.sender_domain.clone(), decision);
        }
    }

    fn reject_current(&mut self) {
        if let Some(cluster) = self.clusters.get(self.current_index) {
            self.history.push(HistoryEntry {
                index: self.current_index,
                cluster: cluster.clone(),
                decision: self.decisions.get(&cluster.sender_domain).cloned(),
            });

            let decision = ClusterDecision {
                sender_domain: cluster.sender_domain.clone(),
                message_ids: cluster.message_ids.clone(),
                label: "needs-review".to_string(),
                should_archive: false,
                action: DecisionAction::Reject,
            };

            self.decisions.insert(cluster.sender_domain.clone(), decision);
        }
    }

    fn toggle_archive(&mut self) {
        if let Some(cluster) = self.clusters.get_mut(self.current_index) {
            cluster.should_archive = !cluster.should_archive;
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
                        self.history.push(HistoryEntry {
                            index: self.current_index,
                            cluster: cluster.clone(),
                            decision: self.decisions.get(&cluster.sender_domain).cloned(),
                        });

                        let decision = ClusterDecision {
                            sender_domain: cluster.sender_domain.clone(),
                            message_ids: cluster.message_ids.clone(),
                            label: label.clone(),
                            should_archive: cluster.should_archive,
                            action: DecisionAction::Custom(label),
                        };

                        self.decisions.insert(cluster.sender_domain.clone(), decision);
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
                let decision = ClusterDecision {
                    sender_domain: cluster.sender_domain.clone(),
                    message_ids: cluster.message_ids.clone(),
                    label: cluster.suggested_label.clone(),
                    should_archive: cluster.should_archive,
                    action: DecisionAction::Skip,
                };

                self.decisions.insert(cluster.sender_domain.clone(), decision);
            }
        }
    }

    fn advance(&mut self) {
        self.current_index += 1;

        // Skip clusters we've already decided on
        while self.current_index < self.clusters.len() {
            let domain = &self.clusters[self.current_index].sender_domain;
            if !self.decisions.contains_key(domain) ||
               matches!(self.decisions.get(domain).map(|d| &d.action), Some(DecisionAction::Skip)) {
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

            // Restore or remove decision
            if let Some(prev_decision) = entry.decision {
                self.decisions.insert(entry.cluster.sender_domain.clone(), prev_decision);
            } else {
                self.decisions.remove(&entry.cluster.sender_domain);
            }

            // Go back to that index
            self.current_index = entry.index;

            // Remove from deferred if it was there
            self.deferred_indices.retain(|&i| i != entry.index);
        }
    }

    fn show_help(&self) -> Result<()> {
        let _ = terminal::disable_raw_mode();
        let _ = execute!(io::stdout(), cursor::Show, terminal::Clear(ClearType::All), cursor::MoveTo(0, 0));

        const W: usize = 58;
        let line = |content: &str| {
            let len = content.chars().count();
            if len >= W {
                println!("║ {} ║", content.chars().take(W).collect::<String>());
            } else {
                println!("║ {}{} ║", content, " ".repeat(W - len));
            }
        };

        println!("╔{}╗", "═".repeat(W + 2));
        line("               KEYBOARD SHORTCUTS");
        println!("╠{}╣", "═".repeat(W + 2));
        line("Y / Enter  Accept suggested label and move to next");
        line("N          Reject (mark as needs-review) and move to next");
        line("A          Toggle auto-archive ON/OFF for this cluster");
        line("L          Open label picker for custom label");
        line("S          Skip/defer this cluster for later");
        line("U          Undo last action");
        line("?          Show this help");
        line("Q          Quit without saving");
        line("W          Write changes (only at end)");
        line("Ctrl+C     Force quit");
        println!("╚{}╝", "═".repeat(W + 2));
        println!();
        println!("Press any key to continue...");

        let _ = io::stdout().flush();

        // Wait for any key
        let _ = terminal::enable_raw_mode();
        let _ = event::read();

        Ok(())
    }
}

enum SessionAction {
    Continue,
    Quit,
    Finish,
}

/// Truncate a string to fit within max_len characters (UTF-8 safe)
fn truncate_str(s: &str, max_len: usize) -> String {
    if s.chars().count() <= max_len {
        s.to_string()
    } else {
        format!("{}...", s.chars().take(max_len - 3).collect::<String>())
    }
}

/// Create email clusters from messages and classifications
pub fn create_clusters(
    messages: &[MessageMetadata],
    classifications: &[(MessageMetadata, Classification)],
    min_emails: usize,
) -> Vec<EmailCluster> {
    // Group messages by sender domain
    let mut domain_map: HashMap<String, Vec<(&MessageMetadata, &Classification)>> = HashMap::new();

    for (msg, class) in classifications {
        domain_map
            .entry(msg.sender_domain.clone())
            .or_default()
            .push((msg, class));
    }

    // Convert to clusters
    let mut clusters: Vec<EmailCluster> = domain_map
        .into_iter()
        .filter(|(_, msgs)| msgs.len() >= min_emails)
        .map(|(domain, msgs)| {
            let first = msgs.first().unwrap();
            let message_ids: Vec<String> = msgs.iter().map(|(m, _)| m.id.clone()).collect();
            let sample_subjects: Vec<String> = msgs
                .iter()
                .take(5)
                .map(|(m, _)| m.subject.clone())
                .collect();

            // Calculate average confidence
            let avg_confidence: f32 = msgs.iter().map(|(_, c)| c.confidence).sum::<f32>() / msgs.len() as f32;

            // Use most common category
            let mut category_counts: HashMap<String, usize> = HashMap::new();
            for (_, c) in &msgs {
                *category_counts.entry(format!("{:?}", c.category)).or_insert(0) += 1;
            }
            let suggested_category = first.1.category.clone();

            // Check if majority suggest archiving
            let archive_count = msgs.iter().filter(|(_, c)| c.should_archive).count();
            let should_archive = archive_count > msgs.len() / 2;

            EmailCluster {
                sender_domain: domain,
                sender_email: first.0.sender_email.clone(),
                message_ids,
                suggested_category,
                suggested_label: first.1.suggested_label.clone(),
                confidence: avg_confidence,
                sample_subjects,
                should_archive,
            }
        })
        .collect();

    // Sort by email count (largest first)
    clusters.sort_by(|a, b| b.email_count().cmp(&a.email_count()));

    clusters
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
    fn test_create_clusters() {
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
            message_ids: vec!["1".to_string(), "2".to_string()],
            suggested_category: EmailCategory::Newsletter,
            suggested_label: "auto/newsletters".to_string(),
            confidence: 0.9,
            sample_subjects: vec!["Subject 1".to_string()],
            should_archive: true,
        };

        assert_eq!(cluster.email_count(), 2);
    }
}
