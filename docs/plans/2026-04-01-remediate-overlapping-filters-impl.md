# Remediate Overlapping Filters — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Detect and resolve existing overlapping Gmail filters by replacing them with mutually exclusive filters, exposed as a CLI subcommand and Tauri commands.

**Architecture:** New library module `src/filter_remediation.rs` with a detect → decide → execute pipeline. CLI gets a `Remediate` subcommand. Tauri gets 4 new commands in `src-tauri/src/commands/remediation.rs`. Reuses existing `FilterOverlapAnalyzer`, `parse_gmail_query`, `FilterManager::build_gmail_query_static`, and `GmailClient` trait.

**Tech Stack:** Rust, tokio async, serde, clap (CLI), tauri (GUI), inquire (interactive prompts), mockall (testing)

---

### Task 1: Define core data types in `src/filter_remediation.rs`

**Files:**
- Create: `src/filter_remediation.rs`
- Modify: `src/lib.rs:62-78` (add module declaration)

**Step 1: Write the failing test**

Add to the bottom of `src/filter_remediation.rs`:

```rust
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
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --lib test_overlap_group_display -- --nocapture`
Expected: FAIL — module doesn't exist yet

**Step 3: Write minimal implementation**

Create `src/filter_remediation.rs`:

```rust
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::client::ExistingFilterInfo;
use crate::models::FilterRule;

/// A group of existing Gmail filters that overlap on the same sender(s)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverlapGroup {
    /// Identifier for the group (typically the domain or sender)
    pub group_id: String,
    /// The shared from: pattern across filters in this group
    pub from_pattern: String,
    /// The overlapping filters
    pub filters: Vec<ExistingFilterInfo>,
    /// Resolved human-readable label names
    pub label_names: Vec<String>,
    /// How this group can be resolved
    pub resolution_type: ResolutionType,
}

/// How an overlap group can be resolved
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ResolutionType {
    /// Filters have distinct subject clauses — can mechanically add -subject:
    /// exclusions to the remainder filter to make them mutually exclusive
    MechanicalFix {
        /// The replacement filters with exclusions added
        proposed_replacements: Vec<FilterRule>,
    },
    /// Filters are identical in criteria (e.g., all bare from: with no subject)
    /// — user must pick which label to keep
    PickWinner,
}

/// User's decision for one overlap group
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GroupDecision {
    /// Replace all with mutually exclusive filters (mechanical fix accepted)
    ReplaceWithExclusive {
        replacement_filters: Vec<FilterRule>,
    },
    /// Keep one filter, delete the rest
    KeepOne {
        keep_filter_id: String,
    },
    /// Skip — don't touch this group
    Skip,
    /// Re-scan this sender to generate fresh exclusive filters
    Rescan {
        from_pattern: String,
    },
}

/// The full remediation plan before execution
#[derive(Debug, Clone)]
pub struct RemediationPlan {
    pub groups: Vec<(OverlapGroup, GroupDecision)>,
}

/// Result of executing the plan
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemediationResult {
    /// Filter IDs that were deleted
    pub deleted: Vec<String>,
    /// Filter IDs that were created
    pub created: Vec<String>,
    /// Number of groups skipped
    pub skipped: usize,
    /// Errors encountered (non-fatal, per-group)
    pub errors: Vec<String>,
}

impl RemediationPlan {
    pub fn new() -> Self {
        Self { groups: vec![] }
    }

    pub fn add(&mut self, group: OverlapGroup, decision: GroupDecision) {
        self.groups.push((group, decision));
    }

    /// Human-readable summary of what the plan would do
    pub fn summary(&self) -> String {
        let mut lines = vec![];
        let mut total_delete = 0;
        let mut total_create = 0;
        let mut total_skip = 0;

        for (group, decision) in &self.groups {
            match decision {
                GroupDecision::ReplaceWithExclusive { replacement_filters } => {
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
                    let del = group.filters.len() - 1;
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
```

Also add `Serialize, Deserialize` derives to `ExistingFilterInfo` in `src/client.rs:36`:

