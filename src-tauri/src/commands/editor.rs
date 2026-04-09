//! Filter editor commands — search, dry-run, and apply filter modifications

use crate::state::AppState;
use gmail_automation::client::ExistingFilterInfo;
use gmail_automation::filter_editor::{self, ActionDiff, EditorApplyResult, FilterAction};
use gmail_automation::GmailClient;
use tauri::State;

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

/// Get raw filter data for the editor (includes all fields).
/// Returns cached filters from state if available, unless refresh is true.
#[tauri::command]
pub async fn editor_get_filters(
    refresh: Option<bool>,
    state: State<'_, AppState>,
) -> Result<Vec<ExistingFilterInfo>, String> {
    let cached = state.get_existing_filters();
    if !refresh.unwrap_or(false) && !cached.is_empty() {
        return Ok(cached);
    }

    let client = state
        .get_client()
        .ok_or_else(|| "Not authenticated".to_string())?;
    let filters = client
        .list_filters()
        .await
        .map_err(|e| format!("Failed to fetch filters: {}", e))?;
    state.set_existing_filters(filters.clone());
    Ok(filters)
}
