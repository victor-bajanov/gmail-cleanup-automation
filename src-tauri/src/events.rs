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
    /// Creating new filters
    Creating,
    /// Complete
    Complete,
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
    /// Decision made on cluster
    DecisionMade,
    /// Decision undone
    DecisionUndone,
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

    /// Emits a cluster event
    pub fn emit_cluster(&self, event: ClusterEvent) {
        let _ = self.app.emit("cluster:event", event);
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

    /// Convenience method for scan complete
    pub fn scan_complete(&self, total: usize) {
        self.emit_scan_progress(ScanProgress {
            phase: ScanPhase::Complete,
            current: total,
            total,
            message: Some("Scan complete".to_string()),
        });
    }
}

/// Extension trait for AppHandle to easily emit events
pub trait AppHandleExt {
    fn events(&self) -> EventEmitter<'_>;
}

impl AppHandleExt for AppHandle {
    fn events(&self) -> EventEmitter<'_> {
        EventEmitter::new(self)
    }
}
