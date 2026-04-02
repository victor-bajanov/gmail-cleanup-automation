use tauri::State;
use std::collections::HashMap;

use gmail_automation::filter_remediation::{
    ApplyResult, GroupDecision, LabelSwap, OverlapDetector, OverlapGroup,
    RemediationApplicator, RemediationPlan, RemediationResult,
};

use crate::state::AppState;

#[tauri::command]
pub async fn detect_overlaps(
    state: State<'_, AppState>,
) -> Result<Vec<OverlapGroup>, String> {
    let client = state
        .get_client()
        .ok_or_else(|| "Not authenticated".to_string())?;

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

    Ok(groups)
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
    state: State<'_, AppState>,
) -> Result<RemediationResult, String> {
    let client = state
        .get_client()
        .ok_or_else(|| "Not authenticated".to_string())?;

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

    let result = plan
        .execute(client.as_ref())
        .await
        .map_err(|e| format!("Execution failed: {}", e))?;

    // Clear remediation state
    *state.remediation_groups.write() = vec![];
    *state.remediation_decisions.write() = HashMap::new();

    Ok(result)
}

#[tauri::command]
pub async fn remediation_summary(
    state: State<'_, AppState>,
) -> Result<String, String> {
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

    Ok(plan.summary())
}

#[tauri::command]
pub async fn collect_remediation_swaps(
    state: State<'_, AppState>,
) -> Result<Vec<LabelSwap>, String> {
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

    Ok(RemediationApplicator::collect_swaps(&plan))
}

#[tauri::command]
pub async fn apply_remediation_swaps(
    state: State<'_, AppState>,
) -> Result<ApplyResult, String> {
    let client = state
        .get_client()
        .ok_or_else(|| "Not authenticated".to_string())?;

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

    let swaps = RemediationApplicator::collect_swaps(&plan);
    RemediationApplicator::apply(client.as_ref(), &swaps)
        .await
        .map_err(|e| format!("Apply failed: {}", e))
}
