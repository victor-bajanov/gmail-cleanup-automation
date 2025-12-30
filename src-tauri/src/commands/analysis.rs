//! Analysis commands for filter overlap detection and coverage

use crate::state::AppState;
use gmail_automation::{
    filter_ast::{Filter, FilterActions},
    filter_overlap::FilterOverlapAnalyzer,
    FilterManager, GmailClient,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tauri::State;

/// Conflict view for frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictView {
    pub filter_a_id: String,
    pub filter_b_id: String,
    pub filter_a_name: String,
    pub filter_b_name: String,
    pub filter_a_query: String,
    pub filter_b_query: String,
    pub filter_a_label: String,
    pub filter_b_label: String,
    pub conflict_type: String,
    pub severity: String,
    pub description: String,
    pub suggestions: Vec<String>,
}

/// Filter metadata for conflict display
struct FilterMeta {
    query: String,
    label: String,
}

/// Analysis result view for frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisView {
    pub total_filters: usize,
    pub conflicts: Vec<ConflictView>,
    pub error_count: usize,
    pub warning_count: usize,
    pub info_count: usize,
    pub summary: String,
}

/// Analyzes filter overlaps
#[tauri::command]
pub async fn analyze_filter_overlaps(
    include_info: Option<bool>,
    state: State<'_, AppState>,
) -> Result<AnalysisView, String> {
    let existing = state.get_existing_filters();
    let proposed = state.get_proposed_filters();

    // Build label ID -> name map for resolving cryptic label IDs
    let label_map: HashMap<String, String> = if let Some(client) = state.get_client() {
        match client.list_labels().await {
            Ok(labels) => labels.into_iter().map(|l| (l.id, l.name)).collect(),
            Err(e) => {
                tracing::warn!("Failed to fetch labels for display: {}", e);
                HashMap::new()
            }
        }
    } else {
        HashMap::new()
    };

    // Helper to resolve label ID to name
    let resolve_label = |label_id: &str| -> String {
        label_map.get(label_id).cloned().unwrap_or_else(|| label_id.to_string())
    };

    // Build metadata map for rich conflict display
    let mut filter_meta: HashMap<String, FilterMeta> = HashMap::new();

    // Convert existing filters to AST format
    let mut filters: Vec<Filter> = Vec::new();

    for f in &existing {
        // Build a human-readable query string from ALL filter criteria
        // Gmail API may return data in query field AND/OR individual fields
        let mut parts = Vec::new();

        // Add individual criteria fields first (more specific)
        if let Some(from) = &f.from {
            parts.push(format!("from:({})", from));
        }
        if let Some(to) = &f.to {
            parts.push(format!("to:({})", to));
        }
        if let Some(subject) = &f.subject {
            parts.push(format!("subject:({})", subject));
        }

        // Add query field if present (may contain additional criteria)
        if let Some(q) = &f.query {
            // Only add if not already covered by individual fields
            if !q.is_empty() {
                parts.push(q.clone());
            }
        }

        let query = parts.join(" ");

        let expr = gmail_automation::filter_overlap::parse_gmail_query(&query);
        let label_id = f
            .add_label_ids
            .first()
            .cloned()
            .unwrap_or_default();

        // Resolve label ID to human-readable name
        let label_name = resolve_label(&label_id);

        // Store metadata for this filter
        filter_meta.insert(f.id.clone(), FilterMeta {
            query: query.clone(),
            label: label_name,
        });

        // Use query as display name
        let display_name = if query.is_empty() {
            format!("Existing filter ({})", &f.id[..8.min(f.id.len())])
        } else {
            query.clone()
        };

        // ExistingFilterInfo doesn't track archive status, default to just label
        filters.push(Filter::new(
            &f.id,
            display_name,
            expr,
            FilterActions::with_label(&label_id),
        ));
    }

    // Add proposed filters
    for (i, f) in proposed.iter().enumerate() {
        let query = FilterManager::build_gmail_query_static(f);
        let expr = gmail_automation::filter_overlap::parse_gmail_query(&query);
        let filter_id = format!("proposed-{}", i);

        // Resolve label ID to human-readable name
        let label_name = resolve_label(&f.target_label_id);

        // Store metadata for this filter
        filter_meta.insert(filter_id.clone(), FilterMeta {
            query: query.clone(),
            label: label_name,
        });

        filters.push(Filter::new(
            filter_id,
            f.name.clone(),
            expr,
            if f.should_archive {
                FilterActions::label_and_archive(&f.target_label_id)
            } else {
                FilterActions::with_label(&f.target_label_id)
            },
        ));
    }

    // Create analyzer
    let analyzer = if include_info.unwrap_or(true) {
        FilterOverlapAnalyzer::new()
    } else {
        FilterOverlapAnalyzer::warnings_and_errors_only()
    };

    // Run analysis
    let result = analyzer.analyze_all(filters);

    let error_count = result.error_count();
    let warning_count = result.warning_count();
    let info_count = result.conflicts.len() - error_count - warning_count;

    // Convert conflicts with full metadata
    let conflicts: Vec<ConflictView> = result.conflicts.iter().map(|c| {
        let meta_a = filter_meta.get(&c.filter_a_id);
        let meta_b = filter_meta.get(&c.filter_b_id);

        ConflictView {
            filter_a_id: c.filter_a_id.clone(),
            filter_b_id: c.filter_b_id.clone(),
            filter_a_name: c.filter_a_name.clone(),
            filter_b_name: c.filter_b_name.clone(),
            filter_a_query: meta_a.map(|m| m.query.clone()).unwrap_or_default(),
            filter_b_query: meta_b.map(|m| m.query.clone()).unwrap_or_default(),
            filter_a_label: meta_a.map(|m| m.label.clone()).unwrap_or_default(),
            filter_b_label: meta_b.map(|m| m.label.clone()).unwrap_or_default(),
            conflict_type: c.conflict_type.name().to_string(),
            severity: c.severity.name().to_string(),
            description: c.description.clone(),
            suggestions: c.resolution_suggestions.clone(),
        }
    }).collect();

    Ok(AnalysisView {
        total_filters: result.total_filters,
        conflicts,
        error_count,
        warning_count,
        info_count,
        summary: result.summary(),
    })
}

