//! OAuth2 authentication management for Gmail API

use google_gmail1::{hyper_rustls, hyper_util, yup_oauth2, Gmail};
use serde::{Deserialize, Serialize};
use std::env;
use std::path::Path;
use yup_oauth2::ApplicationSecret;

use crate::error::{GmailError, Result};

/// Gmail API scopes required for full automation functionality
///
/// These scopes provide:
/// - gmail.modify: Read/write access (no permanent deletion)
/// - gmail.labels: Label management
/// - gmail.settings.basic: Filter creation
pub const REQUIRED_SCOPES: &[&str] = &[
    "https://www.googleapis.com/auth/gmail.modify",
    "https://www.googleapis.com/auth/gmail.labels",
    "https://www.googleapis.com/auth/gmail.settings.basic",
];

/// Read-only scope for safe operations
pub const READONLY_SCOPES: &[&str] = &["https://www.googleapis.com/auth/gmail.readonly"];

/// Metadata-only scope (headers only, no body content)
pub const METADATA_SCOPES: &[&str] = &["https://www.googleapis.com/auth/gmail.metadata"];

/// Type alias for Gmail Hub to simplify type signatures
pub type GmailHub = Gmail<hyper_rustls::HttpsConnector<hyper_util::client::legacy::connect::HttpConnector>>;

/// Authenticate and initialize Gmail API hub with OAuth2
///
/// This is a convenience wrapper around `initialize_gmail_hub` that uses
/// default credential and token paths.
///
/// # Arguments
/// * `credentials_path` - Path to the OAuth2 credentials JSON file
/// * `token_cache_path` - Path where access tokens will be cached
///
/// # Returns
/// A configured Gmail hub ready for API calls
pub async fn authenticate(
    credentials_path: &Path,
    token_cache_path: &Path,
) -> Result<GmailHub> {
    initialize_gmail_hub(credentials_path, token_cache_path).await
}

/// Get a Gmail hub - convenience function with default paths
///
/// Uses standard paths:
/// - Credentials: `./credentials.json`
/// - Token cache: `./token.json`
///
/// # Returns
/// A configured Gmail hub ready for API calls
pub async fn get_gmail_hub() -> Result<GmailHub> {
    let credentials_path = Path::new("credentials.json");
    let token_cache_path = Path::new("token.json");
    initialize_gmail_hub(credentials_path, token_cache_path).await
}

/// Initialize Gmail API hub with OAuth2 authentication
///
/// This function sets up the complete Gmail API client with:
/// - OAuth2 authentication using InstalledFlow (desktop app flow)
/// - Token persistence to disk for automatic refresh
/// - HTTP/1 client with TLS support
///
/// # Arguments
/// * `credentials_path` - Path to the OAuth2 credentials JSON file
/// * `token_cache_path` - Path where access tokens will be cached
///
/// # Returns
/// A configured Gmail hub ready for API calls
pub async fn initialize_gmail_hub(
    credentials_path: &Path,
    token_cache_path: &Path,
) -> Result<GmailHub>
{
    // Read OAuth2 credentials
    let secret = yup_oauth2::read_application_secret(credentials_path)
        .await
        .map_err(|e| GmailError::AuthError(format!("Failed to read credentials: {}", e)))?;

    // Build authenticator with token persistence
    // HTTPRedirect opens a browser for user authorization
    let auth = yup_oauth2::InstalledFlowAuthenticator::builder(
        secret,
        yup_oauth2::InstalledFlowReturnMethod::HTTPRedirect,
    )
    .persist_tokens_to_disk(token_cache_path)
    .build()
    .await
    .map_err(|e| GmailError::AuthError(format!("Failed to build authenticator: {}", e)))?;

    // Pre-authenticate with required scopes to ensure token is cached with correct scopes
    // This prevents the "wrong scope" issue during concurrent operations
    let _token = auth
        .token(REQUIRED_SCOPES)
        .await
        .map_err(|e| GmailError::AuthError(format!("Failed to obtain token: {}", e)))?;

    // Configure HTTP client with TLS
    // Use HTTP/1 for compatibility (HTTP/2 is default but HTTP/1 works better with google-gmail1)
    let client = hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
        .build(
            hyper_rustls::HttpsConnectorBuilder::new()
                .with_native_roots()
                .map_err(|e| {
                    GmailError::AuthError(format!("Failed to load TLS roots: {}", e))
                })?
                .https_or_http()
                .enable_http1()
                .build(),
        );

    Ok(Gmail::new(client, auth))
}

/// Credential structure matching Google's OAuth2 credentials JSON format
#[derive(Debug, Serialize, Deserialize)]
pub struct Credentials {
    pub installed: InstalledApp,
}

/// Installed application credentials (desktop/CLI app)
#[derive(Debug, Serialize, Deserialize)]
pub struct InstalledApp {
    pub client_id: String,
    pub project_id: String,
    pub auth_uri: String,
    pub token_uri: String,
    pub client_secret: String,
    pub redirect_uris: Vec<String>,
}

/// Load OAuth2 credentials from a JSON file
///
/// # Arguments
/// * `path` - Path to credentials.json file
///
/// # Returns
/// Parsed credentials structure
pub async fn load_credentials(path: &Path) -> Result<Credentials> {
    let content = tokio::fs::read_to_string(path).await?;
    let creds = serde_json::from_str(&content)?;
    Ok(creds)
}

