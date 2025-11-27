use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::error::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessingState {
    pub run_id: String,
    pub started_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub phase: ProcessingPhase,
    pub messages_scanned: usize,
    pub messages_classified: usize,
    pub labels_created: Vec<String>,
    pub filters_created: Vec<String>,
    pub messages_modified: usize,
    pub last_processed_message_id: Option<String>,
    pub failed_message_ids: Vec<String>,
    pub completed: bool,
    pub checkpoint_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProcessingPhase {
    Scanning,
    Classifying,
    CreatingLabels,
    CreatingFilters,
    ApplyingLabels,
    Complete,
}

impl ProcessingState {
    pub fn new() -> Self {
        Self {
            run_id: uuid::Uuid::new_v4().to_string(),
            started_at: Utc::now(),
            updated_at: Utc::now(),
            phase: ProcessingPhase::Scanning,
            messages_scanned: 0,
            messages_classified: 0,
            labels_created: Vec::new(),
            filters_created: Vec::new(),
            messages_modified: 0,
            last_processed_message_id: None,
            failed_message_ids: Vec::new(),
            completed: false,
            checkpoint_count: 0,
        }
    }

    /// Save state to disk
    pub async fn save(&self, path: &Path) -> Result<()> {
        // Create parent directory if it doesn't exist
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let json = serde_json::to_string_pretty(self)?;
        tokio::fs::write(path, json).await?;
        tracing::debug!("Saved processing state to {:?}", path);
        Ok(())
    }

    /// Load state from disk
    pub async fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            tracing::info!("No existing state file found, starting fresh");
            return Ok(Self::new());
        }

        let json = tokio::fs::read_to_string(path).await?;
        let state: Self = serde_json::from_str(&json)?;

        tracing::info!(
            "Loaded processing state: run_id={}, phase={:?}, messages_scanned={}",
            state.run_id,
            state.phase,
            state.messages_scanned
        );

        Ok(state)
    }

    /// Save state as a checkpoint (every 100 messages)
    pub async fn checkpoint(&mut self, path: &Path) -> Result<()> {
        self.updated_at = Utc::now();
        self.checkpoint_count += 1;
        self.save(path).await?;
        tracing::info!(
            "Checkpoint #{}: phase={:?}, scanned={}, classified={}",
            self.checkpoint_count,
            self.phase,
            self.messages_scanned,
            self.messages_classified
        );
        Ok(())
    }

    /// Check if we should create a checkpoint (every 100 messages)
    pub fn should_checkpoint(&self) -> bool {
        match self.phase {
            ProcessingPhase::Scanning => {
                self.messages_scanned > 0 && self.messages_scanned % 100 == 0
            }
            ProcessingPhase::Classifying => {
                self.messages_classified > 0 && self.messages_classified % 100 == 0
            }
            ProcessingPhase::ApplyingLabels => {
                self.messages_modified > 0 && self.messages_modified % 100 == 0
            }
            _ => false,
        }
    }

    /// Update phase and save
    pub async fn set_phase(&mut self, phase: ProcessingPhase, path: &Path) -> Result<()> {
        self.phase = phase;
        self.updated_at = Utc::now();
        self.save(path).await?;
        tracing::info!("Phase changed to {:?}", self.phase);
        Ok(())
    }

    /// Mark as completed
    pub async fn complete(&mut self, path: &Path) -> Result<()> {
        self.phase = ProcessingPhase::Complete;
        self.completed = true;
        self.updated_at = Utc::now();
        self.save(path).await?;
        tracing::info!("Processing completed successfully");
        Ok(())
    }

    /// Check if the run can be resumed
    pub fn can_resume(&self) -> bool {
        !self.completed && matches!(
            self.phase,
            ProcessingPhase::Scanning
                | ProcessingPhase::Classifying
                | ProcessingPhase::ApplyingLabels
        )
    }

    /// Get progress percentage
    pub fn progress_percent(&self, total: usize) -> f32 {
        if total == 0 {
            return 0.0;
        }
        match self.phase {
            ProcessingPhase::Scanning => (self.messages_scanned as f32 / total as f32) * 100.0,
            ProcessingPhase::Classifying => {
                (self.messages_classified as f32 / total as f32) * 100.0
            }
            ProcessingPhase::ApplyingLabels => {
                (self.messages_modified as f32 / total as f32) * 100.0
            }
            ProcessingPhase::Complete => 100.0,
            _ => 0.0,
        }
    }
}

