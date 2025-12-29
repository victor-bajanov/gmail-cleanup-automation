//! Event emission helpers for real-time progress updates
//!
//! This module provides typed event emission for communicating progress
//! from the Rust backend to the SolidJS frontend.

use serde::Serialize;
use tauri::{AppHandle, Emitter};

/// Progress update for email scanning
#[derive(Debug, Clone, Serialize)]
pub struct ScanProgress {
    /// Current phase of scanning
    pub phase: ScanPhase,
    /// Current item being processed
    pub current: usize,
    /// Total items to process
    pub total: usize,
    /// Optional message
    pub message: Option<String>,
}

/// Phases of the scanning process
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanPhase {
    /// Listing message IDs
    Listing,
    /// Fetching message details
    Fetching,
    /// Classifying messages
    Classifying,
    /// Creating clusters
    Clustering,
    /// Complete
    Complete,
}

/// Progress update for filter operations
#[derive(Debug, Clone, Serialize)]
pub struct FilterProgress {
    /// Current operation
    pub operation: FilterOperation,
    /// Current item being processed
    pub current: usize,
    /// Total items to process
    pub total: usize,
    /// Name of current filter being processed
    pub filter_name: Option<String>,
}

/// Filter operation types
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FilterOperation {
    /// Fetching existing filters
    FetchingExisting,
    /// Analyzing overlaps
    AnalyzingOverlaps,
    /// Creating new filters
    Creating,
    /// Applying filters retroactively
    ApplyingRetroactive,
    /// Deleting filters
    Deleting,
    /// Complete
    Complete,
}

/// Progress update for label operations
#[derive(Debug, Clone, Serialize)]
pub struct LabelProgress {
    /// Current operation
    pub operation: LabelOperation,
    /// Current item being processed
    pub current: usize,
    /// Total items to process
    pub total: usize,
    /// Name of current label
    pub label_name: Option<String>,
}

/// Label operation types
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LabelOperation {
    /// Fetching existing labels
    Fetching,
    /// Creating new labels
    Creating,
    /// Applying labels to messages
    Applying,
    /// Complete
    Complete,
}

/// Authentication status update
#[derive(Debug, Clone, Serialize)]
pub struct AuthEvent {
    /// Whether authentication succeeded
    pub success: bool,
    /// User's email address (if authenticated)
    pub email: Option<String>,
    /// Error message (if failed)
    pub error: Option<String>,
}

/// Cluster review event
#[derive(Debug, Clone, Serialize)]
pub struct ClusterEvent {
    /// Event type
    pub event_type: ClusterEventType,
    /// Cluster index
    pub cluster_index: usize,
    /// Additional data
    pub data: Option<serde_json::Value>,
}

/// Cluster event types
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClusterEventType {
    /// Cluster selected for review
    Selected,
    /// Decision made on cluster
    DecisionMade,
    /// Decision undone
    DecisionUndone,
    /// All clusters reviewed
    AllReviewed,
}

/// Error event for frontend notification
#[derive(Debug, Clone, Serialize)]
pub struct ErrorEvent {
    /// Error code for programmatic handling
    pub code: String,
    /// Human-readable error message
    pub message: String,
    /// Whether the error is recoverable
    pub recoverable: bool,
}

/// Event emitter helper for typed events
pub struct EventEmitter<'a> {
    app: &'a AppHandle,
}

impl<'a> EventEmitter<'a> {
    /// Creates a new event emitter
    pub fn new(app: &'a AppHandle) -> Self {
        Self { app }
    }

    /// Emits a scan progress event
    pub fn emit_scan_progress(&self, progress: ScanProgress) {
        let _ = self.app.emit("scan:progress", progress);
    }

    /// Emits a filter progress event
    pub fn emit_filter_progress(&self, progress: FilterProgress) {
        let _ = self.app.emit("filter:progress", progress);
    }

    /// Emits a label progress event
    pub fn emit_label_progress(&self, progress: LabelProgress) {
        let _ = self.app.emit("label:progress", progress);
    }

    /// Emits an authentication event
    pub fn emit_auth(&self, event: AuthEvent) {
        let _ = self.app.emit("auth:status", event);
    }

    /// Emits a cluster event
    pub fn emit_cluster(&self, event: ClusterEvent) {
        let _ = self.app.emit("cluster:event", event);
    }

    /// Emits an error event
    pub fn emit_error(&self, error: ErrorEvent) {
        let _ = self.app.emit("error", error);
    }

    /// Convenience method for scan listing phase
    pub fn scan_listing(&self, current: usize, total: usize) {
        self.emit_scan_progress(ScanProgress {
            phase: ScanPhase::Listing,
            current,
            total,
            message: None,
        });
    }

    /// Convenience method for scan fetching phase
    pub fn scan_fetching(&self, current: usize, total: usize) {
        self.emit_scan_progress(ScanProgress {
            phase: ScanPhase::Fetching,
            current,
            total,
            message: None,
        });
    }

    /// Convenience method for scan classifying phase
    pub fn scan_classifying(&self, current: usize, total: usize) {
        self.emit_scan_progress(ScanProgress {
            phase: ScanPhase::Classifying,
            current,
            total,
            message: None,
        });
    }

    /// Convenience method for scan clustering phase
    pub fn scan_clustering(&self, current: usize, total: usize) {
        self.emit_scan_progress(ScanProgress {
            phase: ScanPhase::Clustering,
            current,
            total,
            message: None,
        });
    }

    /// Convenience method for scan complete
    pub fn scan_complete(&self, total: usize) {
        self.emit_scan_progress(ScanProgress {
            phase: ScanPhase::Complete,
            current: total,
            total,
            message: Some("Scan complete".to_string()),
        });
    }

    /// Convenience method for emitting errors
    pub fn error(&self, code: &str, message: &str, recoverable: bool) {
        self.emit_error(ErrorEvent {
            code: code.to_string(),
            message: message.to_string(),
            recoverable,
        });
    }
}

/// Extension trait for AppHandle to easily emit events
pub trait AppHandleExt {
    fn events(&self) -> EventEmitter;
}

impl AppHandleExt for AppHandle {
    fn events(&self) -> EventEmitter {
        EventEmitter::new(self)
    }
}
