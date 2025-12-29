//! Cluster review commands

use crate::events::{AppHandleExt, ClusterEvent, ClusterEventType};
use crate::state::AppState;
use gmail_automation::{ClusterDecision, DecisionAction, EmailCluster};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

/// View model for a cluster (serializable to frontend)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterView {
    /// Cluster index
    pub index: usize,
    /// Sender pattern (email or domain)
    pub sender_pattern: String,
    /// Number of emails in cluster
    pub email_count: usize,
    /// Suggested label
    pub suggested_label: String,
    /// Whether to archive
    pub should_archive: bool,
    /// Sample subjects from the cluster
    pub sample_subjects: Vec<String>,
    /// Sample sender emails
    pub sample_senders: Vec<String>,
    /// Whether this cluster has an existing filter
    pub has_existing_filter: bool,
    /// Existing filter info (if any)
    pub existing_filter_label: Option<String>,
    /// Whether this cluster has been decided
    pub decided: bool,
    /// Decision action (if decided)
    pub decision: Option<String>,
}

impl From<&EmailCluster> for ClusterView {
    fn from(cluster: &EmailCluster) -> Self {
        Self {
            index: 0, // Set by caller
            sender_pattern: cluster.sender_pattern.clone(),
            email_count: cluster.messages.len(),
            suggested_label: cluster.suggested_label.clone(),
            should_archive: cluster.should_archive,
            sample_subjects: cluster
                .messages
                .iter()
                .take(5)
                .map(|m| m.subject.clone())
                .collect(),
            sample_senders: cluster
                .messages
                .iter()
                .map(|m| m.sender_email.clone())
                .collect::<std::collections::HashSet<_>>()
                .into_iter()
                .take(5)
                .collect(),
            has_existing_filter: cluster.existing_filter_id.is_some(),
            existing_filter_label: cluster.existing_filter_label.clone(),
            decided: false,
            decision: None,
        }
    }
}

/// Review session summary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewSummary {
    /// Total clusters
    pub total: usize,
    /// Clusters accepted
    pub accepted: usize,
    /// Clusters rejected
    pub rejected: usize,
    /// Clusters skipped
    pub skipped: usize,
    /// Clusters marked for deletion
    pub deleted: usize,
    /// Clusters excluded
    pub excluded: usize,
    /// Remaining to review
    pub remaining: usize,
}

/// Gets all clusters for review
#[tauri::command]
pub async fn get_clusters(
    min_emails: Option<usize>,
    state: State<'_, AppState>,
) -> Result<Vec<ClusterView>, String> {
    let clusters = state.get_clusters();
    let decisions = state.get_decisions();
    let min = min_emails.unwrap_or(1);

    let decision_map: std::collections::HashMap<usize, &ClusterDecision> =
        decisions.iter().map(|d| (d.cluster_index, d)).collect();

    let views: Vec<ClusterView> = clusters
        .iter()
        .enumerate()
        .filter(|(_, c)| c.messages.len() >= min)
        .map(|(i, c)| {
            let mut view = ClusterView::from(c);
            view.index = i;

            if let Some(decision) = decision_map.get(&i) {
                view.decided = true;
                view.decision = Some(format!("{:?}", decision.action));
            }

            view
        })
        .collect();

    Ok(views)
}

/// Gets a specific cluster by index
#[tauri::command]
pub async fn get_cluster(
    index: usize,
    state: State<'_, AppState>,
) -> Result<ClusterView, String> {
    let clusters = state.get_clusters();
    let decisions = state.get_decisions();

    let cluster = clusters
        .get(index)
        .ok_or_else(|| format!("Cluster {} not found", index))?;

    let mut view = ClusterView::from(cluster);
    view.index = index;

    if let Some(decision) = decisions.iter().find(|d| d.cluster_index == index) {
        view.decided = true;
        view.decision = Some(format!("{:?}", decision.action));
    }

    Ok(view)
}

/// Decision input from frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionInput {
    /// Cluster index
    pub cluster_index: usize,
    /// Action type
    pub action: String,
    /// Custom label (for CustomLabel action)
    pub custom_label: Option<String>,
    /// Override archive setting
    pub archive: Option<bool>,
}

