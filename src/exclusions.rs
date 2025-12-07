//! Persistent exclusions for email clusters
//!
//! Allows users to permanently exclude certain clusters from review.
//! Exclusions are saved to `.gmail-automation/exclusions.json` and
//! persist across runs.

use crate::error::{GmailError, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;

/// A persistent exclusion for a cluster
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Exclusion {
    /// The cluster key (e.g., "*@domain.com" or "email@domain.com|subject:Pattern")
    pub cluster_key: String,
    /// When this exclusion was created
    pub created_at: DateTime<Utc>,
    /// Optional reason for the exclusion
    pub reason: Option<String>,
}

/// Manager for persistent exclusions
#[derive(Debug, Default)]
pub struct ExclusionManager {
    /// Set of excluded cluster keys for fast lookup
    excluded_keys: HashSet<String>,
    /// Full exclusion records (for saving)
    exclusions: Vec<Exclusion>,
}

impl ExclusionManager {
    /// Create a new empty exclusion manager
    pub fn new() -> Self {
        Self {
            excluded_keys: HashSet::new(),
            exclusions: Vec::new(),
        }
    }

    /// Load exclusions from a JSON file
    pub async fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::new());
        }

        let json = tokio::fs::read_to_string(path).await?;
        let exclusions: Vec<Exclusion> = serde_json::from_str(&json)
            .map_err(|e| GmailError::Unknown(format!("Failed to parse exclusions file: {}", e)))?;

        let excluded_keys: HashSet<String> =
            exclusions.iter().map(|e| e.cluster_key.clone()).collect();

        Ok(Self {
            excluded_keys,
            exclusions,
        })
    }

    /// Load exclusions synchronously (for use in non-async contexts)
    pub fn load_sync(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::new());
        }

        let json = std::fs::read_to_string(path)
            .map_err(|e| GmailError::Unknown(format!("Failed to read exclusions file: {}", e)))?;
        let exclusions: Vec<Exclusion> = serde_json::from_str(&json)
            .map_err(|e| GmailError::Unknown(format!("Failed to parse exclusions file: {}", e)))?;

        let excluded_keys: HashSet<String> =
            exclusions.iter().map(|e| e.cluster_key.clone()).collect();

        Ok(Self {
            excluded_keys,
            exclusions,
        })
    }

    /// Save exclusions to a JSON file
    pub async fn save(&self, path: &Path) -> Result<()> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let json = serde_json::to_string_pretty(&self.exclusions)
            .map_err(|e| GmailError::Unknown(format!("Failed to serialize exclusions: {}", e)))?;
        tokio::fs::write(path, json).await?;
        Ok(())
    }

    /// Save exclusions synchronously (for use in non-async contexts)
    pub fn save_sync(&self, path: &Path) -> Result<()> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| GmailError::Unknown(format!("Failed to create directory: {}", e)))?;
        }

        let json = serde_json::to_string_pretty(&self.exclusions)
            .map_err(|e| GmailError::Unknown(format!("Failed to serialize exclusions: {}", e)))?;
        std::fs::write(path, json)
            .map_err(|e| GmailError::Unknown(format!("Failed to write exclusions file: {}", e)))?;
        Ok(())
    }

    /// Add an exclusion
    pub fn add(&mut self, cluster_key: String, reason: Option<String>) {
        if self.excluded_keys.contains(&cluster_key) {
            return; // Already excluded
        }

        self.excluded_keys.insert(cluster_key.clone());
        self.exclusions.push(Exclusion {
            cluster_key,
            created_at: Utc::now(),
            reason,
        });
    }

    /// Check if a cluster key is excluded
    pub fn is_excluded(&self, cluster_key: &str) -> bool {
        self.excluded_keys.contains(cluster_key)
    }

    /// Get the number of exclusions
    pub fn len(&self) -> usize {
        self.exclusions.len()
    }

    /// Check if there are no exclusions
    pub fn is_empty(&self) -> bool {
        self.exclusions.is_empty()
    }

    /// Get all exclusions
    pub fn exclusions(&self) -> &[Exclusion] {
        &self.exclusions
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_exclusion_manager_new() {
        let manager = ExclusionManager::new();
        assert!(manager.is_empty());
        assert!(!manager.is_excluded("test@example.com"));
    }

    #[test]
    fn test_exclusion_manager_add() {
        let mut manager = ExclusionManager::new();
        manager.add("*@example.com".to_string(), Some("Test reason".to_string()));

        assert_eq!(manager.len(), 1);
        assert!(manager.is_excluded("*@example.com"));
        assert!(!manager.is_excluded("*@other.com"));
    }

    #[test]
    fn test_exclusion_manager_add_duplicate() {
        let mut manager = ExclusionManager::new();
        manager.add("*@example.com".to_string(), None);
        manager.add("*@example.com".to_string(), None); // Duplicate

        assert_eq!(manager.len(), 1); // Should not add duplicate
    }

    #[tokio::test]
    async fn test_exclusion_manager_save_load() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("exclusions.json");

        let mut manager = ExclusionManager::new();
        manager.add("*@example.com".to_string(), Some("Test".to_string()));
        manager.add("specific@test.com".to_string(), None);

        manager.save(&path).await.unwrap();

        let loaded = ExclusionManager::load(&path).await.unwrap();
        assert_eq!(loaded.len(), 2);
        assert!(loaded.is_excluded("*@example.com"));
        assert!(loaded.is_excluded("specific@test.com"));
    }

    #[test]
    fn test_exclusion_manager_load_nonexistent() {
        let result = ExclusionManager::load_sync(Path::new("/nonexistent/path"));
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }
}
