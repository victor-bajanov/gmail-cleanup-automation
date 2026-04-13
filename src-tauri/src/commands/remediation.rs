use tauri::{AppHandle, State};
use std::collections::HashMap;

use crate::events::{AppHandleExt, RemediationProgress};
use gmail_automation::filter_remediation::{
    ApplyResult, GroupDecision, LabelSwap, OverlapDetector, OverlapGroup,
    RemediationApplicator, RemediationPlan, RemediationResult,
};

use crate::state::AppState;

/// Build a RemediationPlan from current state (groups + decisions).
fn build_plan_from_state(state: &AppState) -> RemediationPlan {
    let groups = state.remediation_groups.read().clone();
    let decisions = state.remediation_decisions.read().clone();

    let mut plan = RemediationPlan::new();
    for group in groups {
        let decision = decisions
            .get(&group.group_id)
            .cloned()
            .unwrap_or(GroupDecision::Skip);
        plan.add(group, decision);
    }
    plan
}

#[derive(serde::Serialize)]
pub struct DetectOverlapsResponse {
    pub groups: Vec<OverlapGroup>,
    pub label_id_to_name: HashMap<String, String>,
}

#[tauri::command]
pub async fn detect_overlaps(
    state: State<'_, AppState>,
) -> Result<DetectOverlapsResponse, String> {
    let client = state
        .get_client()
        .ok_or_else(|| "Not authenticated".to_string())?;

    // Ensure label cache is warm
    {
        let is_empty = state.label_cache.read().is_empty();
        if is_empty {
            use gmail_automation::client::GmailClient;
            tracing::info!("Label cache cold, warming from Gmail API");
            match client.list_labels().await {
                Ok(labels) => {
                    let mut cache = state.label_cache.write();
                    for label in &labels {
                        cache.insert(label.name.to_lowercase(), label.id.clone());
                    }
                    tracing::info!("Warmed label cache with {} labels", labels.len());
                }
                Err(e) => {
                    tracing::warn!("Failed to warm label cache: {}", e);
                }
            }
        }
    }

    // label_cache is name->id, but OverlapDetector needs id->name; reverse it
    let label_cache = state.label_cache.read().clone();
    let id_to_name: HashMap<String, String> = label_cache
        .into_iter()
        .map(|(name, id)| (id, name))
        .collect();

    let groups = OverlapDetector::detect(client.as_ref(), &id_to_name)
        .await
        .map_err(|e| format!("Detection failed: {}", e))?;

    // Store groups in state for later use
    *state.remediation_groups.write() = groups.clone();
    *state.remediation_decisions.write() = HashMap::new();

    Ok(DetectOverlapsResponse {
        groups,
        label_id_to_name: id_to_name,
    })
}

#[tauri::command]
pub async fn submit_group_decision(
    group_id: String,
    decision: GroupDecision,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state.remediation_decisions.write().insert(group_id, decision);
    Ok(())
}

#[tauri::command]
pub async fn execute_remediation(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<RemediationResult, String> {
    let client = state
        .get_client()
        .ok_or_else(|| "Not authenticated".to_string())?;

    let plan = build_plan_from_state(&state);
    let total = plan.groups.len();

    let result = plan
        .execute_with_progress(client.as_ref(), |done, group_id| {
            app.events().emit_remediation_progress(RemediationProgress {
                done,
                total,
                group_id: group_id.to_string(),
            });
        })
        .await
        .map_err(|e| format!("Execution failed: {}", e))?;

    // Don't clear state here — apply_remediation_swaps needs it afterward.
    // State is cleared by clear_remediation_state or on next detect_overlaps call.

    Ok(result)
}

#[tauri::command]
pub async fn remediation_summary(
    state: State<'_, AppState>,
) -> Result<String, String> {
    let plan = build_plan_from_state(&state);
    Ok(plan.summary())
}

#[tauri::command]
pub async fn collect_remediation_swaps(
    state: State<'_, AppState>,
) -> Result<Vec<LabelSwap>, String> {
    let plan = build_plan_from_state(&state);
    Ok(RemediationApplicator::collect_swaps(&plan))
}

#[tauri::command]
pub async fn apply_remediation_swaps(
    state: State<'_, AppState>,
) -> Result<ApplyResult, String> {
    let client = state
        .get_client()
        .ok_or_else(|| "Not authenticated".to_string())?;

    let plan = build_plan_from_state(&state);
    let swaps = RemediationApplicator::collect_swaps(&plan);

    let result = RemediationApplicator::apply(client.as_ref(), &swaps)
        .await
        .map_err(|e| format!("Apply failed: {}", e))?;

    // Clear remediation state after apply completes
    *state.remediation_groups.write() = vec![];
    *state.remediation_decisions.write() = HashMap::new();

    Ok(result)
}