/// Coverage statistics per filter
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterCoverage {
    pub filter_name: String,
    pub filter_query: String,
    pub email_count: usize,
    pub percentage: f64,
    pub is_existing: bool,
}

/// Coverage analysis result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageAnalysis {
    pub total_emails: usize,
    pub covered_emails: usize,
    pub uncovered_emails: usize,
    pub coverage_percentage: f64,
    pub filter_coverage: Vec<FilterCoverage>,
    pub top_uncovered_domains: Vec<DomainCount>,
}

/// Domain count for uncovered emails
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainCount {
    pub domain: String,
    pub count: usize,
}

/// Extract from pattern from a Gmail query string
/// e.g., "from:(*@github.com)" -> "*@github.com"
fn extract_from_pattern_for_coverage(query: &str) -> Option<String> {
    let query_lower = query.to_lowercase();

    // Try "from:(...)" format
    if let Some(start) = query_lower.find("from:(") {
        let rest = &query[start + 6..];
        if let Some(end) = rest.find(')') {
            let pattern = rest[..end].trim().to_string();
            // Handle exclusions - take just the first part
            let clean = pattern.split_whitespace().next().unwrap_or(&pattern);
            return Some(clean.to_string());
        }
    }

    // Try "from:..." format without parens
    if let Some(start) = query_lower.find("from:") {
        let rest = &query[start + 5..];
        let pattern = rest.split_whitespace().next().unwrap_or(rest);
        if !pattern.is_empty() && !pattern.starts_with('(') {
            return Some(pattern.to_string());
        }
    }

    None
}

