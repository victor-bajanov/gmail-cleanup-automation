//! Settings commands for configuration management

use crate::state::AppState;
use gmail_automation::Config;
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

// =============================================================================
// Config.toml Settings (synced with the main application configuration)
// =============================================================================

/// Configuration settings that mirror config.toml structure for GUI editing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigSettings {
    // Scan settings
    pub scan_period_days: u32,
    pub max_concurrent_requests: usize,
    // Classification settings
    pub classification_mode: String,
    pub llm_provider: String,
    pub minimum_emails_for_label: usize,
    // Labels settings
    pub label_prefix: String,
    pub auto_archive_categories: Vec<String>,
    // Execution settings
    pub dry_run: bool,
    // Circuit breaker settings
    pub circuit_breaker_enabled: bool,
    pub failure_threshold: u32,
    pub reset_timeout_secs: u64,
}

impl Default for ConfigSettings {
    fn default() -> Self {
        Self {
            scan_period_days: 90,
            max_concurrent_requests: 40,
            classification_mode: "rules".to_string(),
            llm_provider: "openai".to_string(),
            minimum_emails_for_label: 5,
            label_prefix: "AutoManaged".to_string(),
            auto_archive_categories: vec![
                "newsletters".to_string(),
                "notifications".to_string(),
                "marketing".to_string(),
            ],
            dry_run: false,
            circuit_breaker_enabled: true,
            failure_threshold: 5,
            reset_timeout_secs: 60,
        }
    }
}

impl From<&Config> for ConfigSettings {
    fn from(config: &Config) -> Self {
        Self {
            scan_period_days: config.scan.period_days,
            max_concurrent_requests: config.scan.max_concurrent_requests,
            classification_mode: config.classification.mode.clone(),
            llm_provider: config.classification.llm_provider.clone(),
            minimum_emails_for_label: config.classification.minimum_emails_for_label,
            label_prefix: config.labels.prefix.clone(),
            auto_archive_categories: config.labels.auto_archive_categories.clone(),
            dry_run: config.execution.dry_run,
            circuit_breaker_enabled: config.circuit_breaker.enabled,
            failure_threshold: config.circuit_breaker.failure_threshold,
            reset_timeout_secs: config.circuit_breaker.reset_timeout_secs,
        }
    }
}

impl ConfigSettings {
    /// Convert ConfigSettings back to a full Config struct
    pub fn to_config(&self) -> Config {
        Config {
            scan: gmail_automation::ScanConfig {
                period_days: self.scan_period_days,
                max_concurrent_requests: self.max_concurrent_requests,
            },
            classification: gmail_automation::ClassificationConfig {
                mode: self.classification_mode.clone(),
                llm_provider: self.llm_provider.clone(),
                minimum_emails_for_label: self.minimum_emails_for_label,
                claude_agents: gmail_automation::ClaudeAgentsConfig::default(),
            },
            labels: gmail_automation::LabelConfig {
                prefix: self.label_prefix.clone(),
                auto_archive_categories: self.auto_archive_categories.clone(),
            },
            execution: gmail_automation::ExecutionConfig {
                dry_run: self.dry_run,
            },
            circuit_breaker: gmail_automation::CircuitBreakerConfig {
                enabled: self.circuit_breaker_enabled,
                failure_threshold: self.failure_threshold,
                reset_timeout_secs: self.reset_timeout_secs,
            },
        }
    }
}

/// Get current config.toml settings
///
/// Loads configuration from config.toml and returns it as ConfigSettings.
/// Falls back to defaults if no config file exists.
#[tauri::command]
pub async fn get_config_settings(state: State<'_, AppState>) -> Result<ConfigSettings, String> {
    let config_path = state.config_path();

    // Try to load from file first
    if config_path.exists() {
        match Config::load(&config_path).await {
            Ok(config) => {
                tracing::info!("Loaded config settings from {:?}", config_path);
                return Ok(ConfigSettings::from(&config));
            }
            Err(e) => {
                tracing::warn!("Failed to load config from {:?}: {}, using defaults", config_path, e);
            }
        }
    }

    // Fall back to AppState config if available
    if let Some(config) = state.get_config() {
        tracing::info!("Using config from AppState");
        return Ok(ConfigSettings::from(&config));
    }

    // Return defaults
    tracing::info!("No config found, using defaults");
    Ok(ConfigSettings::default())
}

/// Save config.toml settings
///
/// Converts ConfigSettings to Config, validates it, saves to config.toml,
/// and updates the AppState.
#[tauri::command]
pub async fn save_config_settings(
    state: State<'_, AppState>,
    settings: ConfigSettings,
) -> Result<bool, String> {
    let config_path = state.config_path();

    // Convert to full Config struct
    let config = settings.to_config();

    // Validate the configuration
    config.validate().map_err(|e| format!("Invalid configuration: {}", e))?;

    // Save to config.toml
    config
        .save(&config_path)
        .await
        .map_err(|e| format!("Failed to save config: {}", e))?;

    // Update AppState with the new config
    state.set_config(config);

    tracing::info!("Saved config settings to {:?}", config_path);
    Ok(true)
}

/// Reset config.toml settings to defaults
///
/// Resets configuration to default values, saves to config.toml,
/// and updates the AppState.
#[tauri::command]
pub async fn reset_config_settings(state: State<'_, AppState>) -> Result<ConfigSettings, String> {
    let config_path = state.config_path();

    // Create default config
    let config = Config::default();
    let settings = ConfigSettings::from(&config);

    // Save to config.toml
    config
        .save(&config_path)
        .await
        .map_err(|e| format!("Failed to save config: {}", e))?;

    // Update AppState
    state.set_config(config);

    tracing::info!("Reset config settings to defaults at {:?}", config_path);
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
