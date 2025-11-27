# Authentication and Configuration Modules

This document describes the authentication and configuration systems implemented for the Gmail Automation System.

## Overview

The system implements two core modules:

1. **Authentication Module (`src/auth.rs`)** - OAuth2 authentication with Gmail API
2. **Configuration Module (`src/config.rs`)** - Application configuration management

## Authentication Module

### Features

- **OAuth2 Authentication**: Uses `yup-oauth2` v12.1.0 for secure Gmail API access
- **Token Persistence**: Automatically saves and refreshes access tokens
- **Secure Storage**: File permissions set to 0600 (Unix) for token security
- **Multiple Scopes**: Support for different permission levels
- **Error Handling**: Comprehensive error types for authentication failures

### API

#### Main Functions

```rust
// Simple authentication with default paths
let hub = auth::get_gmail_hub().await?;

// Authentication with custom paths
let hub = auth::authenticate(
    Path::new("./credentials.json"),
    Path::new("./token.json")
).await?;

// Low-level initialization
let hub = auth::initialize_gmail_hub(
    credentials_path,
    token_cache_path
).await?;
```

#### Type Aliases

```rust
// Convenient type alias for Gmail Hub
pub type GmailHub = Gmail<HttpsConnector<HttpConnector>>;
```

#### OAuth Scopes

```rust
// Full modification access (recommended for automation)
auth::REQUIRED_SCOPES  // modify, labels, settings.basic

// Read-only access (for testing/analysis)
auth::READONLY_SCOPES  // readonly

// Metadata-only access (headers only)
auth::METADATA_SCOPES  // metadata
```

### Usage Example

```rust
use gmail_automation::auth;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize Gmail hub
    let hub = auth::get_gmail_hub().await?;

    // Use the hub for API calls
    let (_, profile) = hub.users()
        .get_profile("me")
        .doit()
        .await?;

    println!("Authenticated as: {}", profile.email_address.unwrap());

    Ok(())
}
```

### Environment Variables

For production deployments, credentials can be loaded from environment variables:

```bash
export GMAIL_CLIENT_ID="your-client-id"
export GMAIL_CLIENT_SECRET="your-client-secret"
export GMAIL_REDIRECT_URI="http://localhost:8080"  # optional
```

```rust
let secret = auth::load_credentials_from_env()?;
```

### Security Features

1. **Token File Permissions**: Automatically set to 0600 (read/write for owner only)
2. **HTTPS Only**: All communication uses TLS via hyper-rustls
3. **Token Refresh**: Automatic token refresh before expiration
4. **Secure Storage**: Tokens stored locally, never transmitted except to Google

### Error Handling

The module returns `GmailError` types from `src/error.rs`:

```rust
match hub_result {
    Ok(hub) => { /* use hub */ },
    Err(GmailError::AuthError(msg)) => {
        eprintln!("Authentication failed: {}", msg);
    },
    Err(e) => {
        eprintln!("Other error: {}", e);
    }
}
```

## Configuration Module

### Features

- **TOML Configuration**: Human-readable configuration format
- **Sensible Defaults**: Works out-of-the-box without configuration
- **Validation**: Comprehensive validation of all settings
- **Partial Configs**: Override only the settings you need
- **Type Safety**: Strongly-typed configuration structures

### Configuration Structure

```toml
[scan]
period_days = 90                    # 1-365
max_concurrent_requests = 40        # 1-50

[classification]
mode = "rules"                      # "rules", "ml", "hybrid"
llm_provider = "openai"             # "openai", "anthropic", "anthropic-agents"
minimum_emails_for_label = 5

[classification.claude_agents]
enabled = false
use_advanced_analysis = true
max_iterations = 3

[labels]
prefix = "AutoManaged"
auto_archive_categories = [
    "newsletters",
    "notifications",
    "marketing"
]

[execution]
dry_run = false
```

### API

#### Loading Configuration

```rust
use gmail_automation::config::Config;
use std::path::Path;

// Load from file (or use defaults if missing)
let config = Config::load(Path::new("config.toml")).await?;

// Use default configuration
let config = Config::default();
```

#### Saving Configuration

```rust
// Save configuration to file
config.save(Path::new("config.toml")).await?;

// Create example configuration file
Config::create_example(Path::new("config.toml.example")).await?;
```

#### Validation

```rust
// Validate configuration
config.validate()?;

// Validation is automatic when loading from file
let config = Config::load(path).await?;  // Already validated
```

### Validation Rules

The configuration module enforces these constraints:

#### Scan Settings
- `period_days`: Must be 1-365 (1 day to 1 year)
- `max_concurrent_requests`: Must be 1-50 (to stay under Gmail rate limits)

#### Classification Settings
- `mode`: Must be "rules", "ml", or "hybrid"
- `llm_provider`: Must be "openai", "anthropic", or "anthropic-agents"
- `minimum_emails_for_label`: Must be > 0
- `claude_agents.max_iterations`: Must be > 0

#### Label Settings
- `prefix`: Cannot be empty or contain '/' character
- `auto_archive_categories`: Cannot contain empty strings

### Usage Examples

#### Basic Usage

```rust
use gmail_automation::config::Config;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load configuration (uses defaults if file doesn't exist)
    let config = Config::load(Path::new("config.toml")).await?;

    // Access configuration values
    println!("Scanning last {} days", config.scan.period_days);
    println!("Max concurrent: {}", config.scan.max_concurrent_requests);
    println!("Classification mode: {}", config.classification.mode);

    Ok(())
}
```

#### Custom Configuration

```rust
use gmail_automation::config::{Config, ScanConfig};

// Create custom configuration
let mut config = Config::default();

// Modify settings
config.scan.period_days = 30;
config.scan.max_concurrent_requests = 20;
config.classification.mode = "hybrid".to_string();
config.execution.dry_run = true;

// Validate before use
config.validate()?;

// Save for future use
config.save(Path::new("config.toml")).await?;
```

