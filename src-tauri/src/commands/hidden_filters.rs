//! Hidden filters persistence for the filter overlap screen
//!
//! This module handles storing and loading the set of filter IDs that the user
//! has chosen to hide from the overlap analysis results.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

/// Hidden filter info with full details for display
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HiddenFilterInfo {
    pub id: String,
    pub name: String,
    pub query: String,
    pub label: String,
}

/// Hidden filters data structure for JSON persistence
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HiddenFiltersData {
    /// Map of filter ID to full filter info for hidden filters
    pub hidden_filters: HashMap<String, HiddenFilterInfo>,
}

/// Gets the path to the hidden filters JSON file
fn hidden_filters_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("gmail-cleanup")
        .join("hidden-filters.json")
}

/// Loads hidden filters from disk
pub fn load_hidden_filters() -> HiddenFiltersData {
    let path = hidden_filters_path();

    if !path.exists() {
        return HiddenFiltersData::default();
    }

    match fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_else(|e| {
            tracing::warn!("Failed to parse hidden-filters.json: {}, using empty set", e);
            HiddenFiltersData::default()
        }),
        Err(e) => {
            tracing::warn!("Failed to read hidden-filters.json: {}, using empty set", e);
            HiddenFiltersData::default()
        }
    }
}

/// Saves hidden filters to disk
pub fn save_hidden_filters(data: &HiddenFiltersData) -> Result<(), String> {
    let path = hidden_filters_path();

    // Ensure directory exists
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create config directory: {}", e))?;
    }

    let content = serde_json::to_string_pretty(data)
        .map_err(|e| format!("Failed to serialize hidden filters: {}", e))?;

    fs::write(&path, content)
        .map_err(|e| format!("Failed to write hidden filters: {}", e))?;

    tracing::debug!("Saved {} hidden filters to {:?}", data.hidden_filters.len(), path);

    Ok(())
}

/// Adds a filter with full info to the hidden set and persists
pub fn add_hidden_filter(data: &mut HiddenFiltersData, info: HiddenFilterInfo) -> Result<(), String> {
    data.hidden_filters.insert(info.id.clone(), info);
    save_hidden_filters(data)
}

/// Removes a filter ID from the hidden set and persists
pub fn remove_hidden_filter(data: &mut HiddenFiltersData, filter_id: &str) -> Result<(), String> {
    data.hidden_filters.remove(filter_id);
    save_hidden_filters(data)
}

/// Clears all hidden filters and persists
pub fn clear_all_hidden_filters(data: &mut HiddenFiltersData) -> Result<(), String> {
    data.hidden_filters.clear();
    save_hidden_filters(data)
}

/// Gets the set of hidden filter IDs (for filtering in overlap analysis)
pub fn get_hidden_filter_ids(data: &HiddenFiltersData) -> std::collections::HashSet<String> {
    data.hidden_filters.keys().cloned().collect()
}

/// Gets all hidden filter info for display
pub fn get_hidden_filter_list(data: &HiddenFiltersData) -> Vec<HiddenFilterInfo> {
    data.hidden_filters.values().cloned().collect()
}
