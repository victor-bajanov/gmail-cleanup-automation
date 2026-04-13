//! Authentication commands for Gmail OAuth2

use crate::state::AppState;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::State;

/// Authentication status response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthStatus {
    /// Whether the user is authenticated
    pub authenticated: bool,
    /// User's email address (if authenticated)
    pub email: Option<String>,
    /// Path to credentials file
    pub credentials_path: String,
    /// Whether credentials file exists
    pub credentials_exist: bool,
    /// Whether token file exists
    pub token_exists: bool,
}

/// Checks the current authentication status
#[tauri::command]
pub async fn check_auth_status(state: State<'_, AppState>) -> Result<AuthStatus, String> {
    let credentials_path = state.credentials_path();
    let token_path = state.token_path();

    tracing::info!("check_auth_status: credentials_path={:?}, token_path={:?}", credentials_path, token_path);

    let credentials_exist = credentials_path.exists();
    let token_exists = token_path.exists();

    // If no token, not authenticated
    if !token_exists {
        return Ok(AuthStatus {
            authenticated: false,
            email: None,
            credentials_path: credentials_path.display().to_string(),
            credentials_exist,
            token_exists: false,
        });
    }

    // If no credentials, can't verify
    if !credentials_exist {
        return Ok(AuthStatus {
            authenticated: false,
            email: None,
            credentials_path: credentials_path.display().to_string(),
            credentials_exist: false,
            token_exists: true,
        });
    }

    tracing::info!("check_auth_status: about to initialize gmail hub");

    // Try to initialize and verify the token
    // Note: We don't call get_profile here because it requests a different scope (gmail.readonly)
    // which would trigger a new auth flow. Instead, just verify the hub initializes successfully.
    match gmail_automation::auth::initialize_gmail_hub(&credentials_path, &token_path).await {
        Ok(_hub) => {
            tracing::info!("check_auth_status: hub initialized successfully");
            // Token was obtained successfully - we're authenticated
            // Email is not available without profile call, but that's OK for status check
            Ok(AuthStatus {
                authenticated: true,
                email: None, // Would require gmail.readonly scope to fetch
                credentials_path: credentials_path.display().to_string(),
                credentials_exist: true,
                token_exists: true,
            })
        }
        Err(e) => {
            tracing::warn!("Failed to initialize Gmail hub: {}", e);
            Ok(AuthStatus {
                authenticated: false,
                email: None,
                credentials_path: credentials_path.display().to_string(),
                credentials_exist: true,
                token_exists: true,
            })
        }
    }
}

/// Sets the path to the credentials.json file
#[tauri::command]
pub async fn set_credentials_path(
    path: String,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    let path_buf = PathBuf::from(&path);

    if !path_buf.exists() {
        return Err(format!("Credentials file not found: {}", path));
    }

    state.set_credentials_path(path_buf);
    Ok(true)
}

/// Initiates the OAuth2 authentication flow
///
/// This will open a browser window for the user to authenticate with Google.
/// After authentication, the token will be stored locally.
#[tauri::command]
pub async fn authenticate(state: State<'_, AppState>) -> Result<AuthStatus, String> {
    let credentials_path = state.credentials_path();
    let token_path = state.token_path();

    tracing::info!("authenticate: credentials_path={:?}, token_path={:?}", credentials_path, token_path);

    if !credentials_path.exists() {
        return Err(format!(
            "Credentials file not found at: {}. Please download credentials.json from Google Cloud Console.",
            credentials_path.display()
        ));
    }

    tracing::info!("authenticate: credentials file exists");

    // Ensure token directory exists
    if let Some(parent) = token_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create token directory: {}", e))?;
    }

    tracing::info!("authenticate: about to initialize_gmail_hub");

    // Initialize Gmail hub (this triggers OAuth flow if needed)
    // Note: We don't call get_profile because it requests gmail.readonly scope
    match gmail_automation::auth::initialize_gmail_hub(&credentials_path, &token_path).await {
        Ok(_hub) => {
            tracing::info!("authenticate: gmail hub initialized successfully");
            Ok(AuthStatus {
                authenticated: true,
                email: None, // Would require gmail.readonly scope
                credentials_path: credentials_path.display().to_string(),
                credentials_exist: true,
                token_exists: true,
            })
        }
        Err(e) => Err(format!("Authentication failed: {}", e)),
    }
}

/// Clears the stored authentication token
#[tauri::command]
pub async fn logout(state: State<'_, AppState>) -> Result<bool, String> {
    let token_path = state.token_path();

    if token_path.exists() {
        std::fs::remove_file(&token_path)
            .map_err(|e| format!("Failed to remove token file: {}", e))?;
    }

    // Clear client from state
    state.clear_session();

    Ok(true)
}

/// Initializes the Gmail client in state after authentication
#[tauri::command]
pub async fn initialize_client(state: State<'_, AppState>) -> Result<bool, String> {
    let credentials_path = state.credentials_path();
    let token_path = state.token_path();

    if !credentials_path.exists() || !token_path.exists() {
        return Err("Not authenticated. Please authenticate first.".to_string());
    }

    // Get or create config
    let config = state.get_config().unwrap_or_else(|| {
        gmail_automation::Config {
            scan: gmail_automation::ScanConfig::default(),
            classification: gmail_automation::ClassificationConfig::default(),
            labels: gmail_automation::LabelConfig::default(),
            execution: gmail_automation::ExecutionConfig::default(),
            circuit_breaker: gmail_automation::CircuitBreakerConfig::default(),
        }
    });

    // Initialize Gmail hub
    let hub = gmail_automation::auth::initialize_gmail_hub(&credentials_path, &token_path)
        .await
        .map_err(|e| format!("Failed to initialize Gmail hub: {}", e))?;

    // Create client with rate limiting
    tracing::info!(
        "Creating Gmail client with max_concurrent_requests={}",
        config.scan.max_concurrent_requests
    );
    let client = gmail_automation::ProductionGmailClient::with_full_config(
        hub,
        config.scan.max_concurrent_requests,
        250.0, // quota units per second (Gmail default: 250)
        500.0, // quota burst capacity (2 seconds worth)
        config.circuit_breaker.clone(),
    );

    state.set_client(client);
    state.set_config(config);

    Ok(true)
}
