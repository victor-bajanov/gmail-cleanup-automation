//! Email scanning commands

use crate::events::{AppHandleExt, ScanPhase, ScanProgress};
use crate::state::AppState;
use gmail_automation::{
    create_clusters, Classification, EmailCategory, EmailClassifier, EmailCluster, GmailClient,
    MessageMetadata,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::{AppHandle, State};

/// Scan configuration options
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanOptions {
    /// Number of days to scan back
    pub period_days: u32,
    /// Minimum emails per cluster
    pub min_cluster_size: usize,
    /// Optional search query to filter messages
    pub query: Option<String>,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            period_days: 30,
            min_cluster_size: 3,
            query: None,
        }
    }
}

/// Scan result summary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanResult {
    /// Total messages found
    pub total_messages: usize,
    /// Messages successfully fetched
    pub fetched_messages: usize,
    /// Number of clusters created
    pub cluster_count: usize,
    /// Unique domains found
    pub unique_domains: usize,
}

/// Starts scanning emails
#[tauri::command]
pub async fn scan_emails(
    app: AppHandle,
    options: ScanOptions,
    state: State<'_, AppState>,
) -> Result<ScanResult, String> {
    let client = state
        .get_client()
        .ok_or("Gmail client not initialized. Please authenticate first.")?;

    let events = app.events();

    // Build search query
    let query = options.query.clone().unwrap_or_else(|| {
        format!("newer_than:{}d", options.period_days)
    });

    // Phase 1: List message IDs
    events.emit_scan_progress(ScanProgress {
        phase: ScanPhase::Listing,
        current: 0,
        total: 0,
        message: Some("Listing messages...".to_string()),
    });

    let message_ids = client
        .list_message_ids(&query)
        .await
        .map_err(|e| format!("Failed to list messages: {}", e))?;

    let total = message_ids.len();
    tracing::info!("Found {} messages to scan", total);

    if total == 0 {
        return Ok(ScanResult {
            total_messages: 0,
            fetched_messages: 0,
            cluster_count: 0,
            unique_domains: 0,
        });
    }

    // Phase 2: Fetch message details
    events.emit_scan_progress(ScanProgress {
        phase: ScanPhase::Fetching,
        current: 0,
        total,
        message: Some(format!("Fetching {} messages...", total)),
    });

    // Create progress callback
    let app_clone = app.clone();
    let progress_callback: gmail_automation::client::ProgressCallback = Arc::new(move |current, total_cb| {
        app_clone.events().scan_fetching(current, total_cb);
    });

    let messages = client
        .fetch_messages_with_progress(message_ids, progress_callback)
        .await
        .map_err(|e| format!("Failed to fetch messages: {}", e))?;

    let fetched_count = messages.len();
    tracing::info!("Fetched {} messages", fetched_count);

    // Store messages in state
    state.add_messages(messages.clone());

    // Calculate unique domains
    let unique_domains: std::collections::HashSet<_> =
        messages.iter().map(|m| m.sender_domain.clone()).collect();

    // Phase 3: Create clusters
    events.emit_scan_progress(ScanProgress {
        phase: ScanPhase::Clustering,
        current: 0,
        total: fetched_count,
        message: Some("Creating clusters...".to_string()),
    });

    let clusters = create_clusters(&messages, options.min_cluster_size);
    let cluster_count = clusters.len();

    tracing::info!("Created {} clusters", cluster_count);

    // Store clusters in state
    state.set_clusters(clusters);

    // Phase 4: Complete
    events.scan_complete(fetched_count);

    Ok(ScanResult {
        total_messages: total,
        fetched_messages: fetched_count,
        cluster_count,
        unique_domains: unique_domains.len(),
    })
}

/// Gets the current list of scanned messages
#[tauri::command]
pub async fn get_messages(state: State<'_, AppState>) -> Result<Vec<MessageMetadata>, String> {
    Ok(state.get_messages())
}

/// Gets message count by domain
#[tauri::command]
pub async fn get_domain_stats(
    state: State<'_, AppState>,
) -> Result<Vec<DomainStat>, String> {
    let messages = state.get_messages();
    let mut domain_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

    for msg in &messages {
        *domain_counts.entry(msg.sender_domain.clone()).or_insert(0) += 1;
    }

    let mut stats: Vec<DomainStat> = domain_counts
        .into_iter()
        .map(|(domain, count)| DomainStat { domain, count })
        .collect();

    // Sort by count descending
    stats.sort_by(|a, b| b.count.cmp(&a.count));

    Ok(stats)
}

/// Domain statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainStat {
    pub domain: String,
    pub count: usize,
}

/// Classifies messages using the rule-based classifier
#[tauri::command]
pub async fn classify_messages(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<usize, String> {
    let messages = state.get_messages();
    let config = state.get_config();

    if messages.is_empty() {
        return Err("No messages to classify. Run scan first.".to_string());
    }

    let events = app.events();
    let total = messages.len();

    events.emit_scan_progress(ScanProgress {
        phase: ScanPhase::Classifying,
        current: 0,
        total,
        message: Some("Classifying messages...".to_string()),
    });

    // Get label prefix from config
    let label_prefix = config
        .as_ref()
        .map(|c| c.label.prefix.clone())
        .unwrap_or_else(|| "AutoManaged".to_string());

    // Create classifier
    let classifier = EmailClassifier::new(&label_prefix);

    // Classify each message
    let mut classifications = Vec::new();
    for (i, msg) in messages.iter().enumerate() {
        let classification = classifier.classify(msg);
        classifications.push((msg.clone(), classification));

        if i % 100 == 0 {
            events.scan_classifying(i, total);
        }
    }

    let count = classifications.len();
    state.set_classifications(classifications);

    events.scan_complete(count);

    Ok(count)
}

/// Clears scan data
#[tauri::command]
pub async fn clear_scan_data(state: State<'_, AppState>) -> Result<(), String> {
    state.clear_messages();
    state.set_classifications(Vec::new());
    state.set_clusters(Vec::new());
    Ok(())
}
