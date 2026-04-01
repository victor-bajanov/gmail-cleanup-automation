use serde::{Deserialize, Serialize};

use crate::client::ExistingFilterInfo;
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

#[derive(Debug, Clone)]
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

#[cfg(test)]
mod tests {
    use super::*;

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
