//! Filter editor commands — search, dry-run, and apply filter modifications

use crate::state::AppState;
use gmail_automation::client::ExistingFilterInfo;
use gmail_automation::filter_editor::{self, ActionDiff, EditorApplyResult, FilterAction};
use gmail_automation::GmailClient;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tauri::State;

/// Response from editor_get_filters including a label ID -> name map
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditorFiltersResponse {
    pub filters: Vec<ExistingFilterInfo>,
    pub label_map: HashMap<String, String>,
}

/// Dry-run: preview what changes would be made without touching Gmail
#[tauri::command]
pub async fn editor_dry_run(
    actions: Vec<FilterAction>,
    state: State<'_, AppState>,
) -> Result<Vec<ActionDiff>, String> {
    let filters = state.get_existing_filters();
    Ok(filter_editor::dry_run(&actions, &filters))
}

/// Apply queued actions to Gmail filters
#[tauri::command]
pub async fn editor_apply(
    actions: Vec<FilterAction>,
    state: State<'_, AppState>,
) -> Result<EditorApplyResult, String> {
    let client = state
        .get_client()
        .ok_or_else(|| "Not authenticated".to_string())?;
    let filters = state.get_existing_filters();
    let result = filter_editor::apply(client.as_ref(), &actions, &filters).await;
    Ok(result)
}

/// Get filter data for the editor with resolved label names.
/// Returns cached filters from state if available, unless refresh is true.
#[tauri::command]
pub async fn editor_get_filters(
    refresh: Option<bool>,
    state: State<'_, AppState>,
) -> Result<EditorFiltersResponse, String> {
    let client = state
        .get_client()
        .ok_or_else(|| "Not authenticated".to_string())?;

    let filters = {
        let cached = state.get_existing_filters();
        if !refresh.unwrap_or(false) && !cached.is_empty() {
            cached
        } else {
            let fetched = client
                .list_filters()
                .await
                .map_err(|e| format!("Failed to fetch filters: {}", e))?;
            state.set_existing_filters(fetched.clone());
            fetched
        }
    };

    // Fetch label names (always refresh these as they're cheap)
    let label_map: HashMap<String, String> = match client.list_labels().await {
        Ok(labels) => labels.into_iter().map(|l| (l.id, l.name)).collect(),
        Err(e) => {
            tracing::warn!("Failed to fetch labels: {}", e);
            HashMap::new()
        }
    };

    Ok(EditorFiltersResponse { filters, label_map })
}