#### Partial Configuration

You can provide a partial configuration file, and defaults will fill in missing values:

```toml
# minimal.toml - override just what you need
[scan]
period_days = 30

[execution]
dry_run = true
```

```rust
let config = Config::load(Path::new("minimal.toml")).await?;

// period_days = 30 (from file)
// max_concurrent_requests = 40 (default)
// classification.mode = "rules" (default)
```

### Default Values

| Setting | Default | Range | Description |
|---------|---------|-------|-------------|
| `scan.period_days` | 90 | 1-365 | Days to scan back |
| `scan.max_concurrent_requests` | 40 | 1-50 | Concurrent API calls |
| `classification.mode` | "rules" | rules/ml/hybrid | Classification strategy |
| `classification.llm_provider` | "openai" | openai/anthropic/anthropic-agents | LLM provider |
| `classification.minimum_emails_for_label` | 5 | > 0 | Min emails for label |
| `classification.claude_agents.enabled` | false | bool | Enable Claude Agents SDK |
| `classification.claude_agents.use_advanced_analysis` | true | bool | Multi-step reasoning |
| `classification.claude_agents.max_iterations` | 3 | > 0 | Max pattern iterations |
| `labels.prefix` | "AutoManaged" | non-empty | Label prefix |
| `labels.auto_archive_categories` | ["newsletters", "notifications", "marketing"] | array | Auto-archive categories |
| `execution.dry_run` | false | bool | Preview mode |

## Testing

Both modules include comprehensive test suites.

### Running Tests

```bash
# Run all tests
cargo test

# Run only auth tests
cargo test auth

# Run only config tests
cargo test config

# Run with output
cargo test -- --nocapture
```

### Test Coverage

#### Authentication Module (`auth.rs`)
- ✅ Credential loading from JSON
- ✅ Credential loading from environment variables
- ✅ Token file security (Unix permissions)
- ✅ Scope constants validation
- ✅ Default redirect URI handling

#### Configuration Module (`config.rs`)
- ✅ Default configuration values
- ✅ Validation of all settings
- ✅ Boundary condition testing (min/max values)
- ✅ Invalid value rejection
- ✅ Serialization/deserialization roundtrip
- ✅ File I/O (load/save)
- ✅ Partial configuration with defaults
- ✅ Invalid TOML handling
- ✅ Nonexistent file handling

## Integration with Other Modules

### Authentication Integration

```rust
// scanner.rs
use crate::auth::{GmailHub, authenticate};

pub struct EmailScanner {
    hub: GmailHub,
}

impl EmailScanner {
    pub async fn new(credentials_path: &Path, token_path: &Path) -> Result<Self> {
        let hub = authenticate(credentials_path, token_path).await?;
        Ok(Self { hub })
    }
}
```

### Configuration Integration

```rust
// main.rs
use crate::config::Config;
use crate::auth;

#[tokio::main]
async fn main() -> Result<()> {
    // Load configuration
    let config = Config::load(Path::new("config.toml")).await?;

    // Initialize authentication
    let hub = auth::get_gmail_hub().await?;

    // Use configuration values
    let scanner = EmailScanner::new(hub, config.scan).await?;

    Ok(())
}
```

## Best Practices

### Authentication

1. **Never commit credentials**: Add `credentials.json` and `token.json` to `.gitignore`
2. **Use environment variables in production**: Load from env vars for deployed systems
3. **Minimal scopes**: Only request the OAuth scopes you actually need
4. **Secure token storage**: The module handles this automatically, but verify file permissions

### Configuration

1. **Start with defaults**: Use `Config::default()` and override as needed
2. **Validate early**: Always call `validate()` after manual configuration changes
3. **Use dry-run mode**: Test configuration with `dry_run = true` before making changes
4. **Document customizations**: Comment your config.toml file for future reference

## Error Recovery

### Authentication Errors

```rust
use gmail_automation::error::GmailError;

match auth::get_gmail_hub().await {
    Ok(hub) => { /* success */ },
    Err(GmailError::AuthError(msg)) => {
        eprintln!("Authentication failed: {}", msg);
        eprintln!("Please check your credentials.json file");
        eprintln!("Delete token.json to re-authenticate");
    },
    Err(e) => eprintln!("Unexpected error: {}", e),
}
```

### Configuration Errors

```rust
match Config::load(path).await {
    Ok(config) => { /* success */ },
    Err(GmailError::ConfigError(msg)) if msg.contains("parse") => {
        eprintln!("Invalid TOML syntax: {}", msg);
        eprintln!("Check your config.toml for syntax errors");
    },
    Err(GmailError::ConfigError(msg)) => {
        eprintln!("Configuration validation failed: {}", msg);
        eprintln!("See config.toml.example for valid settings");
    },
    Err(e) => eprintln!("Unexpected error: {}", e),
}
```

## Future Enhancements

Potential improvements for future versions:

### Authentication
- [ ] Support for service account authentication
- [ ] Multi-account management
- [ ] Token encryption at rest
- [ ] OAuth2 scope negotiation

### Configuration
- [ ] Configuration profiles (dev, staging, prod)
- [ ] Dynamic configuration reload
- [ ] Configuration validation schemas
- [ ] Export/import configuration templates

## References

- [Gmail API Documentation](https://developers.google.com/gmail/api)
- [yup-oauth2 Documentation](https://docs.rs/yup-oauth2/12.1.0)
- [google-gmail1 Documentation](https://docs.rs/google-gmail1/6.0.0)
- [TOML Specification](https://toml.io/en/)

## License

Part of the Gmail Automation System.