/// Load OAuth2 credentials from environment variables
///
/// This is the recommended approach for production deployments
/// to avoid storing credentials in files.
///
/// # Environment Variables
/// - `GMAIL_CLIENT_ID`: OAuth2 client ID
/// - `GMAIL_CLIENT_SECRET`: OAuth2 client secret
/// - `GMAIL_REDIRECT_URI`: Redirect URI (optional, defaults to http://localhost:8080)
///
/// # Returns
/// ApplicationSecret ready for use with authenticator
pub fn load_credentials_from_env() -> Result<ApplicationSecret> {
    let client_id = env::var("GMAIL_CLIENT_ID")
        .map_err(|_| GmailError::ConfigError("GMAIL_CLIENT_ID not set".to_string()))?;
    let client_secret = env::var("GMAIL_CLIENT_SECRET")
        .map_err(|_| GmailError::ConfigError("GMAIL_CLIENT_SECRET not set".to_string()))?;
    let redirect_uri = env::var("GMAIL_REDIRECT_URI")
        .unwrap_or_else(|_| "http://localhost:8080".to_string());

    Ok(ApplicationSecret {
        client_id,
        client_secret,
        auth_uri: "https://accounts.google.com/o/oauth2/auth".to_string(),
        token_uri: "https://oauth2.googleapis.com/token".to_string(),
        redirect_uris: vec![redirect_uri],
        ..Default::default()
    })
}

/// Secure token file permissions on Unix systems
///
/// Sets file permissions to 0600 (read/write for owner only)
/// to prevent unauthorized access to OAuth2 tokens
#[cfg(unix)]
pub async fn secure_token_file(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let mut perms = tokio::fs::metadata(path).await?.permissions();
    perms.set_mode(0o600); // Read/write for owner only
    tokio::fs::set_permissions(path, perms).await?;
    Ok(())
}

/// Secure token file on Windows (stub implementation)
///
/// Windows uses ACLs instead of Unix permissions
/// In production, should use win32 APIs to set appropriate ACLs
#[cfg(windows)]
pub async fn secure_token_file(_path: &Path) -> Result<()> {
    // Windows uses ACLs, file permissions are different
    // In production, use win32 APIs to set appropriate ACLs
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn test_load_credentials() {
        let credentials_json = r#"{
            "installed": {
                "client_id": "test-client-id",
                "project_id": "test-project",
                "auth_uri": "https://accounts.google.com/o/oauth2/auth",
                "token_uri": "https://oauth2.googleapis.com/token",
                "client_secret": "test-secret",
                "redirect_uris": ["http://localhost:8080"]
            }
        }"#;

        let temp_file = NamedTempFile::new().unwrap();
        tokio::fs::write(temp_file.path(), credentials_json)
            .await
            .unwrap();

        let creds = load_credentials(temp_file.path()).await.unwrap();
        assert_eq!(creds.installed.client_id, "test-client-id");
        assert_eq!(creds.installed.project_id, "test-project");
        assert_eq!(creds.installed.client_secret, "test-secret");
    }

    #[tokio::test]
    async fn test_secure_token_file() {
        let temp_file = NamedTempFile::new().unwrap();
        tokio::fs::write(temp_file.path(), "test content")
            .await
            .unwrap();

        // This should not fail
        secure_token_file(temp_file.path()).await.unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let metadata = tokio::fs::metadata(temp_file.path()).await.unwrap();
            let perms = metadata.permissions();
            assert_eq!(perms.mode() & 0o777, 0o600);
        }
    }

    #[test]
    fn test_load_credentials_from_env() {
        env::set_var("GMAIL_CLIENT_ID", "test-id");
        env::set_var("GMAIL_CLIENT_SECRET", "test-secret");
        env::set_var("GMAIL_REDIRECT_URI", "http://localhost:9999");

        let secret = load_credentials_from_env().unwrap();
        assert_eq!(secret.client_id, "test-id");
        assert_eq!(secret.client_secret, "test-secret");
        assert_eq!(secret.redirect_uris[0], "http://localhost:9999");

        env::remove_var("GMAIL_CLIENT_ID");
        env::remove_var("GMAIL_CLIENT_SECRET");
        env::remove_var("GMAIL_REDIRECT_URI");
    }

    #[test]
    fn test_load_credentials_from_env_default_redirect() {
        env::set_var("GMAIL_CLIENT_ID", "test-id");
        env::set_var("GMAIL_CLIENT_SECRET", "test-secret");
        env::remove_var("GMAIL_REDIRECT_URI");

        let secret = load_credentials_from_env().unwrap();
        assert_eq!(secret.redirect_uris[0], "http://localhost:8080");

        env::remove_var("GMAIL_CLIENT_ID");
        env::remove_var("GMAIL_CLIENT_SECRET");
    }

    #[test]
    fn test_scopes_constants() {
        assert_eq!(REQUIRED_SCOPES.len(), 3);
        assert!(REQUIRED_SCOPES.contains(&"https://www.googleapis.com/auth/gmail.modify"));
        assert!(REQUIRED_SCOPES.contains(&"https://www.googleapis.com/auth/gmail.labels"));
        assert!(REQUIRED_SCOPES
            .contains(&"https://www.googleapis.com/auth/gmail.settings.basic"));

        assert_eq!(READONLY_SCOPES.len(), 1);
        assert!(READONLY_SCOPES.contains(&"https://www.googleapis.com/auth/gmail.readonly"));

        assert_eq!(METADATA_SCOPES.len(), 1);
        assert!(METADATA_SCOPES.contains(&"https://www.googleapis.com/auth/gmail.metadata"));
    }
}
