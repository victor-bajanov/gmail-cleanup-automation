use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::error::{GmailError, Result};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub scan: ScanConfig,
    #[serde(default)]
    pub classification: ClassificationConfig,
    #[serde(default)]
    pub labels: LabelConfig,
    #[serde(default)]
    pub execution: ExecutionConfig,
    #[serde(default)]
    pub circuit_breaker: CircuitBreakerConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanConfig {
    #[serde(default = "default_period_days")]
    pub period_days: u32,
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent_requests: usize,
}

impl Default for ScanConfig {
    fn default() -> Self {
        Self {
            period_days: default_period_days(),
            max_concurrent_requests: default_max_concurrent(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassificationConfig {
    #[serde(default = "default_mode")]
    pub mode: String,
    #[serde(default = "default_llm_provider")]
    pub llm_provider: String,
    #[serde(default = "default_min_emails")]
    pub minimum_emails_for_label: usize,
    #[serde(default)]
    pub claude_agents: ClaudeAgentsConfig,
}

impl Default for ClassificationConfig {
    fn default() -> Self {
        Self {
            mode: default_mode(),
            llm_provider: default_llm_provider(),
            minimum_emails_for_label: default_min_emails(),
            claude_agents: ClaudeAgentsConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeAgentsConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_advanced_analysis")]
    pub use_advanced_analysis: bool,
    #[serde(default = "default_max_iterations")]
    pub max_iterations: u32,
}

impl Default for ClaudeAgentsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            use_advanced_analysis: default_advanced_analysis(),
            max_iterations: default_max_iterations(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabelConfig {
    #[serde(default = "default_prefix")]
    pub prefix: String,
    #[serde(default = "default_auto_archive_categories")]
    pub auto_archive_categories: Vec<String>,
}

impl Default for LabelConfig {
    fn default() -> Self {
        Self {
            prefix: default_prefix(),
            auto_archive_categories: default_auto_archive_categories(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExecutionConfig {
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircuitBreakerConfig {
    #[serde(default = "default_circuit_breaker_enabled")]
    pub enabled: bool,
    #[serde(default = "default_failure_threshold")]
    pub failure_threshold: u32,
    #[serde(default = "default_reset_timeout_secs")]
    pub reset_timeout_secs: u64,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            enabled: default_circuit_breaker_enabled(),
            failure_threshold: default_failure_threshold(),
            reset_timeout_secs: default_reset_timeout_secs(),
        }
    }
}

fn default_period_days() -> u32 {
    90
}

fn default_max_concurrent() -> usize {
    40
}

fn default_mode() -> String {
    "rules".to_string()
}

fn default_min_emails() -> usize {
    5
}

fn default_prefix() -> String {
    "AutoManaged".to_string()
}

fn default_auto_archive_categories() -> Vec<String> {
    vec![
        "newsletters".to_string(),
        "notifications".to_string(),
        "marketing".to_string(),
    ]
}

fn default_llm_provider() -> String {
    "openai".to_string()
}

fn default_advanced_analysis() -> bool {
    true
}

fn default_max_iterations() -> u32 {
    3
}

fn default_circuit_breaker_enabled() -> bool {
    true
}

fn default_failure_threshold() -> u32 {
    5
}

fn default_reset_timeout_secs() -> u64 {
    60
}

impl Config {
    pub async fn load(path: &Path) -> Result<Self> {
        // If file doesn't exist, return default config with warning
        if !path.exists() {
            tracing::warn!("Config file not found at {:?}, using defaults", path);
            return Ok(Self::default());
        }

        let content = tokio::fs::read_to_string(path)
            .await
            .map_err(|e| GmailError::ConfigError(format!("Failed to read config file: {}", e)))?;

        let config: Self = toml::from_str(&content)
            .map_err(|e| GmailError::ConfigError(format!("Failed to parse config file: {}", e)))?;

        // Validate the loaded config
        config.validate()?;

        tracing::info!("Loaded configuration from {:?}", path);
        Ok(config)
    }

    pub async fn save(&self, path: &Path) -> Result<()> {
        // Create parent directory if it doesn't exist
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                GmailError::ConfigError(format!("Failed to create config directory: {}", e))
            })?;
        }

        let content = toml::to_string_pretty(self)
            .map_err(|e| GmailError::ConfigError(format!("Failed to serialize config: {}", e)))?;

        tokio::fs::write(path, content)
            .await
            .map_err(|e| GmailError::ConfigError(format!("Failed to write config file: {}", e)))?;

        tracing::info!("Saved configuration to {:?}", path);
        Ok(())
    }

    /// Validate configuration values
    pub fn validate(&self) -> Result<()> {
        // Validate scan config - period_days must be 1-365
        if self.scan.period_days == 0 {
            return Err(GmailError::ConfigError(
                "scan.period_days must be at least 1".to_string(),
            ));
        }
        if self.scan.period_days > 365 {
            return Err(GmailError::ConfigError(
                "scan.period_days cannot exceed 365 (1 year)".to_string(),
            ));
        }

        // Validate max_concurrent_requests - must be 1-50 to stay under rate limits
        if self.scan.max_concurrent_requests == 0 {
            return Err(GmailError::ConfigError(
                "scan.max_concurrent_requests must be at least 1".to_string(),
            ));
        }
        if self.scan.max_concurrent_requests > 50 {
            return Err(GmailError::ConfigError(
                "scan.max_concurrent_requests cannot exceed 50 (to stay under Gmail API rate limits of 250 units/sec)".to_string(),
            ));
        }

        // Validate classification config
        match self.classification.mode.as_str() {
            "rules" | "ml" | "hybrid" => {}
            other => {
                return Err(GmailError::ConfigError(format!(
                    "Invalid classification.mode: '{}'. Must be 'rules', 'ml', or 'hybrid'",
                    other
                )));
            }
        }

        match self.classification.llm_provider.as_str() {
            "openai" | "anthropic" | "anthropic-agents" => {}
            other => {
                return Err(GmailError::ConfigError(format!(
                    "Invalid classification.llm_provider: '{}'. Must be 'openai', 'anthropic', or 'anthropic-agents'",
                    other
                )));
            }
        }

        if self.classification.minimum_emails_for_label == 0 {
            return Err(GmailError::ConfigError(
                "classification.minimum_emails_for_label must be greater than 0".to_string(),
            ));
        }

        if self.classification.claude_agents.max_iterations == 0 {
            return Err(GmailError::ConfigError(
                "classification.claude_agents.max_iterations must be greater than 0".to_string(),
            ));
        }

        // Validate labels config
        if self.labels.prefix.is_empty() {
            return Err(GmailError::ConfigError(
                "labels.prefix cannot be empty".to_string(),
            ));
        }
        if self.labels.prefix.contains('/') {
            return Err(GmailError::ConfigError(
                "labels.prefix cannot contain '/' character".to_string(),
            ));
        }

        // Validate auto_archive_categories
        for category in &self.labels.auto_archive_categories {
            if category.is_empty() {
                return Err(GmailError::ConfigError(
                    "labels.auto_archive_categories cannot contain empty strings".to_string(),
                ));
            }
        }

        // Validate circuit breaker config
        if self.circuit_breaker.failure_threshold == 0 {
            return Err(GmailError::ConfigError(
                "circuit_breaker.failure_threshold must be greater than 0".to_string(),
            ));
        }
        if self.circuit_breaker.reset_timeout_secs == 0 {
            return Err(GmailError::ConfigError(
                "circuit_breaker.reset_timeout_secs must be greater than 0".to_string(),
            ));
        }

        tracing::debug!("Configuration validation passed");
        Ok(())
    }

    /// Create an example configuration file
    pub async fn create_example(path: &Path) -> Result<()> {
        let config = Self::default();
        config.save(path).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_default_config() {
        let config = Config::default();

        // Verify scan defaults
        assert_eq!(config.scan.period_days, 90);
        assert_eq!(config.scan.max_concurrent_requests, 40);

        // Verify classification defaults
        assert_eq!(config.classification.mode, "rules");
        assert_eq!(config.classification.llm_provider, "openai");
        assert_eq!(config.classification.minimum_emails_for_label, 5);
        assert!(!config.classification.claude_agents.enabled);
        assert!(config.classification.claude_agents.use_advanced_analysis);
        assert_eq!(config.classification.claude_agents.max_iterations, 3);

        // Verify label defaults
        assert_eq!(config.labels.prefix, "AutoManaged");
        assert_eq!(config.labels.auto_archive_categories.len(), 3);
        assert!(config
            .labels
            .auto_archive_categories
            .contains(&"newsletters".to_string()));
        assert!(config
            .labels
            .auto_archive_categories
            .contains(&"notifications".to_string()));
        assert!(config
            .labels
            .auto_archive_categories
            .contains(&"marketing".to_string()));

        // Verify execution defaults
        assert!(!config.execution.dry_run);
    }

    #[test]
    fn test_config_validation_valid() {
        let config = Config::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_config_validation_period_days_zero() {
        let mut config = Config::default();
        config.scan.period_days = 0;
        let result = config.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("at least 1"));
    }

    #[test]
    fn test_config_validation_period_days_too_high() {
        let mut config = Config::default();
        config.scan.period_days = 366;
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("cannot exceed 365"));
    }

    #[test]
    fn test_config_validation_period_days_boundary_valid() {
        let mut config = Config::default();

        // Test lower boundary
        config.scan.period_days = 1;
        assert!(config.validate().is_ok());

        // Test upper boundary
        config.scan.period_days = 365;
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_config_validation_max_concurrent_zero() {
        let mut config = Config::default();
        config.scan.max_concurrent_requests = 0;
        let result = config.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("at least 1"));
    }

    #[test]
    fn test_config_validation_max_concurrent_too_high() {
        let mut config = Config::default();
        config.scan.max_concurrent_requests = 51;
        let result = config.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("cannot exceed 50"));
    }

    #[test]
    fn test_config_validation_max_concurrent_boundary_valid() {
        let mut config = Config::default();

        // Test lower boundary
        config.scan.max_concurrent_requests = 1;
        assert!(config.validate().is_ok());

        // Test upper boundary
        config.scan.max_concurrent_requests = 50;
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_config_validation_invalid_mode() {
        let mut config = Config::default();
        config.classification.mode = "invalid".to_string();
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Invalid classification.mode"));
    }

    #[test]
    fn test_config_validation_valid_modes() {
        let mut config = Config::default();

        config.classification.mode = "rules".to_string();
        assert!(config.validate().is_ok());

        config.classification.mode = "ml".to_string();
        assert!(config.validate().is_ok());

        config.classification.mode = "hybrid".to_string();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_config_validation_invalid_llm_provider() {
        let mut config = Config::default();
        config.classification.llm_provider = "invalid".to_string();
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Invalid classification.llm_provider"));
    }

    #[test]
    fn test_config_validation_valid_llm_providers() {
        let mut config = Config::default();

        config.classification.llm_provider = "openai".to_string();
        assert!(config.validate().is_ok());

        config.classification.llm_provider = "anthropic".to_string();
        assert!(config.validate().is_ok());

        config.classification.llm_provider = "anthropic-agents".to_string();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_config_validation_min_emails_zero() {
        let mut config = Config::default();
        config.classification.minimum_emails_for_label = 0;
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("minimum_emails_for_label must be greater than 0"));
    }

    #[test]
    fn test_config_validation_max_iterations_zero() {
        let mut config = Config::default();
        config.classification.claude_agents.max_iterations = 0;
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("max_iterations must be greater than 0"));
    }

    #[test]
    fn test_config_validation_empty_prefix() {
        let mut config = Config::default();
        config.labels.prefix = "".to_string();
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("prefix cannot be empty"));
    }

    #[test]
    fn test_config_validation_prefix_with_slash() {
        let mut config = Config::default();
        config.labels.prefix = "Auto/Managed".to_string();
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("prefix cannot contain '/'"));
    }

    #[test]
    fn test_config_validation_empty_category() {
        let mut config = Config::default();
        config.labels.auto_archive_categories.push("".to_string());
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("auto_archive_categories cannot contain empty strings"));
    }

    #[tokio::test]
    async fn test_config_serialization_roundtrip() {
        let config = Config::default();

        let serialized = toml::to_string_pretty(&config).unwrap();
        let deserialized: Config = toml::from_str(&serialized).unwrap();

        assert_eq!(config.scan.period_days, deserialized.scan.period_days);
        assert_eq!(
            config.scan.max_concurrent_requests,
            deserialized.scan.max_concurrent_requests
        );
        assert_eq!(config.classification.mode, deserialized.classification.mode);
        assert_eq!(config.labels.prefix, deserialized.labels.prefix);
        assert_eq!(config.execution.dry_run, deserialized.execution.dry_run);
    }

    #[tokio::test]
    async fn test_config_load_save_roundtrip() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        // Create and save config
        let config = Config::default();
        config.save(path).await.unwrap();

        // Load it back
        let loaded = Config::load(path).await.unwrap();

        assert_eq!(config.scan.period_days, loaded.scan.period_days);
        assert_eq!(
            config.scan.max_concurrent_requests,
            loaded.scan.max_concurrent_requests
        );
        assert_eq!(config.classification.mode, loaded.classification.mode);
        assert_eq!(config.labels.prefix, loaded.labels.prefix);
    }

    #[tokio::test]
    async fn test_config_load_nonexistent_returns_default() {
        let path = Path::new("/tmp/nonexistent-config-12345.toml");

        // Should return default config without error
        let config = Config::load(path).await.unwrap();

        assert_eq!(config.scan.period_days, 90);
        assert_eq!(config.scan.max_concurrent_requests, 40);
    }

    #[tokio::test]
    async fn test_config_load_invalid_toml() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        // Write invalid TOML
        tokio::fs::write(path, "this is not valid toml {[}]")
            .await
            .unwrap();

        let result = Config::load(path).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Failed to parse config file"));
    }

    #[tokio::test]
    async fn test_config_partial_with_defaults() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        // Write partial config - only override some values
        let partial_config = r#"
[scan]
period_days = 30

[execution]
dry_run = true
"#;
        tokio::fs::write(path, partial_config).await.unwrap();

        let config = Config::load(path).await.unwrap();

        // Check overridden values
        assert_eq!(config.scan.period_days, 30);
        assert!(config.execution.dry_run);

        // Check default values are still present
        assert_eq!(config.scan.max_concurrent_requests, 40); // default
        assert_eq!(config.classification.mode, "rules"); // default
        assert_eq!(config.labels.prefix, "AutoManaged"); // default
    }

    #[tokio::test]
    async fn test_config_create_example() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        Config::create_example(path).await.unwrap();

        // Verify file was created
        assert!(path.exists());

        // Verify it can be loaded
        let config = Config::load(path).await.unwrap();
        assert_eq!(config.scan.period_days, 90);
    }

    #[test]
    fn test_default_functions() {
        assert_eq!(default_period_days(), 90);
        assert_eq!(default_max_concurrent(), 40);
        assert_eq!(default_mode(), "rules");
        assert_eq!(default_min_emails(), 5);
        assert_eq!(default_prefix(), "AutoManaged");
        assert_eq!(default_llm_provider(), "openai");
        assert!(default_advanced_analysis());
        assert_eq!(default_max_iterations(), 3);
    }
}
