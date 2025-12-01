//! Gmail Automation System
//!
//! An intelligent automation system that scans historical emails, classifies them,
//! and creates organized filters and labels to maintain inbox hygiene.
//!
//! # Overview
//!
//! This library provides a complete solution for Gmail automation:
//! - **Authentication**: OAuth2 authentication with token caching
//! - **Scanning**: Efficient concurrent email scanning with checkpointing
//! - **Classification**: Rule-based and ML-based email categorization
//! - **Label Management**: Hierarchical label creation and management
//! - **Filter Generation**: Automatic Gmail filter rule generation
//! - **State Management**: Persistent state with resume capability
//!
//! # Example Usage
//!
//! ```no_run
//! use gmail_automation::{auth, client::ProductionGmailClient, config::Config};
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     // Load configuration
//!     let config = Config::load("config.toml".as_ref()).await?;
//!
//!     // Authenticate
//!     let hub = auth::initialize_gmail_hub(
//!         "credentials.json".as_ref(),
//!         ".gmail-automation/token.json".as_ref()
//!     ).await?;
//!
//!     // Create rate-limited client
//!     let client = ProductionGmailClient::new(hub, config.scan.max_concurrent_requests);
//!
//!     // Use client to interact with Gmail API
//!     // ...
//!
//!     Ok(())
//! }
//! ```
//!
//! # Module Organization
//!
//! - [`auth`] - OAuth2 authentication and Gmail API initialization
//! - [`client`] - Rate-limited Gmail API client with retry logic
//! - [`classifier`] - Email classification (rule-based and ML)
//! - [`cli`] - Command-line interface and pipeline orchestration
//! - [`config`] - Configuration management
//! - [`error`] - Error types and result aliases
//! - [`filter_manager`] - Gmail filter rule generation and management
//! - [`label_manager`] - Gmail label creation and hierarchy management
//! - [`models`] - Core data structures
//! - [`scanner`] - Email scanning with concurrent fetching
//! - [`state`] - Processing state management with checkpointing

pub mod auth;
pub mod client;
pub mod classifier;
pub mod cli;
pub mod config;
pub mod error;
pub mod exclusions;
pub mod filter_manager;
pub mod interactive;
pub mod label_manager;
pub mod models;
pub mod scanner;
pub mod state;

// Re-export commonly used types for convenience
pub use error::{GmailError, Result};

// Core data models
pub use models::{
    Classification, EmailCategory, FilterRule, MessageMetadata,
};

// Classifier types
pub use classifier::{DomainStats, EmailClassifier};

// Scanner types
pub use scanner::{MessageFormat, ScanCheckpoint};

// Config types
pub use config::{
    ClassificationConfig, ClaudeAgentsConfig, Config, ExecutionConfig, LabelConfig, ScanConfig,
};

// Client traits
pub use client::{GmailClient, ProductionGmailClient, RateLimitedGmailClient};

// Manager types
pub use filter_manager::FilterManager;
pub use label_manager::LabelManager;

// State management
pub use state::{ProcessingPhase, ProcessingState};

// CLI types (for binary usage)
pub use cli::{Cli, Commands, ProgressReporter, Report};

// Interactive review types
pub use interactive::{create_clusters, ClusterDecision, DecisionAction, EmailCluster, ReviewSession};
