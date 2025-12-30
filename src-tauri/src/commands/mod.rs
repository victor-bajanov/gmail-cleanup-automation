//! Tauri command implementations
//!
//! This module contains all the Tauri commands that the frontend can invoke.
//! Commands are organized by functionality:
//!
//! - `auth` - Authentication and session management
//! - `scan` - Email scanning and message fetching
//! - `filters` - Filter management and creation
//! - `clusters` - Cluster review and decision making
//! - `analysis` - Overlap detection and coverage analysis
//! - `settings` - Application settings and configuration
//! - `hidden_filters` - Persistence for hidden filter IDs

pub mod analysis;
pub mod auth;
pub mod clusters;
pub mod filters;
pub mod hidden_filters;
pub mod scan;
pub mod settings;

// Re-export all commands for easy registration
pub use analysis::*;
pub use auth::*;
pub use clusters::*;
pub use filters::*;
pub use scan::*;
pub use settings::*;