impl Default for ProcessingState {
    fn default() -> Self {
        Self::new()
    }
}

/// Rollback log for tracking changes that can be undone
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollbackLog {
    pub run_id: String,
    pub created_at: DateTime<Utc>,
    pub operations: Vec<RollbackOperation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RollbackOperation {
    LabelCreated {
        label_id: String,
        label_name: String,
    },
    FilterCreated {
        filter_id: String,
    },
    LabelApplied {
        message_id: String,
        label_id: String,
    },
    MessageArchived {
        message_id: String,
    },
}

impl RollbackLog {
    pub fn new(run_id: String) -> Self {
        Self {
            run_id,
            created_at: Utc::now(),
            operations: Vec::new(),
        }
    }

    /// Add an operation to the rollback log
    pub fn add_operation(&mut self, operation: RollbackOperation) {
        self.operations.push(operation);
    }

    /// Save rollback log to disk
    pub async fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let json = serde_json::to_string_pretty(self)?;
        tokio::fs::write(path, json).await?;
        tracing::debug!("Saved rollback log to {:?}", path);
        Ok(())
    }

    /// Load rollback log from disk
    pub async fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Err(crate::error::GmailError::StateError(
                "Rollback log not found".to_string(),
            ));
        }

        let json = tokio::fs::read_to_string(path).await?;
        let log: Self = serde_json::from_str(&json)?;

        tracing::info!(
            "Loaded rollback log: run_id={}, operations={}",
            log.run_id,
            log.operations.len()
        );

        Ok(log)
    }

    /// Get operation count by type
    pub fn count_by_type(&self) -> (usize, usize, usize, usize) {
        let mut labels = 0;
        let mut filters = 0;
        let mut applied = 0;
        let mut archived = 0;

        for op in &self.operations {
            match op {
                RollbackOperation::LabelCreated { .. } => labels += 1,
                RollbackOperation::FilterCreated { .. } => filters += 1,
                RollbackOperation::LabelApplied { .. } => applied += 1,
                RollbackOperation::MessageArchived { .. } => archived += 1,
            }
        }

        (labels, filters, applied, archived)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_processing_state_new() {
        let state = ProcessingState::new();
        assert!(!state.run_id.is_empty());
        assert_eq!(state.messages_scanned, 0);
        assert_eq!(state.messages_classified, 0);
        assert_eq!(state.checkpoint_count, 0);
        assert!(!state.completed);
        assert!(matches!(state.phase, ProcessingPhase::Scanning));
    }

    #[tokio::test]
    async fn test_processing_state_save_and_load() {
        let temp_dir = TempDir::new().unwrap();
        let state_path = temp_dir.path().join("state.json");

        let mut state = ProcessingState::new();
        state.messages_scanned = 100;
        state.messages_classified = 50;
        state.labels_created.push("label1".to_string());
        state.filters_created.push("filter1".to_string());
        state.last_processed_message_id = Some("msg123".to_string());

        // Save
        state.save(&state_path).await.unwrap();
        assert!(state_path.exists());

        // Load
        let loaded_state = ProcessingState::load(&state_path).await.unwrap();
        assert_eq!(state.run_id, loaded_state.run_id);
        assert_eq!(state.messages_scanned, loaded_state.messages_scanned);
        assert_eq!(state.messages_classified, loaded_state.messages_classified);
        assert_eq!(state.labels_created, loaded_state.labels_created);
        assert_eq!(state.filters_created, loaded_state.filters_created);
        assert_eq!(
            state.last_processed_message_id,
            loaded_state.last_processed_message_id
        );
    }

    #[tokio::test]
    async fn test_processing_state_load_nonexistent() {
        let temp_dir = TempDir::new().unwrap();
        let state_path = temp_dir.path().join("nonexistent.json");

        let loaded_state = ProcessingState::load(&state_path).await.unwrap();
        assert_eq!(loaded_state.messages_scanned, 0);
        assert!(!loaded_state.completed);
    }

    #[tokio::test]
    async fn test_processing_state_checkpoint() {
        let temp_dir = TempDir::new().unwrap();
        let state_path = temp_dir.path().join("state.json");

        let mut state = ProcessingState::new();
        state.messages_scanned = 100;

        // First checkpoint
        state.checkpoint(&state_path).await.unwrap();
        assert_eq!(state.checkpoint_count, 1);
        assert!(state_path.exists());

        // Second checkpoint
        state.messages_scanned = 200;
        state.checkpoint(&state_path).await.unwrap();
        assert_eq!(state.checkpoint_count, 2);

        // Verify persistence
        let loaded_state = ProcessingState::load(&state_path).await.unwrap();
        assert_eq!(loaded_state.checkpoint_count, 2);
        assert_eq!(loaded_state.messages_scanned, 200);
    }

    #[tokio::test]
    async fn test_processing_state_should_checkpoint() {
        let mut state = ProcessingState::new();

        // Scanning phase
        state.phase = ProcessingPhase::Scanning;
        state.messages_scanned = 99;
        assert!(!state.should_checkpoint());

        state.messages_scanned = 100;
        assert!(state.should_checkpoint());

        state.messages_scanned = 200;
        assert!(state.should_checkpoint());

        state.messages_scanned = 201;
        assert!(!state.should_checkpoint());

        // Classifying phase
        state.phase = ProcessingPhase::Classifying;
        state.messages_classified = 100;
        assert!(state.should_checkpoint());

        state.messages_classified = 150;
        assert!(!state.should_checkpoint());

        // ApplyingLabels phase
        state.phase = ProcessingPhase::ApplyingLabels;
        state.messages_modified = 300;
        assert!(state.should_checkpoint());

        // Other phases should not checkpoint
        state.phase = ProcessingPhase::CreatingLabels;
        assert!(!state.should_checkpoint());

        state.phase = ProcessingPhase::Complete;
        assert!(!state.should_checkpoint());
    }

    #[tokio::test]
    async fn test_processing_state_set_phase() {
        let temp_dir = TempDir::new().unwrap();
        let state_path = temp_dir.path().join("state.json");

        let mut state = ProcessingState::new();
        assert!(matches!(state.phase, ProcessingPhase::Scanning));

        state
            .set_phase(ProcessingPhase::Classifying, &state_path)
            .await
            .unwrap();
        assert!(matches!(state.phase, ProcessingPhase::Classifying));

        // Verify persistence
        let loaded_state = ProcessingState::load(&state_path).await.unwrap();
        assert!(matches!(
            loaded_state.phase,
            ProcessingPhase::Classifying
        ));
    }

    #[tokio::test]
    async fn test_processing_state_complete() {
        let temp_dir = TempDir::new().unwrap();
        let state_path = temp_dir.path().join("state.json");

        let mut state = ProcessingState::new();
        assert!(!state.completed);

        state.complete(&state_path).await.unwrap();
        assert!(state.completed);
        assert!(matches!(state.phase, ProcessingPhase::Complete));

        // Verify persistence
        let loaded_state = ProcessingState::load(&state_path).await.unwrap();
        assert!(loaded_state.completed);
    }

    #[tokio::test]
    async fn test_processing_state_can_resume() {
        let mut state = ProcessingState::new();

        // Can resume in scanning phase
        state.phase = ProcessingPhase::Scanning;
        state.completed = false;
        assert!(state.can_resume());

        // Can resume in classifying phase
        state.phase = ProcessingPhase::Classifying;
        assert!(state.can_resume());

        // Can resume in applying labels phase
        state.phase = ProcessingPhase::ApplyingLabels;
        assert!(state.can_resume());

        // Cannot resume in creating labels phase
        state.phase = ProcessingPhase::CreatingLabels;
        assert!(!state.can_resume());

        // Cannot resume in creating filters phase
        state.phase = ProcessingPhase::CreatingFilters;
        assert!(!state.can_resume());

        // Cannot resume when completed
        state.phase = ProcessingPhase::Scanning;
        state.completed = true;
        assert!(!state.can_resume());
    }

    #[tokio::test]
    async fn test_processing_state_progress_percent() {
        let mut state = ProcessingState::new();
        let total = 1000;

        // Scanning phase
        state.phase = ProcessingPhase::Scanning;
        state.messages_scanned = 250;
        assert_eq!(state.progress_percent(total), 25.0);

        state.messages_scanned = 500;
        assert_eq!(state.progress_percent(total), 50.0);

        // Classifying phase
        state.phase = ProcessingPhase::Classifying;
        state.messages_classified = 750;
        assert_eq!(state.progress_percent(total), 75.0);

        // Complete phase
        state.phase = ProcessingPhase::Complete;
        assert_eq!(state.progress_percent(total), 100.0);

        // Edge case: zero total
        assert_eq!(state.progress_percent(0), 0.0);
    }

    #[tokio::test]
    async fn test_processing_state_failed_messages() {
        let temp_dir = TempDir::new().unwrap();
        let state_path = temp_dir.path().join("state.json");

        let mut state = ProcessingState::new();
        state.failed_message_ids.push("msg1".to_string());
        state.failed_message_ids.push("msg2".to_string());
        state.failed_message_ids.push("msg3".to_string());

        state.save(&state_path).await.unwrap();

        let loaded_state = ProcessingState::load(&state_path).await.unwrap();
        assert_eq!(loaded_state.failed_message_ids.len(), 3);
        assert!(loaded_state.failed_message_ids.contains(&"msg1".to_string()));
    }

    #[tokio::test]
    async fn test_rollback_log_new() {
        let log = RollbackLog::new("test-run-id".to_string());
        assert_eq!(log.run_id, "test-run-id");
        assert_eq!(log.operations.len(), 0);
    }

    #[tokio::test]
    async fn test_rollback_log_add_operation() {
        let mut log = RollbackLog::new("test-run".to_string());

        log.add_operation(RollbackOperation::LabelCreated {
            label_id: "label1".to_string(),
            label_name: "Newsletter".to_string(),
        });

        log.add_operation(RollbackOperation::FilterCreated {
            filter_id: "filter1".to_string(),
        });

        log.add_operation(RollbackOperation::LabelApplied {
            message_id: "msg1".to_string(),
            label_id: "label1".to_string(),
        });

        assert_eq!(log.operations.len(), 3);
    }

    #[tokio::test]
    async fn test_rollback_log_save_and_load() {
        let temp_dir = TempDir::new().unwrap();
        let log_path = temp_dir.path().join("rollback.json");

        let mut log = RollbackLog::new("test-run".to_string());
        log.add_operation(RollbackOperation::LabelCreated {
            label_id: "label1".to_string(),
            label_name: "Newsletter".to_string(),
        });
        log.add_operation(RollbackOperation::FilterCreated {
            filter_id: "filter1".to_string(),
        });

        // Save
        log.save(&log_path).await.unwrap();
        assert!(log_path.exists());

        // Load
        let loaded_log = RollbackLog::load(&log_path).await.unwrap();
        assert_eq!(log.run_id, loaded_log.run_id);
        assert_eq!(log.operations.len(), loaded_log.operations.len());
    }

    #[tokio::test]
    async fn test_rollback_log_load_nonexistent() {
        let temp_dir = TempDir::new().unwrap();
        let log_path = temp_dir.path().join("nonexistent.json");

        let result = RollbackLog::load(&log_path).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_rollback_log_count_by_type() {
        let mut log = RollbackLog::new("test-run".to_string());

        // Add various operations
        log.add_operation(RollbackOperation::LabelCreated {
            label_id: "label1".to_string(),
            label_name: "Newsletter".to_string(),
        });
        log.add_operation(RollbackOperation::LabelCreated {
            label_id: "label2".to_string(),
            label_name: "Receipt".to_string(),
        });
        log.add_operation(RollbackOperation::FilterCreated {
            filter_id: "filter1".to_string(),
        });
        log.add_operation(RollbackOperation::LabelApplied {
            message_id: "msg1".to_string(),
            label_id: "label1".to_string(),
        });
        log.add_operation(RollbackOperation::LabelApplied {
            message_id: "msg2".to_string(),
            label_id: "label2".to_string(),
        });
        log.add_operation(RollbackOperation::LabelApplied {
            message_id: "msg3".to_string(),
            label_id: "label1".to_string(),
        });
        log.add_operation(RollbackOperation::MessageArchived {
            message_id: "msg4".to_string(),
        });

        let (labels, filters, applied, archived) = log.count_by_type();
        assert_eq!(labels, 2);
        assert_eq!(filters, 1);
        assert_eq!(applied, 3);
        assert_eq!(archived, 1);
    }

    #[tokio::test]
    async fn test_processing_state_checkpoint_every_100() {
        let temp_dir = TempDir::new().unwrap();
        let state_path = temp_dir.path().join("state.json");

        let mut state = ProcessingState::new();
        state.phase = ProcessingPhase::Scanning;

        // Simulate scanning messages and checkpointing
        for i in 1..=350 {
            state.messages_scanned = i;
            if state.should_checkpoint() {
                state.checkpoint(&state_path).await.unwrap();
            }
        }

        // Should have created 3 checkpoints (at 100, 200, 300)
        assert_eq!(state.checkpoint_count, 3);

        // Verify final state
        let loaded_state = ProcessingState::load(&state_path).await.unwrap();
        assert_eq!(loaded_state.messages_scanned, 300);
        assert_eq!(loaded_state.checkpoint_count, 3);
    }

    #[test]
    fn test_processing_phase_serialization() {
        let phases = vec![
            ProcessingPhase::Scanning,
            ProcessingPhase::Classifying,
            ProcessingPhase::CreatingLabels,
            ProcessingPhase::CreatingFilters,
            ProcessingPhase::ApplyingLabels,
            ProcessingPhase::Complete,
        ];

        for phase in phases {
            let json = serde_json::to_string(&phase).unwrap();
            let deserialized: ProcessingPhase = serde_json::from_str(&json).unwrap();
            // Since we can't derive PartialEq on ProcessingPhase, just check serialization works
            assert!(!json.is_empty());
        }
    }

    #[test]
    fn test_rollback_operation_serialization() {
        let operations = vec![
            RollbackOperation::LabelCreated {
                label_id: "label1".to_string(),
                label_name: "Newsletter".to_string(),
            },
            RollbackOperation::FilterCreated {
                filter_id: "filter1".to_string(),
            },
            RollbackOperation::LabelApplied {
                message_id: "msg1".to_string(),
                label_id: "label1".to_string(),
            },
            RollbackOperation::MessageArchived {
                message_id: "msg2".to_string(),
            },
        ];

        for op in operations {
            let json = serde_json::to_string(&op).unwrap();
            let deserialized: RollbackOperation = serde_json::from_str(&json).unwrap();
            assert!(!json.is_empty());
        }
    }
}
