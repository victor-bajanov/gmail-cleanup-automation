//! Analysis commands for filter overlap detection and coverage

use crate::state::AppState;
use gmail_automation::{
    filter_ast::{Filter, FilterActions, FilterExpr},
    filter_overlap::{
        AnalysisResult, ConflictSeverity, ConflictType, FilterConflict, FilterOverlapAnalyzer,
        PatternRelation,
    },
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
    pub conflict_type: String,
    pub severity: String,
    pub description: String,
    pub suggestions: Vec<String>,
}

impl From<&FilterConflict> for ConflictView {
    fn from(c: &FilterConflict) -> Self {
        Self {
            filter_a_id: c.filter_a_id.clone(),
            filter_b_id: c.filter_b_id.clone(),
            filter_a_name: c.filter_a_name.clone(),
            filter_b_name: c.filter_b_name.clone(),
            conflict_type: c.conflict_type.name().to_string(),
            severity: c.severity.name().to_string(),
            description: c.description.clone(),
            suggestions: c.resolution_suggestions.clone(),
        }
    }
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

    // Convert existing filters to AST format
    let mut filters: Vec<Filter> = existing
        .iter()
        .map(|f| {
            let query = f.query.clone().unwrap_or_default();
            let expr = gmail_automation::filter_overlap::parse_gmail_query(&query);
            let label = f
                .add_label_ids
                .as_ref()
                .and_then(|ids| ids.first())
                .cloned()
                .unwrap_or_default();

            Filter::new(
                &f.id,
                format!("Existing: {}", &f.id),
                expr,
                if f.should_archive {
                    FilterActions::label_and_archive(&label)
                } else {
                    FilterActions::with_label(&label)
                },
            )
        })
        .collect();

    // Add proposed filters
    for (i, f) in proposed.iter().enumerate() {
        let query = FilterManager::build_gmail_query_static(f);
        let expr = gmail_automation::filter_overlap::parse_gmail_query(&query);

        filters.push(Filter::new(
            format!("proposed-{}", i),
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

    let conflicts: Vec<ConflictView> = result.conflicts.iter().map(ConflictView::from).collect();

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

/// Analyzes filter coverage
#[tauri::command]
pub async fn analyze_coverage(state: State<'_, AppState>) -> Result<CoverageAnalysis, String> {
    let messages = state.get_messages();
    let proposed = state.get_proposed_filters();

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

    // Check coverage for each filter
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

        let mut matched_count = 0;

        for msg in &messages {
            let matches = if is_domain {
                msg.sender_domain.to_lowercase() == domain_pattern
            } else {
                msg.sender_email.to_lowercase() == email_pattern
            };

            if matches {
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
            filter_name: filter.name.clone(),
            filter_query: FilterManager::build_gmail_query_static(filter),
            email_count: matched_count,
            percentage,
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
