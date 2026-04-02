use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::client::{ExistingFilterInfo, GmailClient};
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
    /// All filters share the same label — consolidate into one broad filter.
    Consolidate {
        /// The filter to keep (broadest one, no subject restriction).
        keep_filter_id: String,
        /// IDs of filters to delete.
        remove_filter_ids: Vec<String>,
    },
    MechanicalFix {
        proposed_replacements: Vec<FilterRule>,
    },
    PickWinner,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GroupDecision {
    /// Consolidate same-label filters: keep one, delete the rest.
    Consolidate {
        keep_filter_id: String,
        remove_filter_ids: Vec<String>,
    },
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

    /// Execute the remediation plan: delete old filters, create replacements.
    pub async fn execute(
        &self,
        client: &dyn GmailClient,
    ) -> crate::error::Result<RemediationResult> {
        self.execute_with_progress(client, |_, _| {}).await
    }

    /// Execute with a per-group progress callback: fn(index, group_id).
    pub async fn execute_with_progress(
        &self,
        client: &dyn GmailClient,
        on_progress: impl Fn(usize, &str),
    ) -> crate::error::Result<RemediationResult> {
        let mut result = RemediationResult {
            deleted: vec![],
            created: vec![],
            skipped: 0,
            errors: vec![],
        };

        for (i, (group, decision)) in self.groups.iter().enumerate() {
            on_progress(i, &group.group_id);
            match decision {
                GroupDecision::Skip => {
                    result.skipped += 1;
                }
                GroupDecision::KeepOne { keep_filter_id } => {
                    // Validate the keeper exists in the group
                    if !group.filters.iter().any(|f| f.id == *keep_filter_id) {
                        result.errors.push(format!(
                            "Group {}: keep_filter_id '{}' not found in group — skipping to avoid data loss",
                            group.group_id, keep_filter_id
                        ));
                        result.skipped += 1;
                        continue;
                    }
                    // Delete all except the keeper
                    for filter in &group.filters {
                        if filter.id != *keep_filter_id {
                            match client.delete_filter(&filter.id).await {
                                Ok(()) => result.deleted.push(filter.id.clone()),
                                Err(e) => result.errors.push(format!(
                                    "Failed to delete filter {} in group {}: {}",
                                    filter.id, group.group_id, e
                                )),
                            }
                        }
                    }
                }
                GroupDecision::ReplaceWithExclusive {
                    replacement_filters,
                } => {
                    // Delete all existing filters in the group first
                    let mut delete_failed = false;
                    for filter in &group.filters {
                        match client.delete_filter(&filter.id).await {
                            Ok(()) => result.deleted.push(filter.id.clone()),
                            Err(e) => {
                                result.errors.push(format!(
                                    "Failed to delete filter {} in group {}: {}",
                                    filter.id, group.group_id, e
                                ));
                                delete_failed = true;
                            }
                        }
                    }
                    // Only create replacements if all deletes succeeded
                    if delete_failed {
                        result.errors.push(format!(
                            "Skipping replacement creation for group {} due to delete failures",
                            group.group_id
                        ));
                    } else {
                        for replacement in replacement_filters {
                            match client.create_filter(replacement).await {
                                Ok(id) => result.created.push(id),
                                Err(e) => result.errors.push(format!(
                                    "Failed to create replacement filter in group {}: {}",
                                    group.group_id, e
                                )),
                            }
                        }
                    }
                }
                GroupDecision::Consolidate {
                    keep_filter_id,
                    remove_filter_ids,
                } => {
                    // Validate the keeper exists
                    if !group.filters.iter().any(|f| f.id == *keep_filter_id) {
                        result.errors.push(format!(
                            "Group {}: consolidate keep_filter_id '{}' not found — skipping",
                            group.group_id, keep_filter_id
                        ));
                        result.skipped += 1;
                        continue;
                    }
                    for fid in remove_filter_ids {
                        match client.delete_filter(fid).await {
                            Ok(()) => result.deleted.push(fid.clone()),
                            Err(e) => result.errors.push(format!(
                                "Failed to delete filter {} in group {}: {}",
                                fid, group.group_id, e
                            )),
                        }
                    }
                }
                GroupDecision::Rescan { .. } => {
                    // Rescan should have been resolved before execution
                    result.errors.push(format!(
                        "Group {} has unresolved Rescan decision — skipping",
                        group.group_id
                    ));
                    result.skipped += 1;
                }
            }
        }

        Ok(result)
    }

    pub fn summary(&self) -> String {
        let mut lines = vec![];
        let mut total_delete = 0usize;
        let mut total_create = 0usize;
        let mut total_skip = 0usize;

        for (group, decision) in &self.groups {
            match decision {
                GroupDecision::Consolidate {
                    remove_filter_ids, ..
                } => {
                    let del = remove_filter_ids.len();
                    lines.push(format!(
                        "  {} — consolidate (same label), delete {} redundant filters",
                        group.group_id, del
                    ));
                    total_delete += del;
                }
                GroupDecision::ReplaceWithExclusive {
                    replacement_filters,
                } => {
                    let del = group.filters.len();
                    let cre = replacement_filters.len();
                    lines.push(format!(
                        "  {} — delete {} filters, create {} exclusive replacements",
                        group.group_id, del, cre
                    ));
                    total_delete += del;
                    total_create += cre;
                }
                GroupDecision::KeepOne { keep_filter_id } => {
                    let del = group.filters.len().saturating_sub(1);
                    lines.push(format!(
                        "  {} — keep filter {}, delete {} others",
                        group.group_id, keep_filter_id, del
                    ));
                    total_delete += del;
                }
                GroupDecision::Skip => {
                    lines.push(format!("  {} — skip", group.group_id));
                    total_skip += 1;
                }
                GroupDecision::Rescan { .. } => {
                    lines.push(format!(
                        "  {} — rescan sender emails and regenerate filters",
                        group.group_id
                    ));
                }
            }
        }

        let header = format!(
            "Remediation plan: {} groups, {} deletions, {} creations, {} skipped",
            self.groups.len(),
            total_delete,
            total_create,
            total_skip
        );
        std::iter::once(header)
            .chain(lines)
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemediationResult {
    pub deleted: Vec<String>,
    pub created: Vec<String>,
    pub skipped: usize,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabelSwap {
    pub from_pattern: String,
    pub add_label_id: String,
    pub remove_label_ids: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ApplyResult {
    pub messages_relabeled: usize,
    pub messages_failed: usize,
    pub errors: Vec<String>,
}

pub struct RemediationApplicator;

impl RemediationApplicator {
    pub async fn apply(
        client: &dyn GmailClient,
        swaps: &[LabelSwap],
    ) -> crate::error::Result<ApplyResult> {
        let mut result = ApplyResult::default();

        for swap in swaps {
            let query = format!("from:{}", swap.from_pattern);
            let message_ids = match client.list_message_ids(&query).await {
                Ok(ids) => ids,
                Err(e) => {
                    result.errors.push(format!(
                        "Failed to query messages for {}: {}",
                        swap.from_pattern, e
                    ));
                    continue;
                }
            };

            if message_ids.is_empty() {
                continue;
            }

            match client
                .batch_modify_labels(
                    &message_ids,
                    std::slice::from_ref(&swap.add_label_id),
                    &swap.remove_label_ids,
                )
                .await
            {
                Ok(count) => result.messages_relabeled += count,
                Err(e) => {
                    result.messages_failed += message_ids.len();
                    result.errors.push(format!(
                        "Failed to swap labels for {} ({} messages): {}",
                        swap.from_pattern,
                        message_ids.len(),
                        e
                    ));
                }
            }
        }

        Ok(result)
    }

    pub fn collect_swaps(plan: &RemediationPlan) -> Vec<LabelSwap> {
        let mut swaps = Vec::new();

        for (group, decision) in &plan.groups {
            if let GroupDecision::KeepOne { keep_filter_id } = decision {
                let winner_label = group
                    .filters
                    .iter()
                    .find(|f| f.id == *keep_filter_id)
                    .and_then(|f| f.add_label_ids.first().cloned());

                let Some(winner) = winner_label else {
                    continue;
                };

                let loser_labels: Vec<String> = group
                    .filters
                    .iter()
                    .filter(|f| f.id != *keep_filter_id)
                    .flat_map(|f| f.add_label_ids.iter().cloned())
                    .filter(|l| *l != winner)
                    .collect::<std::collections::HashSet<_>>()
                    .into_iter()
                    .collect();

                if !loser_labels.is_empty() {
                    swaps.push(LabelSwap {
                        from_pattern: group.from_pattern.clone(),
                        add_label_id: winner,
                        remove_label_ids: loser_labels,
                    });
                }
            }
        }

        swaps
    }
}

pub struct OverlapDetector;

impl OverlapDetector {
    /// Fetch existing filters from Gmail and detect overlap groups.
    /// This is the main entry point for the detection phase.
    pub async fn detect(
        client: &dyn GmailClient,
        label_map: &HashMap<String, String>,
    ) -> crate::error::Result<Vec<OverlapGroup>> {
        let filters = client.list_filters().await?;
        let mut groups = Self::group_filters(&filters, label_map);
        Self::classify_groups(&mut groups);
        Ok(groups)
    }

    /// Parse the from-pattern from an ExistingFilterInfo.
    /// Returns (domain, Option<specific_sender>).
    fn parse_from_key(filter: &ExistingFilterInfo) -> Option<(String, Option<String>)> {
        // Try the `from` field first (raw value from Gmail API)
        if let Some(ref from) = filter.from {
            let from_normalized = from.trim().to_lowercase();
            if from_normalized.contains('@') {
                // Specific sender — extract domain
                let domain = from_normalized
                    .split('@')
                    .next_back()
                    .unwrap_or("")
                    .to_string();
                return Some((domain, Some(from_normalized)));
            } else if !from_normalized.is_empty() {
                // It's a domain
                return Some((from_normalized, None));
            }
        }

        // Fall back to parsing the query field with parse_gmail_query()
        if let Some(ref query) = filter.query {
            let expr = parse_gmail_query(query);
            if let Some(ref from_clause) = expr.from_clause {
                return match from_clause {
                    FromClause::Domain(dp) => {
                        Some((dp.domain.to_lowercase(), None))
                    }
                    FromClause::SpecificSender(ep) => {
                        Some((ep.domain.to_lowercase(), Some(ep.full_address().to_lowercase())))
                    }
                };
            }
        }

        None
    }

    /// Classify each group as Consolidate, MechanicalFix, or PickWinner.
    /// Consolidate: all filters share the same label — keep broadest, delete rest.
    /// MechanicalFix: synthesize replacement filters with -subject: exclusions.
    /// PickWinner: user must choose which label to keep.
    pub fn classify_groups(groups: &mut [OverlapGroup]) {
        for group in groups.iter_mut() {
            // 0. Check if all filters target the same label — if so, consolidate
            let unique_labels: std::collections::HashSet<&str> = group
                .filters
                .iter()
                .flat_map(|f| f.add_label_ids.iter().map(|s| s.as_str()))
                .collect();

            if unique_labels.len() == 1 && !group.filters.is_empty() {
                // All filters go to the same label — pick the broadest one (no subject)
                let broadest = group
                    .filters
                    .iter()
                    .find(|f| {
                        f.subject.as_ref().map_or(true, |s| s.is_empty())
                            && f.query
                                .as_ref()
                                .map_or(true, |q| {
                                    let expr = parse_gmail_query(q);
                                    expr.subject_clause.is_none()
                                })
                    })
                    .unwrap_or(&group.filters[0]);

                let keep_id = broadest.id.clone();
                let remove_ids: Vec<String> = group
                    .filters
                    .iter()
                    .filter(|f| f.id != keep_id)
                    .map(|f| f.id.clone())
                    .collect();

                group.resolution_type = ResolutionType::Consolidate {
                    keep_filter_id: keep_id,
                    remove_filter_ids: remove_ids,
                };
                continue;
            }

            // 1. Collect subject keywords from filters that have them
            let mut all_subject_keywords: Vec<String> = Vec::new();
            let mut filter_subjects: Vec<(usize, Vec<String>)> = Vec::new(); // (index, keywords)

            for (i, filter) in group.filters.iter().enumerate() {
                let mut keywords: Vec<String> = Vec::new();

                // Check filter.subject field first
                if let Some(ref subj) = filter.subject {
                    if !subj.is_empty() {
                        keywords.push(subj.clone());
                    }
                }

                // Also check filter.query field for subject clause
                if keywords.is_empty() {
                    if let Some(ref query) = filter.query {
                        let expr = parse_gmail_query(query);
                        if let Some(ref subject_clause) = expr.subject_clause {
                            keywords.extend(subject_clause.keywords.clone());
                        }
                    }
                }

                if !keywords.is_empty() {
                    all_subject_keywords.extend(keywords.clone());
                }
                filter_subjects.push((i, keywords));
            }

            // 2. If no subject keywords found at all → PickWinner
            if all_subject_keywords.is_empty() {
                group.resolution_type = ResolutionType::PickWinner;
                continue;
            }

            // 3. Subject keywords exist → MechanicalFix
            let mut proposed_replacements: Vec<FilterRule> = Vec::new();

            for (i, keywords) in &filter_subjects {
                let filter = &group.filters[*i];
                let is_bare = keywords.is_empty();

                let excluded_subject_patterns = if is_bare {
                    all_subject_keywords.clone()
                } else {
                    vec![]
                };

                let from_pattern = filter.from.clone();
                let is_specific_sender = from_pattern
                    .as_ref()
                    .map(|f| f.contains('@'))
                    .unwrap_or(false);

                let target_label_id = filter
                    .add_label_ids
                    .first()
                    .cloned()
                    .unwrap_or_default();

                let should_archive = filter.remove_label_ids.contains(&"INBOX".to_string());

                proposed_replacements.push(FilterRule {
                    id: Some(filter.id.clone()),
                    name: format!("{}_{}", group.from_pattern, target_label_id),
                    from_pattern,
                    is_specific_sender,
                    excluded_senders: vec![],
                    subject_keywords: keywords.clone(),
                    excluded_subject_patterns,
                    target_label_id,
                    should_archive,
                    estimated_matches: 0,
                });
            }

            group.resolution_type = ResolutionType::MechanicalFix {
                proposed_replacements,
            };
        }
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
    fn test_classify_mechanical_fix() {
        let filters = vec![
            make_filter("f1", Some("cba.com.au"), Some("statement"), "lbl_fin"),
            make_filter("f2", Some("cba.com.au"), None, "lbl_oth"),
        ];
        let mut groups = OverlapDetector::group_filters(&filters, &label_map());
        OverlapDetector::classify_groups(&mut groups);
        assert_eq!(groups.len(), 1);
        assert!(matches!(groups[0].resolution_type, ResolutionType::MechanicalFix { .. }));
    }

    #[test]
    fn test_classify_pick_winner() {
        let filters = vec![
            make_filter("f1", Some("cba.com.au"), None, "lbl_fin"),
            make_filter("f2", Some("cba.com.au"), None, "lbl_rec"),
        ];
        let mut groups = OverlapDetector::group_filters(&filters, &label_map());
        OverlapDetector::classify_groups(&mut groups);
        assert_eq!(groups.len(), 1);
        assert!(matches!(groups[0].resolution_type, ResolutionType::PickWinner));
    }

    #[test]
    fn test_classify_consolidate_same_label() {
        // Two filters for same domain, same label → Consolidate
        let filters = vec![
            make_filter("f1", Some("accounts.google.com"), Some("Security alert"), "lbl_fin"),
            make_filter("f2", Some("accounts.google.com"), None, "lbl_fin"),
        ];
        let mut groups = OverlapDetector::group_filters(&filters, &label_map());
        OverlapDetector::classify_groups(&mut groups);
        assert_eq!(groups.len(), 1);
        match &groups[0].resolution_type {
            ResolutionType::Consolidate { keep_filter_id, remove_filter_ids } => {
                // Should keep f2 (the broad one without subject) and remove f1
                assert_eq!(keep_filter_id, "f2");
                assert_eq!(remove_filter_ids, &vec!["f1".to_string()]);
            }
            other => panic!("Expected Consolidate, got {:?}", other),
        }
    }

    #[test]
    fn test_classify_consolidate_all_bare_same_label() {
        // Multiple bare filters for same domain, same label → Consolidate (keep first)
        let filters = vec![
            make_filter("f1", Some("cba.com.au"), None, "lbl_fin"),
            make_filter("f2", Some("cba.com.au"), None, "lbl_fin"),
        ];
        let mut groups = OverlapDetector::group_filters(&filters, &label_map());
        OverlapDetector::classify_groups(&mut groups);
        assert_eq!(groups.len(), 1);
        match &groups[0].resolution_type {
            ResolutionType::Consolidate { keep_filter_id, remove_filter_ids } => {
                assert_eq!(keep_filter_id, "f1");
                assert_eq!(remove_filter_ids.len(), 1);
            }
            other => panic!("Expected Consolidate, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_execute_consolidate() {
        let group = OverlapGroup {
            group_id: "accounts.google.com".to_string(),
            from_pattern: "accounts.google.com".to_string(),
            filters: vec![
                make_filter("f1", Some("accounts.google.com"), Some("Security alert"), "lbl_fin"),
                make_filter("f2", Some("accounts.google.com"), None, "lbl_fin"),
            ],
            label_names: vec!["Financial".to_string()],
            resolution_type: ResolutionType::Consolidate {
                keep_filter_id: "f2".to_string(),
                remove_filter_ids: vec!["f1".to_string()],
            },
        };

        let mut plan = RemediationPlan::new();
        plan.add(
            group,
            GroupDecision::Consolidate {
                keep_filter_id: "f2".to_string(),
                remove_filter_ids: vec!["f1".to_string()],
            },
        );

        let client = MockExecuteClient::new();
        let result = plan.execute(&client).await.unwrap();
        assert_eq!(result.deleted, vec!["f1".to_string()]);
        assert_eq!(result.created.len(), 0);
        assert_eq!(result.skipped, 0);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn test_mechanical_fix_adds_exclusions_to_remainder() {
        let filters = vec![
            make_filter("f1", Some("cba.com.au"), Some("statement"), "lbl_fin"),
            make_filter("f2", Some("cba.com.au"), Some("receipt"), "lbl_rec"),
            make_filter("f3", Some("cba.com.au"), None, "lbl_oth"),
        ];
        let mut groups = OverlapDetector::group_filters(&filters, &label_map());
        OverlapDetector::classify_groups(&mut groups);

        if let ResolutionType::MechanicalFix { ref proposed_replacements } = groups[0].resolution_type {
            // The remainder filter should exclude "statement" and "receipt"
            let remainder = proposed_replacements.iter()
                .find(|f| f.subject_keywords.is_empty())
                .unwrap();
            assert!(remainder.excluded_subject_patterns.contains(&"statement".to_string()));
            assert!(remainder.excluded_subject_patterns.contains(&"receipt".to_string()));
            assert_eq!(proposed_replacements.len(), 3);
        } else {
            panic!("Expected MechanicalFix");
        }
    }

    use async_trait::async_trait;

    struct MockDetectClient {
        filters: Vec<ExistingFilterInfo>,
    }

    #[async_trait]
    impl crate::client::GmailClient for MockDetectClient {
        async fn list_message_ids(&self, _query: &str) -> crate::error::Result<Vec<String>> {
            unimplemented!()
        }
        async fn get_message(&self, _id: &str) -> crate::error::Result<crate::models::MessageMetadata> {
            unimplemented!()
        }
        async fn list_labels(&self) -> crate::error::Result<Vec<crate::client::LabelInfo>> {
            unimplemented!()
        }
        async fn create_label(&self, _name: &str) -> crate::error::Result<String> {
            unimplemented!()
        }
        async fn delete_label(&self, _label_id: &str) -> crate::error::Result<()> {
            unimplemented!()
        }
        async fn create_filter(&self, _filter: &crate::models::FilterRule) -> crate::error::Result<String> {
            unimplemented!()
        }
        async fn list_filters(&self) -> crate::error::Result<Vec<ExistingFilterInfo>> {
            Ok(self.filters.clone())
        }
        async fn delete_filter(&self, _filter_id: &str) -> crate::error::Result<()> {
            unimplemented!()
        }
        async fn update_filter(&self, _filter_id: &str, _filter: &crate::models::FilterRule) -> crate::error::Result<String> {
            unimplemented!()
        }
        async fn apply_label(&self, _message_id: &str, _label_id: &str) -> crate::error::Result<()> {
            unimplemented!()
        }
        async fn remove_label(&self, _message_id: &str, _label_id: &str) -> crate::error::Result<()> {
            unimplemented!()
        }
        async fn batch_remove_label(&self, _message_ids: &[String], _label_id: &str) -> crate::error::Result<usize> {
            unimplemented!()
        }
        async fn batch_add_label(&self, _message_ids: &[String], _label_id: &str) -> crate::error::Result<usize> {
            unimplemented!()
        }
        async fn batch_modify_labels(
            &self,
            _message_ids: &[String],
            _add_label_ids: &[String],
            _remove_label_ids: &[String],
        ) -> crate::error::Result<usize> {
            unimplemented!()
        }
        async fn fetch_messages_batch(&self, _message_ids: Vec<String>) -> crate::error::Result<Vec<crate::models::MessageMetadata>> {
            unimplemented!()
        }
        async fn fetch_messages_with_progress(
            &self,
            _message_ids: Vec<String>,
            _on_progress: crate::client::ProgressCallback,
        ) -> crate::error::Result<Vec<crate::models::MessageMetadata>> {
            unimplemented!()
        }
        async fn quota_stats(&self) -> crate::rate_limiter::QuotaStats {
            unimplemented!()
        }
    }

    #[tokio::test]
    async fn test_detect_returns_classified_groups() {
        let client = MockDetectClient {
            filters: vec![
                make_filter("f1", Some("cba.com.au"), Some("statement"), "lbl_fin"),
                make_filter("f2", Some("cba.com.au"), None, "lbl_oth"),
                make_filter("f3", Some("github.com"), None, "lbl_rec"),
            ],
        };
        let groups = OverlapDetector::detect(&client, &label_map()).await.unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].group_id, "cba.com.au");
        assert!(matches!(groups[0].resolution_type, ResolutionType::MechanicalFix { .. }));
    }

    #[test]
    fn test_plan_summary() {
        let plan = RemediationPlan::new();
        let summary = plan.summary();
        assert!(summary.contains("0 groups"));
        assert!(summary.contains("0 deletions"));
        assert!(summary.contains("0 creations"));
    }

    // --- MockExecuteClient and execution tests ---

    use std::sync::{Arc, Mutex};

    struct MockExecuteClient {
        deleted: Arc<Mutex<Vec<String>>>,
        created: Arc<Mutex<Vec<String>>>,
        create_counter: Arc<Mutex<usize>>,
    }

    impl MockExecuteClient {
        fn new() -> Self {
            Self {
                deleted: Arc::new(Mutex::new(vec![])),
                created: Arc::new(Mutex::new(vec![])),
                create_counter: Arc::new(Mutex::new(0)),
            }
        }
    }

    #[async_trait]
    impl crate::client::GmailClient for MockExecuteClient {
        async fn list_message_ids(&self, _query: &str) -> crate::error::Result<Vec<String>> {
            unimplemented!()
        }
        async fn get_message(&self, _id: &str) -> crate::error::Result<crate::models::MessageMetadata> {
            unimplemented!()
        }
        async fn list_labels(&self) -> crate::error::Result<Vec<crate::client::LabelInfo>> {
            unimplemented!()
        }
        async fn create_label(&self, _name: &str) -> crate::error::Result<String> {
            unimplemented!()
        }
        async fn delete_label(&self, _label_id: &str) -> crate::error::Result<()> {
            unimplemented!()
        }
        async fn create_filter(&self, _filter: &crate::models::FilterRule) -> crate::error::Result<String> {
            let mut counter = self.create_counter.lock().unwrap();
            *counter += 1;
            let id = format!("f_new_{}", counter);
            self.created.lock().unwrap().push(id.clone());
            Ok(id)
        }
        async fn list_filters(&self) -> crate::error::Result<Vec<ExistingFilterInfo>> {
            Ok(vec![])
        }
        async fn delete_filter(&self, filter_id: &str) -> crate::error::Result<()> {
            self.deleted.lock().unwrap().push(filter_id.to_string());
            Ok(())
        }
        async fn update_filter(&self, _filter_id: &str, _filter: &crate::models::FilterRule) -> crate::error::Result<String> {
            unimplemented!()
        }
        async fn apply_label(&self, _message_id: &str, _label_id: &str) -> crate::error::Result<()> {
            unimplemented!()
        }
        async fn remove_label(&self, _message_id: &str, _label_id: &str) -> crate::error::Result<()> {
            unimplemented!()
        }
        async fn batch_remove_label(&self, _message_ids: &[String], _label_id: &str) -> crate::error::Result<usize> {
            unimplemented!()
        }
        async fn batch_add_label(&self, _message_ids: &[String], _label_id: &str) -> crate::error::Result<usize> {
            unimplemented!()
        }
        async fn batch_modify_labels(
            &self,
            _message_ids: &[String],
            _add_label_ids: &[String],
            _remove_label_ids: &[String],
        ) -> crate::error::Result<usize> {
            unimplemented!()
        }
        async fn fetch_messages_batch(&self, _message_ids: Vec<String>) -> crate::error::Result<Vec<crate::models::MessageMetadata>> {
            unimplemented!()
        }
        async fn fetch_messages_with_progress(
            &self,
            _message_ids: Vec<String>,
            _on_progress: crate::client::ProgressCallback,
        ) -> crate::error::Result<Vec<crate::models::MessageMetadata>> {
            unimplemented!()
        }
        async fn quota_stats(&self) -> crate::rate_limiter::QuotaStats {
            unimplemented!()
        }
    }

    #[tokio::test]
    async fn test_execute_replace_with_exclusive() {
        let group = OverlapGroup {
            group_id: "cba.com.au".to_string(),
            from_pattern: "cba.com.au".to_string(),
            filters: vec![
                make_filter("f1", Some("cba.com.au"), Some("statement"), "lbl_fin"),
                make_filter("f2", Some("cba.com.au"), None, "lbl_oth"),
            ],
            label_names: vec!["Financial".to_string(), "Other".to_string()],
            resolution_type: ResolutionType::PickWinner,
        };

        let replacement = FilterRule {
            id: None,
            name: "test".to_string(),
            from_pattern: Some("cba.com.au".to_string()),
            is_specific_sender: false,
            excluded_senders: vec![],
            subject_keywords: vec![],
            excluded_subject_patterns: vec!["statement".to_string()],
            target_label_id: "lbl_oth".to_string(),
            should_archive: false,
            estimated_matches: 0,
        };

        let mut plan = RemediationPlan::new();
        plan.add(
            group,
            GroupDecision::ReplaceWithExclusive {
                replacement_filters: vec![replacement],
            },
        );

        let client = MockExecuteClient::new();
        let result = plan.execute(&client).await.unwrap();
        assert_eq!(result.deleted.len(), 2);
        assert_eq!(result.created.len(), 1);
        assert_eq!(result.skipped, 0);
        assert!(result.errors.is_empty());
    }

    #[tokio::test]
    async fn test_execute_keep_one() {
        let group = OverlapGroup {
            group_id: "cba.com.au".to_string(),
            from_pattern: "cba.com.au".to_string(),
            filters: vec![
                make_filter("f1", Some("cba.com.au"), None, "lbl_fin"),
                make_filter("f2", Some("cba.com.au"), None, "lbl_rec"),
            ],
            label_names: vec!["Financial".to_string(), "Receipts".to_string()],
            resolution_type: ResolutionType::PickWinner,
        };

        let mut plan = RemediationPlan::new();
        plan.add(
            group,
            GroupDecision::KeepOne {
                keep_filter_id: "f1".to_string(),
            },
        );

        let client = MockExecuteClient::new();
        let result = plan.execute(&client).await.unwrap();
        assert_eq!(result.deleted.len(), 1); // only f2 deleted
        assert_eq!(result.created.len(), 0);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn test_collect_swaps_pick_winner_produces_swap() {
        let group = OverlapGroup {
            group_id: "cba.com.au".to_string(),
            from_pattern: "cba.com.au".to_string(),
            filters: vec![
                make_filter("f1", Some("cba.com.au"), None, "lbl_fin"),
                make_filter("f2", Some("cba.com.au"), None, "lbl_rec"),
            ],
            label_names: vec!["Financial".to_string(), "Receipts".to_string()],
            resolution_type: ResolutionType::PickWinner,
        };

        let mut plan = RemediationPlan::new();
        plan.add(
            group,
            GroupDecision::KeepOne {
                keep_filter_id: "f1".to_string(),
            },
        );

        let swaps = RemediationApplicator::collect_swaps(&plan);
        assert_eq!(swaps.len(), 1);
        assert_eq!(swaps[0].from_pattern, "cba.com.au");
        assert_eq!(swaps[0].add_label_id, "lbl_fin");
        assert_eq!(swaps[0].remove_label_ids, vec!["lbl_rec".to_string()]);
    }

    #[test]
    fn test_collect_swaps_consolidate_produces_no_swap() {
        let group = OverlapGroup {
            group_id: "cba.com.au".to_string(),
            from_pattern: "cba.com.au".to_string(),
            filters: vec![
                make_filter("f1", Some("cba.com.au"), None, "lbl_fin"),
                make_filter("f2", Some("cba.com.au"), None, "lbl_fin"),
            ],
            label_names: vec!["Financial".to_string()],
            resolution_type: ResolutionType::Consolidate {
                keep_filter_id: "f1".to_string(),
                remove_filter_ids: vec!["f2".to_string()],
            },
        };

        let mut plan = RemediationPlan::new();
        plan.add(
            group,
            GroupDecision::Consolidate {
                keep_filter_id: "f1".to_string(),
                remove_filter_ids: vec!["f2".to_string()],
            },
        );

        let swaps = RemediationApplicator::collect_swaps(&plan);
        assert_eq!(swaps.len(), 0);
    }

    #[test]
    fn test_collect_swaps_skip_produces_no_swap() {
        let group = OverlapGroup {
            group_id: "cba.com.au".to_string(),
            from_pattern: "cba.com.au".to_string(),
            filters: vec![
                make_filter("f1", Some("cba.com.au"), None, "lbl_fin"),
                make_filter("f2", Some("cba.com.au"), None, "lbl_rec"),
            ],
            label_names: vec![],
            resolution_type: ResolutionType::PickWinner,
        };

        let mut plan = RemediationPlan::new();
        plan.add(group, GroupDecision::Skip);

        let swaps = RemediationApplicator::collect_swaps(&plan);
        assert_eq!(swaps.len(), 0);
    }

    // --- MockApplyClient and apply tests ---

    struct MockApplyClient {
        message_ids: HashMap<String, Vec<String>>,
        modifications: Arc<Mutex<Vec<(Vec<String>, Vec<String>, Vec<String>)>>>,
    }

    #[async_trait]
    impl crate::client::GmailClient for MockApplyClient {
        async fn list_message_ids(&self, query: &str) -> crate::error::Result<Vec<String>> {
            for (pattern, ids) in &self.message_ids {
                if query.contains(pattern) {
                    return Ok(ids.clone());
                }
            }
            Ok(vec![])
        }
        async fn get_message(&self, _id: &str) -> crate::error::Result<crate::models::MessageMetadata> {
            unimplemented!()
        }
        async fn list_labels(&self) -> crate::error::Result<Vec<crate::client::LabelInfo>> {
            unimplemented!()
        }
        async fn create_label(&self, _name: &str) -> crate::error::Result<String> {
            unimplemented!()
        }
        async fn delete_label(&self, _label_id: &str) -> crate::error::Result<()> {
            unimplemented!()
        }
        async fn create_filter(&self, _filter: &crate::models::FilterRule) -> crate::error::Result<String> {
            unimplemented!()
        }
        async fn list_filters(&self) -> crate::error::Result<Vec<ExistingFilterInfo>> {
            unimplemented!()
        }
        async fn delete_filter(&self, _filter_id: &str) -> crate::error::Result<()> {
            unimplemented!()
        }
        async fn update_filter(&self, _filter_id: &str, _filter: &crate::models::FilterRule) -> crate::error::Result<String> {
            unimplemented!()
        }
        async fn apply_label(&self, _message_id: &str, _label_id: &str) -> crate::error::Result<()> {
            unimplemented!()
        }
        async fn remove_label(&self, _message_id: &str, _label_id: &str) -> crate::error::Result<()> {
            unimplemented!()
        }
        async fn batch_remove_label(&self, _message_ids: &[String], _label_id: &str) -> crate::error::Result<usize> {
            unimplemented!()
        }
        async fn batch_add_label(&self, _message_ids: &[String], _label_id: &str) -> crate::error::Result<usize> {
            unimplemented!()
        }
        async fn batch_modify_labels(
            &self,
            message_ids: &[String],
            add_label_ids: &[String],
            remove_label_ids: &[String],
        ) -> crate::error::Result<usize> {
            let count = message_ids.len();
            self.modifications.lock().unwrap().push((
                message_ids.to_vec(),
                add_label_ids.to_vec(),
                remove_label_ids.to_vec(),
            ));
            Ok(count)
        }
        async fn fetch_messages_batch(&self, _message_ids: Vec<String>) -> crate::error::Result<Vec<crate::models::MessageMetadata>> {
            unimplemented!()
        }
        async fn fetch_messages_with_progress(
            &self,
            _message_ids: Vec<String>,
            _on_progress: crate::client::ProgressCallback,
        ) -> crate::error::Result<Vec<crate::models::MessageMetadata>> {
            unimplemented!()
        }
        async fn quota_stats(&self) -> crate::rate_limiter::QuotaStats {
            unimplemented!()
        }
    }

    #[tokio::test]
    async fn test_apply_swaps_labels() {
        let mut message_ids = HashMap::new();
        message_ids.insert(
            "cba.com.au".to_string(),
            vec!["msg1".to_string(), "msg2".to_string(), "msg3".to_string()],
        );
        let modifications = Arc::new(Mutex::new(vec![]));

        let client = MockApplyClient {
            message_ids,
            modifications: modifications.clone(),
        };

        let swaps = vec![LabelSwap {
            from_pattern: "cba.com.au".to_string(),
            add_label_id: "lbl_fin".to_string(),
            remove_label_ids: vec!["lbl_rec".to_string()],
        }];

        let result = RemediationApplicator::apply(&client, &swaps).await.unwrap();
        assert_eq!(result.messages_relabeled, 3);
        assert_eq!(result.messages_failed, 0);
        assert!(result.errors.is_empty());

        let mods = modifications.lock().unwrap();
        assert_eq!(mods.len(), 1);
        assert_eq!(mods[0].0, vec!["msg1", "msg2", "msg3"]);
        assert_eq!(mods[0].1, vec!["lbl_fin"]);
        assert_eq!(mods[0].2, vec!["lbl_rec"]);
    }

    #[tokio::test]
    async fn test_apply_empty_swaps() {
        let client = MockApplyClient {
            message_ids: HashMap::new(),
            modifications: Arc::new(Mutex::new(vec![])),
        };

        let result = RemediationApplicator::apply(&client, &[]).await.unwrap();
        assert_eq!(result.messages_relabeled, 0);
        assert_eq!(result.messages_failed, 0);
    }

    #[tokio::test]
    async fn test_execute_skip() {
        let group = OverlapGroup {
            group_id: "cba.com.au".to_string(),
            from_pattern: "cba.com.au".to_string(),
            filters: vec![],
            label_names: vec![],
            resolution_type: ResolutionType::PickWinner,
        };

        let mut plan = RemediationPlan::new();
        plan.add(group, GroupDecision::Skip);

        let client = MockExecuteClient::new();
        let result = plan.execute(&client).await.unwrap();
        assert_eq!(result.deleted.len(), 0);
        assert_eq!(result.created.len(), 0);
        assert_eq!(result.skipped, 1);
    }
}
