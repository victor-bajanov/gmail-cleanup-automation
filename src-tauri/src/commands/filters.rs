//! Filter management commands

use crate::events::{AppHandleExt, FilterOperation, FilterProgress};
use crate::state::AppState;
use gmail_automation::{DecisionAction, FilterManager, FilterRule, GmailClient};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

/// View model for a filter
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterView {
    /// Filter ID (if existing)
    pub id: Option<String>,
    /// Filter name
    pub name: String,
    /// Gmail query string
    pub query: String,
    /// Target label
    pub label: String,
    /// Whether to archive
    pub archive: bool,
    /// Estimated matches
    pub estimated_matches: usize,
    /// Whether this is a proposed (new) filter
    pub is_proposed: bool,
    /// Change type for diff view
    pub change_type: FilterChangeType,
}

/// Type of change for filter diff
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilterChangeType {
    /// No change
    Unchanged,
    /// New filter
    New,
    /// Modified filter
    Modified,
    /// Deleted filter
    Deleted,
}

/// Filter comparison result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterComparison {
    /// Existing filters
    pub existing: Vec<FilterView>,
    /// Proposed filters
    pub proposed: Vec<FilterView>,
    /// Summary of changes
    pub summary: FilterChangeSummary,
}

/// Summary of filter changes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterChangeSummary {
    /// Number of unchanged filters
    pub unchanged: usize,
    /// Number of new filters
    pub new: usize,
    /// Number of modified filters
    pub modified: usize,
    /// Number of deleted/orphaned filters
    pub deleted: usize,
    /// Total emails affected
    pub total_affected: usize,
}