Change:
```rust
#[derive(Debug, Clone)]
pub struct ExistingFilterInfo {
```
To:
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExistingFilterInfo {
```

And add the serde import at the top of `src/client.rs` if not already present.

Add module declaration in `src/lib.rs` after `pub mod filter_overlap;`:

```rust
pub mod filter_remediation;
```

And add re-exports at the bottom of `src/lib.rs`:

```rust
pub use filter_remediation::{
    GroupDecision, OverlapGroup, RemediationPlan, RemediationResult, ResolutionType,
};
```

**Step 4: Run test to verify it passes**

Run: `cargo test --lib test_overlap_group_display -- --nocapture`
Expected: PASS

**Step 5: Commit**

```bash
git add src/filter_remediation.rs src/lib.rs src/client.rs
git commit -m "feat: add core data types for filter remediation"
```

---

### Task 2: Implement overlap detection (grouping by from-pattern)

**Files:**
- Modify: `src/filter_remediation.rs`

**Step 1: Write the failing tests**

Add to `mod tests` in `src/filter_remediation.rs`:

```rust
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
    [
        ("lbl_fin", "Financial"),
        ("lbl_rec", "Receipts"),
        ("lbl_per", "Personal"),
        ("lbl_oth", "Other"),
    ]
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
    // cba.com.au group has 2 filters, github.com has 1 (discarded)
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
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --lib test_group_ -- --nocapture`
Expected: FAIL — `OverlapDetector` doesn't exist

**Step 3: Write minimal implementation**

Add to `src/filter_remediation.rs`:

```rust
use crate::filter_ast::{EmailPattern, FromClause, DomainPattern};
use crate::filter_overlap::parse_gmail_query;

/// Detects overlap groups among existing Gmail filters
pub struct OverlapDetector;

impl OverlapDetector {
    /// Parse the from-pattern from an ExistingFilterInfo into a normalized key.
    /// Returns (domain, Option<specific_sender>).
    fn parse_from_key(filter: &ExistingFilterInfo) -> Option<(String, Option<String>)> {
        // Try the `from` field first (raw value from Gmail API)
        if let Some(ref from) = filter.from {
            let from = from.trim().to_lowercase();
            if from.contains('@') {
                // Specific sender like "noreply@cba.com.au"
                let domain = from.split('@').last()?.to_string();
                return Some((domain, Some(from)));
            } else {
                // Domain like "cba.com.au"
                return Some((from, None));
            }
        }

        // Fall back to parsing the query field
        if let Some(ref query) = filter.query {
            let expr = parse_gmail_query(query);
            match expr.from_clause {
                Some(FromClause::Domain(d)) => return Some((d.domain.to_lowercase(), None)),
                Some(FromClause::SpecificSender(e)) => {
                    return Some((e.domain.to_lowercase(), Some(e.full_address().to_lowercase())));
                }
                None => {}
            }
        }

        None
    }

    /// Group filters by overlapping from-patterns.
    /// Filters sharing the same domain (or where domain subsumes specific sender)
    /// land in the same group. Groups of size 1 are discarded.
    pub fn group_filters(
        filters: &[ExistingFilterInfo],
        label_map: &HashMap<String, String>,
    ) -> Vec<OverlapGroup> {
        // Group by domain
        let mut domain_groups: HashMap<String, Vec<ExistingFilterInfo>> = HashMap::new();

        for filter in filters {
            if let Some((domain, _specific)) = Self::parse_from_key(filter) {
                domain_groups
                    .entry(domain)
                    .or_default()
                    .push(filter.clone());
            }
        }

        // Convert to OverlapGroups, discarding singletons
        domain_groups
            .into_iter()
            .filter(|(_, group)| group.len() > 1)
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
                    resolution_type: ResolutionType::PickWinner, // placeholder, classified in next task
                }
            })
            .collect()
    }
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test --lib test_group_ -- --nocapture`
Expected: PASS

**Step 5: Commit**

```bash
git add src/filter_remediation.rs
git commit -m "feat: implement overlap grouping by from-pattern"
```

---

### Task 3: Classify groups as MechanicalFix or PickWinner

**Files:**
- Modify: `src/filter_remediation.rs`

**Step 1: Write the failing tests**

Add to `mod tests`:

```rust
#[test]
fn test_classify_mechanical_fix() {
    // One filter has subject, one is bare from: — can add exclusions
    let filters = vec![
        make_filter("f1", Some("cba.com.au"), Some("statement"), "lbl_fin"),
        make_filter("f2", Some("cba.com.au"), None, "lbl_oth"),
    ];
    let mut groups = OverlapDetector::group_filters(&filters, &label_map());
    OverlapDetector::classify_groups(&mut groups);
    assert_eq!(groups.len(), 1);
    assert!(matches!(
        groups[0].resolution_type,
        ResolutionType::MechanicalFix { .. }
    ));
}