/// Analyzes filter coverage
#[tauri::command]
pub async fn analyze_coverage(state: State<'_, AppState>) -> Result<CoverageAnalysis, String> {
    let messages = state.get_messages();
    let proposed = state.get_proposed_filters();
    let existing = state.get_existing_filters();

    if messages.is_empty() {
        return Ok(CoverageAnalysis {
            total_emails: 0,
            covered_emails: 0,
            uncovered_emails: 0,
            coverage_percentage: 0.0,
            filter_coverage: vec![],
            top_uncovered_domains: vec![],
        });
    }

    let total = messages.len();
    let mut covered_message_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut filter_coverage = Vec::new();

    // Helper to check if a pattern matches a message
    let check_pattern_match = |from_pattern: &str, msg: &gmail_automation::MessageMetadata| -> bool {
        let pattern_lower = from_pattern.to_lowercase();
        let is_domain = pattern_lower.starts_with('*');

        if is_domain {
            let domain = pattern_lower.trim_start_matches("*@");
            msg.sender_domain.to_lowercase() == domain
        } else {
            msg.sender_email.to_lowercase() == pattern_lower
        }
    };

    // Check coverage for existing filters first
    for filter in &existing {
        // Build query from ALL available criteria (same as overlap analysis)
        let mut parts = Vec::new();
        if let Some(from) = &filter.from {
            parts.push(format!("from:({})", from));
        }
        if let Some(to) = &filter.to {
            parts.push(format!("to:({})", to));
        }
        if let Some(subject) = &filter.subject {
            parts.push(format!("subject:({})", subject));
        }
        if let Some(q) = &filter.query {
            if !q.is_empty() {
                parts.push(q.clone());
            }
        }
        let query = parts.join(" ");

        // Try to extract from pattern - check direct `from` field first, then query
        let from_pattern = filter.from.clone()
            .or_else(|| extract_from_pattern_for_coverage(&query));

        if let Some(pattern) = from_pattern {
            let mut matched_count = 0;

            for msg in &messages {
                if check_pattern_match(&pattern, msg) {
                    covered_message_ids.insert(msg.id.clone());
                    matched_count += 1;
                }
            }

            let percentage = if total > 0 {
                (matched_count as f64 / total as f64) * 100.0
            } else {
                0.0
            };

            // Create a readable name from the query
            let filter_name = if query.is_empty() {
                format!("Existing filter (no criteria)")
            } else {
                format!("Existing: {}", &query)
            };

            filter_coverage.push(FilterCoverage {
                filter_name,
                filter_query: query,
                email_count: matched_count,
                percentage,
                is_existing: true,
            });
        }
    }

    // Check coverage for proposed filters
    for filter in &proposed {
        let from_pattern = filter.from_pattern.clone().unwrap_or_default();
        if from_pattern.is_empty() {
            continue;
        }

        let mut matched_count = 0;

        for msg in &messages {
            if check_pattern_match(&from_pattern, msg) {
                covered_message_ids.insert(msg.id.clone());
                matched_count += 1;
            }
        }

        let percentage = if total > 0 {
            (matched_count as f64 / total as f64) * 100.0
        } else {
            0.0
        };

        filter_coverage.push(FilterCoverage {
            filter_name: format!("Proposed: {}", filter.name),
            filter_query: FilterManager::build_gmail_query_static(filter),
            email_count: matched_count,
            percentage,
            is_existing: false,
        });
    }

    // Sort by email count descending
    filter_coverage.sort_by(|a, b| b.email_count.cmp(&a.email_count));

    // Find uncovered emails
    let covered = covered_message_ids.len();
    let uncovered = total - covered;

    // Get top uncovered domains
    let mut uncovered_domains: HashMap<String, usize> = HashMap::new();
    for msg in &messages {
        if !covered_message_ids.contains(&msg.id) {
            *uncovered_domains.entry(msg.sender_domain.clone()).or_insert(0) += 1;
        }
    }

    let mut top_uncovered: Vec<DomainCount> = uncovered_domains
        .into_iter()
        .map(|(domain, count)| DomainCount { domain, count })
        .collect();

    top_uncovered.sort_by(|a, b| b.count.cmp(&a.count));
    top_uncovered.truncate(20);

    let coverage_percentage = if total > 0 {
        (covered as f64 / total as f64) * 100.0
    } else {
        0.0
    };

    Ok(CoverageAnalysis {
        total_emails: total,
        covered_emails: covered,
        uncovered_emails: uncovered,
        coverage_percentage,
        filter_coverage,
        top_uncovered_domains: top_uncovered,
    })
}

/// Overlap matrix entry for heatmap
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverlapMatrixEntry {
    pub filter_a_index: usize,
    pub filter_b_index: usize,
    pub filter_a_name: String,
    pub filter_b_name: String,
    pub relation: String,
    pub overlap_count: usize,
}