/// Fetches existing Gmail filters
#[tauri::command]
pub async fn get_existing_filters(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<FilterView>, String> {
    let client = state
        .get_client()
        .ok_or("Gmail client not initialized")?;

    app.events().emit_filter_progress(FilterProgress {
        operation: FilterOperation::FetchingExisting,
        current: 0,
        total: 0,
        filter_name: None,
    });

    let filters = client
        .list_filters()
        .await
        .map_err(|e| format!("Failed to fetch filters: {}", e))?;

    // Store in state
    state.set_existing_filters(filters.clone());

    let views: Vec<FilterView> = filters
        .iter()
        .map(|f| FilterView {
            id: Some(f.id.clone()),
            name: f.id.clone(), // Gmail filters don't have names, use ID
            query: f.query.clone().unwrap_or_default(),
            label: f
                .add_label_ids
                .first()
                .cloned()
                .unwrap_or_default(),
            archive: false, // ExistingFilterInfo doesn't track this
            estimated_matches: 0,
            is_proposed: false,
            change_type: FilterChangeType::Unchanged,
        })
        .collect();

    app.events().emit_filter_progress(FilterProgress {
        operation: FilterOperation::Complete,
        current: views.len(),
        total: views.len(),
        filter_name: None,
    });

    Ok(views)
}

/// Helper to build sender pattern from cluster
fn build_sender_pattern(cluster: &gmail_automation::EmailCluster) -> String {
    if cluster.is_specific_sender && !cluster.sender_email.is_empty() {
        cluster.sender_email.clone()
    } else {
        format!("*@{}", cluster.sender_domain)
    }
}

/// Generates proposed filters from decisions
#[tauri::command]
pub async fn generate_proposed_filters(
    state: State<'_, AppState>,
) -> Result<Vec<FilterView>, String> {
    let decisions = state.get_gui_decisions();
    let clusters = state.get_clusters();
    let config = state.get_config();

    if decisions.is_empty() {
        return Ok(Vec::new());
    }

    let _label_prefix = config
        .as_ref()
        .map(|c| c.labels.prefix.clone())
        .unwrap_or_else(|| "AutoManaged".to_string());

    let mut filters = Vec::new();

    for decision in &decisions {
        // Skip rejected, skipped, excluded, or deleted clusters
        match &decision.action {
            DecisionAction::Reject
            | DecisionAction::Skip
            | DecisionAction::Exclude
            | DecisionAction::Delete => continue,
            _ => {}
        }

        if let Some(cluster) = clusters.get(decision.cluster_index) {
            // Determine label
            let label = match &decision.action {
                DecisionAction::Custom(l) => l.clone(),
                _ => decision.target_label.clone(),
            };

            let sender_pattern = build_sender_pattern(cluster);

            // Build filter rule
            let filter = FilterRule {
                id: None,
                name: format!("{} -> {}", sender_pattern, label),
                from_pattern: Some(sender_pattern.clone()),
                is_specific_sender: cluster.is_specific_sender,
                excluded_senders: cluster.excluded_senders.clone(),
                subject_keywords: vec![],
                excluded_subject_patterns: vec![],
                target_label_id: label.clone(),
                should_archive: decision.should_archive,
                estimated_matches: cluster.message_ids.len(),
            };

            filters.push(filter);
        }
    }

    // Store proposed filters
    state.set_proposed_filters(filters.clone());

    // Convert to views
    let views: Vec<FilterView> = filters
        .iter()
        .map(|f| FilterView {
            id: None,
            name: f.name.clone(),
            query: FilterManager::build_gmail_query_static(f),
            label: f.target_label_id.clone(),
            archive: f.should_archive,
            estimated_matches: f.estimated_matches,
            is_proposed: true,
            change_type: FilterChangeType::New,
        })
        .collect();

    Ok(views)
}

/// Compares existing and proposed filters
#[tauri::command]
pub async fn compare_filters(
    state: State<'_, AppState>,
) -> Result<FilterComparison, String> {
    let existing = state.get_existing_filters();
    let proposed = state.get_proposed_filters();

    let existing_views: Vec<FilterView> = existing
        .iter()
        .map(|f| {
            let query = f.query.clone().unwrap_or_default();
            let from_pattern = extract_from_pattern(&query);

            // Check if there's a matching proposed filter
            let has_proposed_match = proposed.iter().any(|p| {
                p.from_pattern
                    .as_ref()
                    .map(|fp| fp == &from_pattern)
                    .unwrap_or(false)
            });

            FilterView {
                id: Some(f.id.clone()),
                name: f.id.clone(),
                query,
                label: f
                    .add_label_ids
                    .first()
                    .cloned()
                    .unwrap_or_default(),
                archive: false, // ExistingFilterInfo doesn't track this
                estimated_matches: 0,
                is_proposed: false,
                change_type: if has_proposed_match {
                    FilterChangeType::Modified
                } else {
                    FilterChangeType::Unchanged
                },
            }
        })
        .collect();

    let proposed_views: Vec<FilterView> = proposed
        .iter()
        .map(|f| {
            let query = FilterManager::build_gmail_query_static(f);

            // Check if there's an existing filter for this pattern
            let from_pattern = f.from_pattern.clone().unwrap_or_default();
            let has_existing = existing.iter().any(|e| {
                e.query
                    .as_ref()
                    .map(|q| extract_from_pattern(q) == from_pattern)
                    .unwrap_or(false)
            });

            FilterView {
                id: None,
                name: f.name.clone(),
                query,
                label: f.target_label_id.clone(),
                archive: f.should_archive,
                estimated_matches: f.estimated_matches,
                is_proposed: true,
                change_type: if has_existing {
                    FilterChangeType::Modified
                } else {
                    FilterChangeType::New
                },
            }
        })
        .collect();

    // Calculate summary
    let unchanged = existing_views
        .iter()
        .filter(|f| matches!(f.change_type, FilterChangeType::Unchanged))
        .count();
    let new = proposed_views
        .iter()
        .filter(|f| matches!(f.change_type, FilterChangeType::New))
        .count();
    let modified = proposed_views
        .iter()
        .filter(|f| matches!(f.change_type, FilterChangeType::Modified))
        .count();
    let deleted = 0; // TODO: detect orphaned filters
    let total_affected: usize = proposed_views.iter().map(|f| f.estimated_matches).sum();

    Ok(FilterComparison {
        existing: existing_views,
        proposed: proposed_views,
        summary: FilterChangeSummary {
            unchanged,
            new,
            modified,
            deleted,
            total_affected,
        },
    })
}

/// Creates filters from proposed rules
#[tauri::command]
pub async fn apply_filters(
    app: AppHandle,
    dry_run: bool,
    state: State<'_, AppState>,
) -> Result<ApplyFiltersResult, String> {
    let client = state.get_client().ok_or("Gmail client not initialized")?;
    let proposed = state.get_proposed_filters();

    if proposed.is_empty() {
        return Ok(ApplyFiltersResult {
            success: true,
            created: 0,
            failed: 0,
            dry_run,
            errors: vec![],
        });
    }

    let total = proposed.len();
    let mut created = 0;
    let mut errors = Vec::new();

    // Create FilterManager - clone the Arc for ownership
    let client_clone = std::sync::Arc::clone(&client);
    let mut manager = FilterManager::new(Box::new(client_clone));

    for (i, filter) in proposed.iter().enumerate() {
        app.events().emit_filter_progress(FilterProgress {
            operation: FilterOperation::Creating,
            current: i + 1,
            total,
            filter_name: Some(filter.name.clone()),
        });

        if dry_run {
            tracing::info!("Dry run: would create filter {}", filter.name);
            created += 1;
        } else {
            match manager.create_filter(filter).await {
                Ok(_) => {
                    created += 1;
                }
                Err(e) => {
                    errors.push(format!("Failed to create '{}': {}", filter.name, e));
                }
            }
        }
    }

    app.events().emit_filter_progress(FilterProgress {
        operation: FilterOperation::Complete,
        current: total,
        total,
        filter_name: None,
    });

    Ok(ApplyFiltersResult {
        success: errors.is_empty(),
        created,
        failed: errors.len(),
        dry_run,
        errors,
    })
}

/// Result of applying filters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplyFiltersResult {
    pub success: bool,
    pub created: usize,
    pub failed: usize,
    pub dry_run: bool,
    pub errors: Vec<String>,
}

/// Deletes a filter by ID
#[tauri::command]
pub async fn delete_filter(
    filter_id: String,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    let client = state.get_client().ok_or("Gmail client not initialized")?;

    client
        .delete_filter(&filter_id)
        .await
        .map_err(|e| format!("Failed to delete filter: {}", e))?;

    Ok(true)
}

/// Helper to extract from pattern from query
fn extract_from_pattern(query: &str) -> String {
    if let Some(start) = query.find("from:(") {
        let rest = &query[start + 6..];
        if let Some(end) = rest.find(')') {
            return rest[..end].to_string();
        }
    }
    String::new()
}