#[test]
fn test_classify_pick_winner() {
    // Both filters are bare from: with no subject — can't synthesize
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
fn test_mechanical_fix_adds_exclusions_to_remainder() {
    let filters = vec![
        make_filter("f1", Some("cba.com.au"), Some("statement"), "lbl_fin"),
        make_filter("f2", Some("cba.com.au"), Some("receipt"), "lbl_rec"),
        make_filter("f3", Some("cba.com.au"), None, "lbl_oth"),
    ];
    let mut groups = OverlapDetector::group_filters(&filters, &label_map());
    OverlapDetector::classify_groups(&mut groups);

    if let ResolutionType::MechanicalFix { ref proposed_replacements } = groups[0].resolution_type {
        // The remainder filter (f3) should now exclude "statement" and "receipt"
        let remainder = proposed_replacements.iter().find(|f| f.subject_keywords.is_empty()).unwrap();
        assert!(remainder.excluded_subject_patterns.contains(&"statement".to_string()));
        assert!(remainder.excluded_subject_patterns.contains(&"receipt".to_string()));
        // Subject-specific filters should be unchanged
        assert_eq!(proposed_replacements.len(), 3);
    } else {
        panic!("Expected MechanicalFix");
    }
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --lib test_classify_ -- --nocapture`
Expected: FAIL — `classify_groups` doesn't exist

**Step 3: Write minimal implementation**

Add to `impl OverlapDetector`:

```rust
    /// Classify each group as MechanicalFix or PickWinner and, for MechanicalFix,
    /// synthesize the replacement filters with -subject: exclusions.
    pub fn classify_groups(groups: &mut Vec<OverlapGroup>) {
        for group in groups.iter_mut() {
            // Collect subject keywords from filters that have them
            let mut subject_keywords: Vec<String> = vec![];
            let mut has_bare_filter = false;

            for filter in &group.filters {
                if let Some(ref subject) = filter.subject {
                    let trimmed = subject.trim().to_lowercase();
                    if !trimmed.is_empty() {
                        subject_keywords.push(trimmed);
                    }
                } else {
                    has_bare_filter = true;
                }
            }

            // Also check query field for subject clauses
            for filter in &group.filters {
                if filter.subject.is_none() {
                    if let Some(ref query) = filter.query {
                        let expr = parse_gmail_query(query);
                        if let Some(ref sc) = expr.subject_clause {
                            for kw in &sc.keywords {
                                let trimmed = kw.trim().to_lowercase();
                                if !trimmed.is_empty() && !subject_keywords.contains(&trimmed) {
                                    subject_keywords.push(trimmed);
                                }
                            }
                        } else {
                            has_bare_filter = true;
                        }
                    } else {
                        has_bare_filter = true;
                    }
                }
            }

            if subject_keywords.is_empty() {
                // All filters have identical criteria — user must pick
                group.resolution_type = ResolutionType::PickWinner;
            } else {
                // At least some filters have subject clauses — synthesize exclusions
                let mut replacements: Vec<FilterRule> = vec![];

                for filter in &group.filters {
                    let filter_subject = filter
                        .subject
                        .as_deref()
                        .map(|s| s.trim().to_lowercase())
                        .unwrap_or_default();

                    let has_subject = !filter_subject.is_empty() || filter
                        .query
                        .as_ref()
                        .map(|q| parse_gmail_query(q).subject_clause.is_some())
                        .unwrap_or(false);

                    let is_specific_sender = filter
                        .from
                        .as_ref()
                        .map(|f| f.contains('@'))
                        .unwrap_or(false);

                    let mut rule = FilterRule {
                        id: Some(filter.id.clone()),
                        name: format!("remediated-{}", filter.id),
                        from_pattern: filter.from.clone(),
                        is_specific_sender,
                        excluded_senders: vec![],
                        subject_keywords: if has_subject {
                            if !filter_subject.is_empty() {
                                vec![filter_subject]
                            } else if let Some(ref query) = filter.query {
                                parse_gmail_query(query)
                                    .subject_clause
                                    .map(|sc| sc.keywords)
                                    .unwrap_or_default()
                            } else {
                                vec![]
                            }
                        } else {
                            vec![]
                        },
                        excluded_subject_patterns: vec![],
                        target_label_id: filter.add_label_ids.first().cloned().unwrap_or_default(),
                        should_archive: filter.remove_label_ids.contains(&"INBOX".to_string()),
                        estimated_matches: 0,
                    };

                    // If this is a bare/remainder filter, add exclusions for all known subjects
                    if !has_subject {
                        rule.excluded_subject_patterns = subject_keywords.clone();
                    }

                    replacements.push(rule);
                }

                group.resolution_type = ResolutionType::MechanicalFix {
                    proposed_replacements: replacements,
                };
            }
        }
    }
```

**Step 4: Run tests to verify they pass**

Run: `cargo test --lib test_classify_ -- --nocapture`
Expected: PASS

**Step 5: Commit**

```bash
git add src/filter_remediation.rs
git commit -m "feat: classify overlap groups and synthesize exclusions"
```

---

### Task 4: Implement the async `detect` entry point

**Files:**
- Modify: `src/filter_remediation.rs`

**Step 1: Write the failing test**

Add to `mod tests`:

```rust
use crate::client::GmailClient;
use crate::error::Result;
use mockall::mock;

// Only define mock if not already available
mock! {
    pub TestGmailClient {}

    #[async_trait::async_trait]
    impl GmailClient for TestGmailClient {
        async fn list_filters(&self) -> Result<Vec<ExistingFilterInfo>>;
        // Include other required trait methods as stubs — they won't be called.
        // The exact set depends on the GmailClient trait. For brevity, only
        // list_filters is shown here; add the rest as no-op stubs from the trait.
    }
}

#[tokio::test]
async fn test_detect_returns_classified_groups() {
    let mut mock = MockTestGmailClient::new();
    mock.expect_list_filters().returning(|| {
        Ok(vec![
            make_filter("f1", Some("cba.com.au"), Some("statement"), "lbl_fin"),
            make_filter("f2", Some("cba.com.au"), None, "lbl_oth"),
            make_filter("f3", Some("github.com"), None, "lbl_rec"),
        ])
    });

    let groups = OverlapDetector::detect(&mock, &label_map()).await.unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].group_id, "cba.com.au");
    assert!(matches!(
        groups[0].resolution_type,
        ResolutionType::MechanicalFix { .. }
    ));
}
```

Note: The mock definition will need all `GmailClient` trait methods. Check `src/client.rs` for the full trait and add stub expectations for each method. Only `list_filters` will actually be called — the rest can use `returning(|| unimplemented!())` or not set expectations at all (mockall panics only on unexpected calls if configured).

**Step 2: Run test to verify it fails**

Run: `cargo test --lib test_detect_returns -- --nocapture`
Expected: FAIL — `detect` method doesn't exist

**Step 3: Write minimal implementation**

Add to `impl OverlapDetector`:

```rust
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
```

**Step 4: Run test to verify it passes**

Run: `cargo test --lib test_detect_returns -- --nocapture`
Expected: PASS

**Step 5: Commit**

```bash
git add src/filter_remediation.rs
git commit -m "feat: add async detect entry point for overlap detection"
```

---

### Task 5: Implement plan execution

**Files:**
- Modify: `src/filter_remediation.rs`

**Step 1: Write the failing tests**

Add to `mod tests`:

```rust
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
        resolution_type: ResolutionType::PickWinner, // doesn't matter for execution
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
    plan.add(group, GroupDecision::ReplaceWithExclusive {
        replacement_filters: vec![replacement],
    });

    let mut mock = MockTestGmailClient::new();
    // Expect both filters to be deleted
    mock.expect_delete_filter()
        .with(mockall::predicate::eq("f1"))
        .returning(|_| Ok(()));
    mock.expect_delete_filter()
        .with(mockall::predicate::eq("f2"))
        .returning(|_| Ok(()));
    // Expect one replacement filter to be created
    mock.expect_create_filter()
        .returning(|_| Ok("f_new".to_string()));

    let result = plan.execute(&mock).await.unwrap();
    assert_eq!(result.deleted.len(), 2);
    assert_eq!(result.created.len(), 1);
    assert_eq!(result.skipped, 0);
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
    plan.add(group, GroupDecision::KeepOne {
        keep_filter_id: "f1".to_string(),
    });

    let mut mock = MockTestGmailClient::new();
    // Only f2 should be deleted (f1 is kept)
    mock.expect_delete_filter()
        .with(mockall::predicate::eq("f2"))
        .returning(|_| Ok(()));

    let result = plan.execute(&mock).await.unwrap();
    assert_eq!(result.deleted.len(), 1);
    assert_eq!(result.created.len(), 0);
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

    let mock = MockTestGmailClient::new();
    let result = plan.execute(&mock).await.unwrap();
    assert_eq!(result.deleted.len(), 0);
    assert_eq!(result.created.len(), 0);
    assert_eq!(result.skipped, 1);
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --lib test_execute_ -- --nocapture`
Expected: FAIL — `execute` method doesn't exist

**Step 3: Write minimal implementation**

Add to `impl RemediationPlan`:

```rust
    /// Execute the remediation plan: delete old filters, create replacements.
    pub async fn execute(
        &self,
        client: &dyn GmailClient,
    ) -> crate::error::Result<RemediationResult> {
        let mut result = RemediationResult {
            deleted: vec![],
            created: vec![],
            skipped: 0,
            errors: vec![],
        };

        for (group, decision) in &self.groups {
            match decision {
                GroupDecision::Skip => {
                    result.skipped += 1;
                }
                GroupDecision::KeepOne { keep_filter_id } => {
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
                GroupDecision::ReplaceWithExclusive { replacement_filters } => {
                    // Delete all existing filters in the group
                    for filter in &group.filters {
                        match client.delete_filter(&filter.id).await {
                            Ok(()) => result.deleted.push(filter.id.clone()),
                            Err(e) => result.errors.push(format!(
                                "Failed to delete filter {} in group {}: {}",
                                filter.id, group.group_id, e
                            )),
                        }
                    }
                    // Create replacements
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
                GroupDecision::Rescan { .. } => {
                    // Rescan is handled before execution — the caller should
                    // have already resolved this into ReplaceWithExclusive.
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
```

Add the import at the top of the file:

```rust
use crate::client::GmailClient;
```

**Step 4: Run tests to verify they pass**

Run: `cargo test --lib test_execute_ -- --nocapture`
Expected: PASS

**Step 5: Commit**

```bash
git add src/filter_remediation.rs
git commit -m "feat: implement remediation plan execution"
```

---

### Task 6: Add CLI `Remediate` subcommand

**Files:**
- Modify: `src/cli.rs:120-133` (add Remediate variant to Commands enum)
- Modify: `src/main.rs:134` (add match arm for Remediate)

**Step 1: Add the command variant**

In `src/cli.rs`, add before the closing `}` of the `Commands` enum (before line 133):

```rust
    /// Detect and fix overlapping Gmail filters
    Remediate {
        /// Preview changes without applying them
        #[arg(long)]
        dry_run: bool,
    },
```

**Step 2: Add the match arm in main.rs**

In `src/main.rs`, add a new match arm in the `match cli.command {` block (after the Unmanage arm):

```rust
        Commands::Remediate { dry_run } => {
            use gmail_automation::filter_remediation::{OverlapDetector, GroupDecision, RemediationPlan};

            tracing::info!("Starting filter remediation");
            if dry_run {
                println!("Running in DRY RUN mode — no changes will be made\n");
            }

            // Authenticate
            let auth_spinner = ProgressReporter::with_multi_progress((*multi_progress).clone());
            let hub = gmail_automation::auth::initialize_gmail_hub(
                &cli.credentials,
                &cli.token_cache,
            )
            .await?;

            // Create client
            let config = gmail_automation::Config::load(&cli.config).await?;
            let client = gmail_automation::ProductionGmailClient::new(
                hub,
                config.rate_limiting.max_concurrent_requests,
                &config.rate_limiting,
                &config.circuit_breaker,
            )?;

            // Build label map (id -> name)
            let labels = client.list_labels().await?;
            let label_map: std::collections::HashMap<String, String> = labels
                .iter()
                .map(|l| (l.id.clone(), l.name.clone()))
                .collect();

            // Detect overlaps
            println!("Scanning existing filters for overlaps...");
            let groups = OverlapDetector::detect(&client, &label_map).await?;

            if groups.is_empty() {
                println!("No overlapping filters found. Nothing to remediate.");
                return Ok(());
            }

            println!("Found {} overlap group(s):\n", groups.len());

            // Interactive decision-making per group
            let mut plan = RemediationPlan::new();

            for group in groups {
                println!("─── {} ───", group.group_id);
                println!("  From: {}", group.from_pattern);
                println!("  Labels: {}", group.label_names.join(", "));
                println!("  Filters: {}", group.filters.len());

                let decision = match &group.resolution_type {
                    gmail_automation::ResolutionType::MechanicalFix { proposed_replacements } => {
                        println!("  Type: Can be fixed automatically (subject exclusions)\n");
                        for r in proposed_replacements {
                            let query = gmail_automation::FilterManager::build_gmail_query_static(r);
                            let label = label_map.get(&r.target_label_id)
                                .map(|s| s.as_str())
                                .unwrap_or(&r.target_label_id);
                            println!("    → {} → {}", query, label);
                        }
                        println!();

                        let choice = inquire::Select::new(
                            "Action?",
                            vec!["Accept fix", "Skip"],
                        )
                        .prompt()
                        .unwrap_or("Skip");

                        if choice == "Accept fix" {
                            GroupDecision::ReplaceWithExclusive {
                                replacement_filters: proposed_replacements.clone(),
                            }
                        } else {
                            GroupDecision::Skip
                        }
                    }
                    gmail_automation::ResolutionType::PickWinner => {
                        println!("  Type: Identical filters — pick which label to keep\n");

                        let mut options: Vec<String> = group.filters.iter().map(|f| {
                            let label = f.add_label_ids.first()
                                .and_then(|id| label_map.get(id))
                                .map(|s| s.as_str())
                                .unwrap_or("(unknown)");
                            format!("Keep: {} (filter {})", label, f.id)
                        }).collect();
                        options.push("Rescan".to_string());
                        options.push("Skip".to_string());

                        let choice = inquire::Select::new("Action?", options.clone())
                            .prompt()
                            .unwrap_or_else(|_| "Skip".to_string());

                        if choice == "Skip" {
                            GroupDecision::Skip
                        } else if choice == "Rescan" {
                            GroupDecision::Rescan {
                                from_pattern: group.from_pattern.clone(),
                            }
                        } else {
                            // Extract filter ID from the choice string
                            let keep_id = group.filters.iter()
                                .find(|f| choice.contains(&f.id))
                                .map(|f| f.id.clone())
                                .unwrap_or_default();
                            GroupDecision::KeepOne { keep_filter_id: keep_id }
                        }
                    }
                };

                plan.add(group, decision);
            }

            // Show summary and confirm
            println!("\n{}\n", plan.summary());

            if dry_run {
                println!("Dry run complete. No changes were made.");
                return Ok(());
            }

            let confirm = inquire::Confirm::new("Execute this plan?")
                .with_default(false)
                .prompt()
                .unwrap_or(false);

            if !confirm {
                println!("Aborted.");
                return Ok(());
            }

            let result = plan.execute(&client).await?;
            println!("\nRemediation complete:");
            println!("  Deleted: {} filters", result.deleted.len());
            println!("  Created: {} filters", result.created.len());
            println!("  Skipped: {} groups", result.skipped);
            if !result.errors.is_empty() {
                println!("  Errors:");
                for e in &result.errors {
                    println!("    - {}", e);
                }
            }

            Ok(())
        }
```

**Step 2: Verify it compiles**

Run: `cargo build`
Expected: PASS (may need to adjust imports — check errors)

**Step 3: Commit**

```bash
git add src/cli.rs src/main.rs
git commit -m "feat: add CLI remediate subcommand"
```

---

### Task 7: Add Tauri remediation commands

**Files:**
- Create: `src-tauri/src/commands/remediation.rs`
- Modify: `src-tauri/src/commands/mod.rs` (add module)
- Modify: `src-tauri/src/state.rs` (add remediation state fields)
- Modify: `src-tauri/src/main.rs` (register commands)

**Step 1: Add state fields**

In `src-tauri/src/state.rs`, add to `AppState`:

```rust
pub remediation_groups: RwLock<Vec<gmail_automation::OverlapGroup>>,
pub remediation_decisions: RwLock<HashMap<String, gmail_automation::GroupDecision>>,
```

Initialize them in `AppState::new()`:

```rust
remediation_groups: RwLock::new(vec![]),
remediation_decisions: RwLock::new(HashMap::new()),
```

**Step 2: Create the commands file**

Create `src-tauri/src/commands/remediation.rs`:

```rust
use tauri::State;
use std::collections::HashMap;

use gmail_automation::filter_remediation::{
    GroupDecision, OverlapDetector, OverlapGroup, RemediationPlan, RemediationResult,
};

use crate::state::AppState;

#[tauri::command]
pub async fn detect_overlaps(
    state: State<'_, AppState>,
) -> Result<Vec<OverlapGroup>, String> {
    let client = state
        .get_client()
        .ok_or_else(|| "Not authenticated".to_string())?;

    let label_cache = state.label_cache.read().unwrap().clone();

    let groups = OverlapDetector::detect(client.as_ref(), &label_cache)
        .await
        .map_err(|e| format!("Detection failed: {}", e))?;

    // Store groups in state for later use
    *state.remediation_groups.write().unwrap() = groups.clone();
    *state.remediation_decisions.write().unwrap() = HashMap::new();

    Ok(groups)
}

#[tauri::command]
pub async fn submit_group_decision(
    group_id: String,
    decision: GroupDecision,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let mut decisions = state.remediation_decisions.write().unwrap();
    decisions.insert(group_id, decision);
    Ok(())
}

#[tauri::command]
pub async fn execute_remediation(
    state: State<'_, AppState>,
) -> Result<RemediationResult, String> {
    let client = state
        .get_client()
        .ok_or_else(|| "Not authenticated".to_string())?;

    let groups = state.remediation_groups.read().unwrap().clone();
    let decisions = state.remediation_decisions.read().unwrap().clone();

    let mut plan = RemediationPlan::new();
    for group in groups {
        let decision = decisions
            .get(&group.group_id)
            .cloned()
            .unwrap_or(GroupDecision::Skip);
        plan.add(group, decision);
    }

    let result = plan
        .execute(client.as_ref())
        .await
        .map_err(|e| format!("Execution failed: {}", e))?;

    // Clear remediation state
    *state.remediation_groups.write().unwrap() = vec![];
    *state.remediation_decisions.write().unwrap() = HashMap::new();

    Ok(result)
}

#[tauri::command]
pub async fn remediation_summary(
    state: State<'_, AppState>,
) -> Result<String, String> {
    let groups = state.remediation_groups.read().unwrap().clone();
    let decisions = state.remediation_decisions.read().unwrap().clone();

    let mut plan = RemediationPlan::new();
    for group in groups {
        let decision = decisions
            .get(&group.group_id)
            .cloned()
            .unwrap_or(GroupDecision::Skip);
        plan.add(group, decision);
    }

    Ok(plan.summary())
}
```

**Step 3: Register the module and commands**

In `src-tauri/src/commands/mod.rs`, add:

```rust
pub mod remediation;
pub use remediation::*;
```

In `src-tauri/src/main.rs`, add the command handlers to the `invoke_handler` list:

```rust
commands::detect_overlaps,
commands::submit_group_decision,
commands::execute_remediation,
commands::remediation_summary,
```

**Step 4: Verify it compiles**

Run: `cargo build -p gmail-cleanup-gui`
Expected: PASS

**Step 5: Commit**

```bash
git add src-tauri/src/commands/remediation.rs src-tauri/src/commands/mod.rs \
    src-tauri/src/state.rs src-tauri/src/main.rs
git commit -m "feat: add Tauri remediation commands"
```

---

### Task 8: Integration test — round-trip detect → decide → execute

**Files:**
- Create: `tests/remediation_test.rs`

**Step 1: Write the integration test**

```rust
//! Integration test for the filter remediation pipeline.
//! Uses a mock client to verify the full detect → decide → execute flow.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use gmail_automation::client::{ExistingFilterInfo, GmailClient, LabelInfo};
use gmail_automation::error::Result;
use gmail_automation::filter_remediation::{
    GroupDecision, OverlapDetector, RemediationPlan,
};
use gmail_automation::models::{FilterRule, MessageMetadata};
use gmail_automation::scanner::QuotaStats;

/// A mock client that tracks created/deleted filters
struct MockRemediationClient {
    initial_filters: Vec<ExistingFilterInfo>,
    deleted: Arc<Mutex<Vec<String>>>,
    created: Arc<Mutex<Vec<FilterRule>>>,
}

// Implement GmailClient trait for MockRemediationClient.
// Only list_filters, delete_filter, create_filter need real implementations.
// (Full trait impl with stubs for unused methods required by the compiler.)

#[tokio::test]
async fn test_full_remediation_roundtrip() {
    let filters = vec![
        ExistingFilterInfo {
            id: "f1".into(),
            query: None,
            from: Some("cba.com.au".into()),
            to: None,
            subject: Some("statement".into()),
            add_label_ids: vec!["lbl_fin".into()],
            remove_label_ids: vec![],
        },
        ExistingFilterInfo {
            id: "f2".into(),
            query: None,
            from: Some("cba.com.au".into()),
            to: None,
            subject: None,
            add_label_ids: vec!["lbl_oth".into()],
            remove_label_ids: vec![],
        },
    ];

    let deleted = Arc::new(Mutex::new(vec![]));
    let created = Arc::new(Mutex::new(vec![]));

    let client = MockRemediationClient {
        initial_filters: filters,
        deleted: deleted.clone(),
        created: created.clone(),
    };

    let label_map: HashMap<String, String> = [
        ("lbl_fin".into(), "Financial".into()),
        ("lbl_oth".into(), "Other".into()),
    ]
    .into_iter()
    .collect();

    // Detect
    let groups = OverlapDetector::detect(&client, &label_map).await.unwrap();
    assert_eq!(groups.len(), 1);

    // Decide: accept the mechanical fix
    let mut plan = RemediationPlan::new();
    for group in groups {
        if let gmail_automation::ResolutionType::MechanicalFix {
            ref proposed_replacements,
        } = group.resolution_type
        {
            plan.add(
                group.clone(),
                GroupDecision::ReplaceWithExclusive {
                    replacement_filters: proposed_replacements.clone(),
                },
            );
        }
    }

    // Execute
    let result = plan.execute(&client).await.unwrap();
    assert_eq!(result.deleted.len(), 2);
    assert!(result.created.len() >= 1);
    assert!(result.errors.is_empty());

    // Verify the remainder replacement has exclusions
    let created_filters = created.lock().unwrap();
    let remainder = created_filters
        .iter()
        .find(|f| f.subject_keywords.is_empty())
        .expect("Should have a remainder filter");
    assert!(
        remainder.excluded_subject_patterns.contains(&"statement".to_string()),
        "Remainder should exclude 'statement'"
    );
}
```

Note: The full `GmailClient` trait impl for `MockRemediationClient` will need stubs for all methods. Use `unimplemented!()` for unused methods.

**Step 2: Run test to verify it passes**

Run: `cargo test --test remediation_test -- --nocapture`
Expected: PASS

**Step 3: Commit**

```bash
git add tests/remediation_test.rs
git commit -m "test: add integration test for remediation round-trip"
```

---

### Task 9: Final verification

**Step 1: Run full test suite**

Run: `cargo test`
Expected: All tests pass

**Step 2: Run clippy**

Run: `cargo clippy -- -D warnings`
Expected: No warnings

**Step 3: Verify CLI help shows new command**

Run: `cargo run -- --help`
Expected: Output includes `remediate` subcommand

**Step 4: Commit any fixups**

If clippy or tests surfaced issues, fix and commit.