/// Gets the overlap matrix for heatmap visualization
#[tauri::command]
pub async fn get_overlap_matrix(state: State<'_, AppState>) -> Result<OverlapMatrixResult, String> {
    let proposed = state.get_proposed_filters();
    let messages = state.get_messages();

    if proposed.is_empty() {
        return Ok(OverlapMatrixResult {
            filter_names: vec![],
            matrix: vec![],
        });
    }

    let filter_names: Vec<String> = proposed.iter().map(|f| f.name.clone()).collect();
    let mut matrix = Vec::new();

    // Build message sets for each filter
    let mut filter_message_sets: Vec<std::collections::HashSet<String>> = Vec::new();

    for filter in &proposed {
        let from_pattern = filter.from_pattern.clone().unwrap_or_default();
        let is_domain = from_pattern.starts_with('*');

        let domain_pattern = if is_domain {
            from_pattern.trim_start_matches("*@").to_lowercase()
        } else {
            String::new()
        };

        let email_pattern = if !is_domain {
            from_pattern.to_lowercase()
        } else {
            String::new()
        };

        let mut message_set = std::collections::HashSet::new();

        for msg in &messages {
            let matches = if is_domain {
                msg.sender_domain.to_lowercase() == domain_pattern
            } else {
                msg.sender_email.to_lowercase() == email_pattern
            };

            if matches {
                message_set.insert(msg.id.clone());
            }
        }

        filter_message_sets.push(message_set);
    }

    // Calculate overlaps
    for i in 0..proposed.len() {
        for j in (i + 1)..proposed.len() {
            let overlap_count = filter_message_sets[i]
                .intersection(&filter_message_sets[j])
                .count();

            let relation = if overlap_count == 0 {
                "disjoint"
            } else if filter_message_sets[i].is_subset(&filter_message_sets[j]) {
                "subsumed_by"
            } else if filter_message_sets[j].is_subset(&filter_message_sets[i]) {
                "subsumes"
            } else {
                "overlaps"
            };

            matrix.push(OverlapMatrixEntry {
                filter_a_index: i,
                filter_b_index: j,
                filter_a_name: proposed[i].name.clone(),
                filter_b_name: proposed[j].name.clone(),
                relation: relation.to_string(),
                overlap_count,
            });
        }
    }

    Ok(OverlapMatrixResult {
        filter_names,
        matrix,
    })
}

/// Result for overlap matrix
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverlapMatrixResult {
    pub filter_names: Vec<String>,
    pub matrix: Vec<OverlapMatrixEntry>,
}

/// Gets uncovered emails for review
#[tauri::command]
pub async fn get_uncovered_emails(
    limit: Option<usize>,
    state: State<'_, AppState>,
) -> Result<Vec<UncoveredEmail>, String> {
    let messages = state.get_messages();
    let proposed = state.get_proposed_filters();

    let limit = limit.unwrap_or(100);

    // Build covered set
    let mut covered_ids: std::collections::HashSet<String> = std::collections::HashSet::new();

    for filter in &proposed {
        let from_pattern = filter.from_pattern.clone().unwrap_or_default();
        let is_domain = from_pattern.starts_with('*');

        let pattern = if is_domain {
            from_pattern.trim_start_matches("*@").to_lowercase()
        } else {
            from_pattern.to_lowercase()
        };

        for msg in &messages {
            let matches = if is_domain {
                msg.sender_domain.to_lowercase() == pattern
            } else {
                msg.sender_email.to_lowercase() == pattern
            };

            if matches {
                covered_ids.insert(msg.id.clone());
            }
        }
    }

    // Find uncovered
    let uncovered: Vec<UncoveredEmail> = messages
        .iter()
        .filter(|m| !covered_ids.contains(&m.id))
        .take(limit)
        .map(|m| UncoveredEmail {
            id: m.id.clone(),
            sender: m.sender_email.clone(),
            domain: m.sender_domain.clone(),
            subject: m.subject.clone(),
            date: m.date_received.to_rfc3339(),
        })
        .collect();

    Ok(uncovered)
}

/// Uncovered email info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UncoveredEmail {
    pub id: String,
    pub sender: String,
    pub domain: String,
    pub subject: String,
    pub date: String,
}