/// Submits a decision for a cluster
#[tauri::command]
pub async fn submit_cluster_decision(
    app: AppHandle,
    input: DecisionInput,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    let clusters = state.get_clusters();

    let cluster = clusters
        .get(input.cluster_index)
        .ok_or_else(|| format!("Cluster {} not found", input.cluster_index))?;

    // Parse action
    let action = match input.action.to_lowercase().as_str() {
        "accept" | "y" => DecisionAction::Accept,
        "reject" | "n" => DecisionAction::Reject,
        "skip" | "s" => DecisionAction::Skip,
        "delete" | "d" => DecisionAction::Delete,
        "exclude" | "e" => DecisionAction::ExcludeForever,
        "customlabel" | "custom" | "l" => {
            let label = input
                .custom_label
                .ok_or("Custom label required for CustomLabel action")?;
            DecisionAction::CustomLabel(label)
        }
        _ => return Err(format!("Unknown action: {}", input.action)),
    };

    // Create decision
    let decision = ClusterDecision {
        cluster_index: input.cluster_index,
        action,
        should_archive: input.archive.unwrap_or(cluster.should_archive),
        target_label: cluster.suggested_label.clone(),
    };

    // Store decision
    state.add_decision(decision);

    // Emit event
    app.events().emit_cluster(ClusterEvent {
        event_type: ClusterEventType::DecisionMade,
        cluster_index: input.cluster_index,
        data: None,
    });

    Ok(true)
}

/// Undoes the last decision
#[tauri::command]
pub async fn undo_last_decision(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<usize>, String> {
    if let Some(undone) = state.undo_last_decision() {
        app.events().emit_cluster(ClusterEvent {
            event_type: ClusterEventType::DecisionUndone,
            cluster_index: undone.cluster_index,
            data: None,
        });
        Ok(Some(undone.cluster_index))
    } else {
        Ok(None)
    }
}

/// Gets the review summary
#[tauri::command]
pub async fn get_review_summary(state: State<'_, AppState>) -> Result<ReviewSummary, String> {
    let clusters = state.get_clusters();
    let decisions = state.get_decisions();

    let mut accepted = 0;
    let mut rejected = 0;
    let mut skipped = 0;
    let mut deleted = 0;
    let mut excluded = 0;

    for decision in &decisions {
        match &decision.action {
            DecisionAction::Accept => accepted += 1,
            DecisionAction::Reject => rejected += 1,
            DecisionAction::Skip => skipped += 1,
            DecisionAction::Delete => deleted += 1,
            DecisionAction::ExcludeForever => excluded += 1,
            DecisionAction::CustomLabel(_) => accepted += 1,
        }
    }

    let decided_indices: std::collections::HashSet<_> =
        decisions.iter().map(|d| d.cluster_index).collect();

    let remaining = clusters
        .iter()
        .enumerate()
        .filter(|(i, _)| !decided_indices.contains(i))
        .count();

    Ok(ReviewSummary {
        total: clusters.len(),
        accepted,
        rejected,
        skipped,
        deleted,
        excluded,
        remaining,
    })
}

/// Gets all decisions
#[tauri::command]
pub async fn get_decisions(state: State<'_, AppState>) -> Result<Vec<ClusterDecision>, String> {
    Ok(state.get_decisions())
}

/// Clears all decisions
#[tauri::command]
pub async fn clear_decisions(state: State<'_, AppState>) -> Result<(), String> {
    state.clear_decisions();
    Ok(())
}

/// Gets the next undecided cluster index
#[tauri::command]
pub async fn get_next_undecided_cluster(state: State<'_, AppState>) -> Result<Option<usize>, String> {
    let clusters = state.get_clusters();
    let decisions = state.get_decisions();

    let decided_indices: std::collections::HashSet<_> =
        decisions.iter().map(|d| d.cluster_index).collect();

    for (i, _) in clusters.iter().enumerate() {
        if !decided_indices.contains(&i) {
            return Ok(Some(i));
        }
    }

    Ok(None)
}
