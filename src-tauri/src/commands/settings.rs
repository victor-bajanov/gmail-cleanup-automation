//! Settings commands for configuration management

use crate::state::AppState;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tauri::State;

/// Application settings that can be configured by the user
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    /// Number of days to scan for emails
    pub scan_period_days: u32,
    /// Minimum emails required to form a cluster
    pub min_cluster_size: usize,
    /// Prefix for auto-generated labels
    pub label_prefix: String,
    /// Default archive behavior for new filters
    pub default_archive: bool,
    /// Theme preference (light, dark, system)
    pub theme: String,
    /// Show keyboard shortcuts hints
    pub show_shortcuts: bool,
    /// Enable sound notifications
    pub sound_enabled: bool,
    /// Auto-advance to next cluster after decision
    pub auto_advance: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            scan_period_days: 30,
            min_cluster_size: 3,
            label_prefix: String::new(),
            default_archive: true,
            theme: "system".to_string(),
            show_shortcuts: true,
            sound_enabled: false,
            auto_advance: true,
        }
    }
}

fn settings_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("gmail-cleanup")
        .join("settings.json")
}

/// Get current application settings
#[tauri::command]
pub async fn get_settings() -> Result<AppSettings, String> {
    let path = settings_path();

    if !path.exists() {
        return Ok(AppSettings::default());
    }

    let content = fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read settings: {}", e))?;

    serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse settings: {}", e))
}

/// Save application settings
#[tauri::command]
pub async fn save_settings(settings: AppSettings) -> Result<bool, String> {
    let path = settings_path();

    // Ensure directory exists
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create settings directory: {}", e))?;
    }

    let content = serde_json::to_string_pretty(&settings)
        .map_err(|e| format!("Failed to serialize settings: {}", e))?;

    fs::write(&path, content)
        .map_err(|e| format!("Failed to write settings: {}", e))?;

    Ok(true)
}

/// Reset settings to defaults
#[tauri::command]
pub async fn reset_settings() -> Result<AppSettings, String> {
    let settings = AppSettings::default();
    save_settings(settings.clone()).await?;
    Ok(settings)
}

/// Get window state (size, position) for persistence
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WindowState {
    pub width: u32,
    pub height: u32,
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub maximized: bool,
}

fn window_state_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("gmail-cleanup")
        .join("window-state.json")
}

/// Get saved window state
#[tauri::command]
pub async fn get_window_state() -> Result<WindowState, String> {
    let path = window_state_path();

    if !path.exists() {
        return Ok(WindowState::default());
    }

    let content = fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read window state: {}", e))?;

    serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse window state: {}", e))
}

/// Save window state
#[tauri::command]
pub async fn save_window_state(state: WindowState) -> Result<bool, String> {
    let path = window_state_path();

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create config directory: {}", e))?;
    }

    let content = serde_json::to_string_pretty(&state)
        .map_err(|e| format!("Failed to serialize window state: {}", e))?;

    fs::write(&path, content)
        .map_err(|e| format!("Failed to write window state: {}", e))?;

    Ok(true)
}
