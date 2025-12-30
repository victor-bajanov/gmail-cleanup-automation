// Prevents additional console window on Windows in release
#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod commands;
mod events;
mod state;

use state::AppState;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

fn main() {
    // Initialize logging
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info,gmail_cleanup_gui=debug".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!("Starting Gmail Cleanup GUI");

    // Install default crypto provider for rustls (same as CLI)
    #[cfg(not(windows))]
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect("Failed to install default crypto provider");

    #[cfg(windows)]
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install default crypto provider");

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            // Auth commands
            commands::check_auth_status,
            commands::set_credentials_path,
            commands::authenticate,
            commands::logout,
            commands::initialize_client,
            // Scan commands
            commands::scan_emails,
            commands::get_messages,
            commands::get_domain_stats,
            commands::classify_messages,
            commands::clear_scan_data,
            // Cluster commands
            commands::get_clusters,
            commands::get_cluster,
            commands::submit_cluster_decision,
            commands::undo_last_decision,
            commands::get_review_summary,
            commands::get_decisions,
            commands::clear_decisions,
            commands::get_next_undecided_cluster,
            // Filter commands
            commands::get_existing_filters,
            commands::generate_proposed_filters,
            commands::compare_filters,
            commands::apply_filters,
            commands::delete_filter,
            // Analysis commands
            commands::analyze_filter_overlaps,
            commands::analyze_coverage,
            commands::get_overlap_matrix,
            commands::get_uncovered_emails,
            // Settings commands
            commands::get_settings,
            commands::save_settings,
            commands::reset_settings,
            commands::get_window_state,
            commands::save_window_state,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
