use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::client::ExistingFilterInfo;
use crate::filter_ast::FromClause;
use crate::filter_overlap::parse_gmail_query;
use crate::models::FilterRule;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverlapGroup {
    pub group_id: String,
    pub from_pattern: String,
    pub filters: Vec<ExistingFilterInfo>,
    pub label_names: Vec<String>,
    pub resolution_type: ResolutionType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ResolutionType {
    MechanicalFix {
        proposed_replacements: Vec<FilterRule>,
    },
    PickWinner,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GroupDecision {
    ReplaceWithExclusive {
        replacement_filters: Vec<FilterRule>,
    },
    KeepOne {
        keep_filter_id: String,
    },
    Skip,
    Rescan {
        from_pattern: String,
    },
}

#[derive(Debug, Clone, Default)]
pub struct RemediationPlan {
    pub groups: Vec<(OverlapGroup, GroupDecision)>,
}

impl RemediationPlan {
    pub fn new() -> Self {
        Self { groups: vec![] }
    }

    pub fn add(&mut self, group: OverlapGroup, decision: GroupDecision) {
        self.groups.push((group, decision));
    }

    pub fn summary(&self) -> String {
        let mut deletions = 0usize;
        let mut creations = 0usize;
        let mut skips = 0usize;
        let mut rescans = 0usize;

        for (group, decision) in &self.groups {
            match decision {
                GroupDecision::ReplaceWithExclusive {
                    replacement_filters,
                } => {
                    deletions += group.filters.len();
                    creations += replacement_filters.len();
                }
                GroupDecision::KeepOne { .. } => {
                    // Delete all but the kept one
                    if group.filters.len() > 1 {
                        deletions += group.filters.len() - 1;
                    }
                }
                GroupDecision::Skip => {
                    skips += 1;
                }
                GroupDecision::Rescan { .. } => {
                    rescans += 1;
                }
            }
        }

        format!(
            "{} groups: {} deletions, {} creations, {} skips, {} rescans",
            self.groups.len(),
            deletions,
            creations,
            skips,
            rescans
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemediationResult {
    pub deleted: Vec<String>,
    pub created: Vec<String>,
    pub skipped: usize,
    pub errors: Vec<String>,
}

pub struct OverlapDetector;

impl OverlapDetector {
    /// Parse the from-pattern from an ExistingFilterInfo.
    /// Returns (domain, Option<specific_sender>).
    fn parse_from_key(filter: &ExistingFilterInfo) -> Option<(String, Option<String>)> {
        // Try the `from` field first (raw value from Gmail API)
        if let Some(ref from) = filter.from {
            if from.contains('@') {
                // Specific sender — extract domain
                let domain = from.split('@').last().unwrap_or("").to_string();
                return Some((domain, Some(from.clone())));
            } else if !from.is_empty() {
                // It's a domain
                return Some((from.clone(), None));
            }
        }

        // Fall back to parsing the query field with parse_gmail_query()
        if let Some(ref query) = filter.query {
            let expr = parse_gmail_query(query);
            if let Some(ref from_clause) = expr.from_clause {
                return match from_clause {
                    FromClause::Domain(dp) => Some((dp.domain.clone(), None)),
                    FromClause::SpecificSender(ep) => {
                        Some((ep.domain.clone(), Some(ep.full_address())))
                    }
                };
            }
        }

        None
    }

    /// Group filters by overlapping from-patterns.
    /// Filters sharing the same domain land in the same group.
    /// Groups of size 1 are discarded (no overlap).
    pub fn group_filters(
        filters: &[ExistingFilterInfo],
        label_map: &HashMap<String, String>,
    ) -> Vec<OverlapGroup> {
        let mut domain_groups: HashMap<String, Vec<ExistingFilterInfo>> = HashMap::new();

        for filter in filters {
            if let Some((domain, _specific_sender)) = Self::parse_from_key(filter) {
                domain_groups
                    .entry(domain)
                    .or_default()
                    .push(filter.clone());
            }
        }

        // Discard singletons, build OverlapGroup for each group
        let mut groups: Vec<OverlapGroup> = domain_groups
            .into_iter()
            .filter(|(_, group_filters)| group_filters.len() > 1)
            .map(|(domain, group_filters)| {
                let label_names: Vec<String> = group_filters
                    .iter()
                    .flat_map(|f| &f.add_label_ids)
                    .filter_map(|id| label_map.get(id).cloned())
                    .collect();

                OverlapGroup {
                    group_id: domain.clone(),
                    from_pattern: domain,
                    filters: group_filters,
                    label_names,
                    resolution_type: ResolutionType::PickWinner,
                }
            })
            .collect();

        // Sort for deterministic output
        groups.sort_by(|a, b| a.group_id.cmp(&b.group_id));
        groups
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::HashMap;
    use crate::client::ExistingFilterInfo;

    fn make_filter(id: &str, from: Option<&str>, subject: Option<&str>, label: &str) -> ExistingFilterInfo {
        ExistingFilterInfo {
            id: id.to_string(),
            query: None,
            from: from.map(|s| s.to_string()),
            to: None,
            subject: subject.map(|s| s.to_string()),
            add_label_ids: vec![label.to_string()],
            remove_label_ids: vec![],
        }
    }

    fn label_map() -> HashMap<String, String> {
        [("lbl_fin", "Financial"), ("lbl_rec", "Receipts"), ("lbl_per", "Personal"), ("lbl_oth", "Other")]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn test_group_same_domain_filters() {
        let filters = vec![
            make_filter("f1", Some("cba.com.au"), None, "lbl_fin"),
            make_filter("f2", Some("cba.com.au"), None, "lbl_rec"),
            make_filter("f3", Some("github.com"), None, "lbl_oth"),
        ];
        let groups = OverlapDetector::group_filters(&filters, &label_map());
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].group_id, "cba.com.au");
        assert_eq!(groups[0].filters.len(), 2);
    }

    #[test]
    fn test_group_domain_subsumes_specific_sender() {
        let filters = vec![
            make_filter("f1", Some("cba.com.au"), None, "lbl_fin"),
            make_filter("f2", Some("noreply@cba.com.au"), None, "lbl_rec"),
        ];
        let groups = OverlapDetector::group_filters(&filters, &label_map());
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].filters.len(), 2);
    }

    #[test]
    fn test_no_groups_for_non_overlapping() {
        let filters = vec![
            make_filter("f1", Some("cba.com.au"), None, "lbl_fin"),
            make_filter("f2", Some("github.com"), None, "lbl_rec"),
        ];
        let groups = OverlapDetector::group_filters(&filters, &label_map());
        assert_eq!(groups.len(), 0);
    }

    #[test]
    fn test_overlap_group_display() {
        let group = OverlapGroup {
            group_id: "cba.com.au".to_string(),
            from_pattern: "cba.com.au".to_string(),
            filters: vec![],
            label_names: vec!["Financial".to_string(), "Receipts".to_string()],
            resolution_type: ResolutionType::PickWinner,
        };
        assert_eq!(group.group_id, "cba.com.au");
        assert!(matches!(group.resolution_type, ResolutionType::PickWinner));
    }

    #[test]
    fn test_plan_summary() {
        let plan = RemediationPlan::new();
        let summary = plan.summary();
        assert!(summary.contains("0 groups"));
    }
}
