# Fix Overlap Grouping and Decision Counter Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace domain-based overlap grouping with union-find grouping based on actual pairwise overlap analysis, and fix the decision counter reactivity bug.

**Architecture:** Add `filter_to_expr()` converter to build `FilterExpr` from `ExistingFilterInfo`. Replace `group_filters()` with union-find connected components using `FilterOverlapAnalyzer::analyze_expr_relation()`. Fix SolidJS counter by replacing `createMemo` with plain functions.

**Tech Stack:** Rust (proptest for property-based testing), SolidJS/TypeScript

---

### Task 1: Add `filter_to_expr` converter

**Files:**
- Modify: `src/filter_remediation.rs` (add method to `OverlapDetector` impl block, around line 471)

**Step 1: Write failing test**

Add to the test module in `src/filter_remediation.rs`, after the existing `make_filter_with_query` helper:

```rust
#[test]
fn test_filter_to_expr_from_field() {
    let filter = make_filter("f1", Some("noreply@example.com"), None, "lbl_fin");
    let expr = OverlapDetector::filter_to_expr(&filter);
    assert!(expr.from_clause.is_some(), "from_clause should be set from filter.from");
    assert!(expr.subject_clause.is_none());
    assert!(expr.exclusions.is_empty());
}

#[test]
fn test_filter_to_expr_query_with_exclusions() {
    let filter = make_filter_with_query(
        "f1",
        None,
        Some("from:(*@amazon.com.au) -from:(orders@amazon.com.au) -from:(shipping@amazon.com.au)"),
        None,
        "lbl_rec",
    );
    let expr = OverlapDetector::filter_to_expr(&filter);
    assert!(expr.from_clause.is_some(), "from_clause should be set from query");
    assert_eq!(expr.exclusions.len(), 2, "should have 2 FROM exclusions");
}

#[test]
fn test_filter_to_expr_subject_field() {
    let filter = make_filter("f1", Some("noreply@example.com"), Some("Order Confirmation"), "lbl_rec");
    let expr = OverlapDetector::filter_to_expr(&filter);
    assert!(expr.from_clause.is_some());
    assert!(expr.subject_clause.is_some(), "subject from filter.subject field");
}

#[test]
fn test_filter_to_expr_prefers_query_over_from_field() {
    // When query is present, use it (it has richer info like exclusions)
    let filter = make_filter_with_query(
        "f1",
        Some("amazon.com.au"),
        Some("from:(*@amazon.com.au) -from:(orders@amazon.com.au)"),
        None,
        "lbl_rec",
    );
    let expr = OverlapDetector::filter_to_expr(&filter);
    assert!(expr.from_clause.is_some());
    assert_eq!(expr.exclusions.len(), 1, "should parse exclusion from query");
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p gmail-automation test_filter_to_expr -- --nocapture 2>&1 | tail -10`
Expected: FAIL — method doesn't exist

**Step 3: Implement `filter_to_expr`**

Add to the `impl OverlapDetector` block (after `parse_from_key`, around line 471):

```rust
/// Convert an ExistingFilterInfo to a FilterExpr for overlap analysis.
///
/// Prefers the `query` field (richer — includes exclusions, subject clauses).
/// Falls back to `from` and `subject` fields if no query.
pub fn filter_to_expr(filter: &ExistingFilterInfo) -> FilterExpr {
    // If query exists, parse it — it has the richest information
    if let Some(ref query) = filter.query {
        let mut expr = parse_gmail_query(query);

        // If parse_gmail_query didn't find a from_clause but filter.from exists,
        // supplement from the from field
        if expr.from_clause.is_none() {
            if let Some(ref from) = filter.from {
                expr.from_clause = Self::parse_from_field(from);
            }
        }

        // If no subject_clause from query but filter.subject exists, use it
        if expr.subject_clause.is_none() {
            if let Some(ref subj) = filter.subject {
                if !subj.is_empty() {
                    expr.subject_clause = Some(SubjectClause::any_of(
                        vec![subj.clone()],
                    ));
                }
            }
        }

        return expr;
    }

    // No query — build from individual fields
    let mut expr = FilterExpr::new();

    if let Some(ref from) = filter.from {
        expr.from_clause = Self::parse_from_field(from);
    }

    if let Some(ref subj) = filter.subject {
        if !subj.is_empty() {
            expr.subject_clause = Some(SubjectClause::any_of(
                vec![subj.clone()],
            ));
        }
    }

    expr
}

/// Parse a raw `from` field value into a FromClause.
fn parse_from_field(from: &str) -> Option<FromClause> {
    let from_normalized = from.trim().to_lowercase();
    if from_normalized.is_empty() {
        return None;
    }
    if from_normalized.contains('@') {
        // Specific sender
        EmailPattern::parse(&from_normalized)
            .map(FromClause::SpecificSender)
    } else {
        // Domain
        Some(FromClause::Domain(DomainPattern {
            domain: from_normalized,
            include_subdomains: false,
        }))
    }
}
```

Note: You'll need to import `SubjectClause`, `DomainPattern`, `EmailPattern` in the impl block or at the top of the file. Check what's already imported — `parse_gmail_query` uses these types so they should be available.

**Step 4: Run tests to verify they pass**

Run: `cargo test -p gmail-automation test_filter_to_expr -- --nocapture 2>&1 | tail -10`
Expected: All 4 pass

**Step 5: Commit**

```bash
git add src/filter_remediation.rs
git commit -m "feat: add filter_to_expr converter for ExistingFilterInfo"
```

---

### Task 2: Replace domain-based grouping with union-find overlap grouping

**Files:**
- Modify: `src/filter_remediation.rs:676-715` — rewrite `group_filters()`

**Step 1: Write failing tests for the new grouping behavior**

Add to the test module:

```rust
#[test]
fn test_group_excludes_disjoint_catchall_with_exclusions() {
    // Catch-all with exclusions should NOT be grouped with the excluded senders
    let filters = vec![
        make_filter_with_query(
            "f1", None,
            Some("from:(*@amazon.com.au) -from:(orders@amazon.com.au) -from:(shipping@amazon.com.au)"),
            None, "lbl_rec",
        ),
        make_filter("f2", Some("orders@amazon.com.au"), None, "lbl_rec"),
        make_filter("f3", Some("shipping@amazon.com.au"), None, "lbl_rec"),
    ];
    let groups = OverlapDetector::group_filters(&filters, &label_map());
    // f1 excludes f2 and f3 — all are disjoint, no groups should form
    assert_eq!(groups.len(), 0, "Disjoint filters should not be grouped: {:?}", groups);
}

#[test]
fn test_group_keeps_genuine_overlaps() {
    // Domain filter and specific sender within that domain DO overlap
    let filters = vec![
        make_filter("f1", Some("amazon.com.au"), None, "lbl_rec"),
        make_filter("f2", Some("orders@amazon.com.au"), None, "lbl_fin"),
    ];
    let groups = OverlapDetector::group_filters(&filters, &label_map());
    assert_eq!(groups.len(), 1, "Genuinely overlapping filters should be grouped");
    assert_eq!(groups[0].filters.len(), 2);
}

#[test]
fn test_group_transitive_overlap() {
    // A overlaps B, B overlaps C, but A and C may be disjoint
    // They should still be in the same group (connected component)
    let filters = vec![
        make_filter("f1", Some("amazon.com.au"), None, "lbl_rec"),  // domain catch-all
        make_filter("f2", Some("orders@amazon.com.au"), None, "lbl_fin"),  // specific, subsumed by f1
        make_filter("f3", Some("shipping@amazon.com.au"), None, "lbl_oth"),  // specific, subsumed by f1
    ];
    let groups = OverlapDetector::group_filters(&filters, &label_map());
    assert_eq!(groups.len(), 1, "Transitively connected filters should be in one group");
    assert_eq!(groups[0].filters.len(), 3);
}

#[test]
fn test_group_mixed_overlap_and_disjoint() {
    // 4 filters on amazon.com.au:
    // f1: catch-all excluding f2, f3 (disjoint with f2, f3)
    // f2: orders@ (specific)
    // f3: shipping@ (specific)
    // f4: domain catch-all without exclusions (overlaps with f1, f2, f3)
    let filters = vec![
        make_filter_with_query(
            "f1", None,
            Some("from:(*@amazon.com.au) -from:(orders@amazon.com.au) -from:(shipping@amazon.com.au)"),
            None, "lbl_rec",
        ),
        make_filter("f2", Some("orders@amazon.com.au"), None, "lbl_fin"),
        make_filter("f3", Some("shipping@amazon.com.au"), None, "lbl_oth"),
        make_filter("f4", Some("amazon.com.au"), None, "lbl_per"),
    ];
    let groups = OverlapDetector::group_filters(&filters, &label_map());
    // f4 overlaps with f1, f2, f3 (it's a broad domain catch-all with no exclusions)
    // So all 4 end up in one group via transitivity through f4
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].filters.len(), 4);
}

#[test]
fn test_group_subject_exclusion_disjoint() {
    // subject:(Y) vs -subject:(Y) with same FROM — disjoint
    let filters = vec![
        make_filter_with_query(
            "f1", Some("noreply@example.com"),
            Some("from:(noreply@example.com) subject:(Notification)"),
            None, "lbl_rec",
        ),
        make_filter_with_query(
            "f2", Some("noreply@example.com"),
            Some("from:(noreply@example.com) -subject:(Notification)"),
            None, "lbl_oth",
        ),
    ];
    let groups = OverlapDetector::group_filters(&filters, &label_map());
    assert_eq!(groups.len(), 0, "Subject-excluded filters should not be grouped");
}

#[test]
fn test_group_different_domains_never_grouped() {
    let filters = vec![
        make_filter("f1", Some("github.com"), None, "lbl_rec"),
        make_filter("f2", Some("gitlab.com"), None, "lbl_rec"),
    ];
    let groups = OverlapDetector::group_filters(&filters, &label_map());
    assert_eq!(groups.len(), 0, "Different domains should never be grouped");
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p gmail-automation test_group_excludes_disjoint -- --nocapture 2>&1 | tail -10`
Expected: FAIL — the current domain-based grouping will group them

**Step 3: Rewrite `group_filters` with union-find**

Replace `group_filters` (lines 676-715) entirely:

```rust
/// Group filters by actual overlap analysis using union-find.
///
/// Instead of grouping by domain string, we parse each filter into a FilterExpr
/// and run pairwise overlap analysis. Filters that are not Disjoint are connected.
/// Connected components become overlap groups. Singletons are discarded.
pub fn group_filters(
    filters: &[ExistingFilterInfo],
    label_map: &HashMap<String, String>,
) -> Vec<OverlapGroup> {
    let n = filters.len();
    if n < 2 {
        return vec![];
    }

    // Parse all filters into FilterExpr
    let exprs: Vec<FilterExpr> = filters.iter().map(Self::filter_to_expr).collect();

    // Union-Find
    let mut parent: Vec<usize> = (0..n).collect();
    let mut rank: Vec<usize> = vec![0; n];

    fn find(parent: &mut [usize], i: usize) -> usize {
        if parent[i] != i {
            parent[i] = find(parent, parent[i]); // path compression
        }
        parent[i]
    }

    fn union(parent: &mut [usize], rank: &mut [usize], a: usize, b: usize) {
        let ra = find(parent, a);
        let rb = find(parent, b);
        if ra == rb {
            return;
        }
        if rank[ra] < rank[rb] {
            parent[ra] = rb;
        } else if rank[ra] > rank[rb] {
            parent[rb] = ra;
        } else {
            parent[rb] = ra;
            rank[ra] += 1;
        }
    }

    // Pairwise overlap analysis
    let analyzer = FilterOverlapAnalyzer::new();
    for i in 0..n {
        for j in (i + 1)..n {
            let relation = analyzer.analyze_expr_relation(&exprs[i], &exprs[j]);
            if !matches!(relation, PatternRelation::Disjoint) {
                union(&mut parent, &mut rank, i, j);
            }
        }
    }

    // Collect connected components
    let mut components: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..n {
        let root = find(&mut parent, i);
        components.entry(root).or_default().push(i);
    }

    // Build OverlapGroups from components with 2+ members
    let mut groups: Vec<OverlapGroup> = components
        .into_values()
        .filter(|members| members.len() > 1)
        .map(|members| {
            let group_filters: Vec<ExistingFilterInfo> =
                members.iter().map(|&i| filters[i].clone()).collect();

            // Derive group_id from the dominant domain
            let domain = group_filters
                .iter()
                .find_map(|f| Self::parse_from_key(f).map(|(d, _)| d))
                .unwrap_or_else(|| "unknown".to_string());

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

    groups.sort_by(|a, b| a.group_id.cmp(&b.group_id));
    groups
}
```

You'll need to add `use crate::filter_overlap::{FilterOverlapAnalyzer, PatternRelation};` at the top of the file if not already imported.

**Step 4: Run ALL grouping tests**

Run: `cargo test -p gmail-automation test_group_ -- --nocapture 2>&1 | tail -20`
Expected: All pass. Note: `test_group_same_domain_filters` (line 763) and `test_group_domain_subsumes_specific_sender` (line 776) should still pass — these are genuine overlaps. `test_no_groups_for_non_overlapping` (line 787) should still pass.

**Step 5: Run full test suite for regressions**

Run: `cargo test -p gmail-automation 2>&1 | tail -10`
Expected: All tests pass. Some existing proptest properties may need adjustment if they assumed domain-based grouping. Check and fix if needed.

**Step 6: Commit**

```bash
git add src/filter_remediation.rs
git commit -m "fix: replace domain-based grouping with union-find overlap analysis"
```

---

### Task 3: Fixture-based tests for tricky real-world scenarios

**Files:**
- Modify: `src/filter_remediation.rs` (test module)

**Step 1: Add fixture tests**

These are hand-crafted scenarios that have historically caused false positives or negatives. Add to test module:

```rust
/// Real-world fixture: the amazon.com.au pattern from the bug report.
/// Catch-all with 3 exclusions + the 3 excluded specific senders + different labels.
#[test]
fn test_fixture_amazon_catchall_with_exclusions() {
    let filters = vec![
        // Catch-all: everything from amazon.com.au EXCEPT three specific senders
        make_filter_with_query(
            "f1", None,
            Some("from:(*@amazon.com.au) -from:(order-update@amazon.com.au) -from:(auto-confirm@amazon.com.au) -from:(shipment-tracking@amazon.com.au)"),
            None, "lbl_rec",
        ),
        make_filter("f2", Some("auto-confirm@amazon.com.au"), None, "lbl_rec"),
        make_filter("f3", Some("order-update@amazon.com.au"), None, "lbl_rec"),
        make_filter("f4", Some("shipment-tracking@amazon.com.au"), None, "lbl_oth"),
    ];
    let groups = OverlapDetector::group_filters(&filters, &label_map());
    // f1 is disjoint with f2, f3, f4 (it excludes all three)
    // f2, f3, f4 are pairwise disjoint (different specific senders)
    // No overlapping pair exists -> no groups
    assert_eq!(groups.len(), 0,
        "Amazon catch-all with exclusions: no genuine overlaps. Got: {:?}",
        groups.iter().map(|g| (&g.group_id, g.filters.len())).collect::<Vec<_>>()
    );
}

/// Fixture: two filters on same sender, different subjects — mutually exclusive via -subject:
#[test]
fn test_fixture_woolworths_subject_exclusion() {
    let filters = vec![
        make_filter_with_query(
            "f1", Some("noreply@online.woolworths.com.au"),
            Some("from:(noreply@online.woolworths.com.au) subject:(Woolworths Online Notification)"),
            None, "lbl_rec",
        ),
        make_filter_with_query(
            "f2", Some("noreply@online.woolworths.com.au"),
            Some("from:(noreply@online.woolworths.com.au) -subject:(Woolworths Online Notification)"),
            None, "lbl_oth",
        ),
    ];
    let groups = OverlapDetector::group_filters(&filters, &label_map());
    assert_eq!(groups.len(), 0,
        "Woolworths subject/!subject pair should be disjoint, not grouped");
}

/// Fixture: overlapping domain + specific sender with SAME label (consolidation candidate)
#[test]
fn test_fixture_genuine_consolidation_candidate() {
    let filters = vec![
        make_filter("f1", Some("cba.com.au"), None, "lbl_fin"),
        make_filter("f2", Some("noreply@cba.com.au"), None, "lbl_fin"),
        make_filter("f3", Some("alerts@cba.com.au"), None, "lbl_fin"),
    ];
    let groups = OverlapDetector::group_filters(&filters, &label_map());
    assert_eq!(groups.len(), 1, "Genuine overlaps should still be grouped");
    assert_eq!(groups[0].filters.len(), 3);
}

/// Fixture: two identical from:, different labels — genuine PickWinner
#[test]
fn test_fixture_same_from_different_labels() {
    let filters = vec![
        make_filter("f1", Some("noreply@example.com"), None, "lbl_fin"),
        make_filter("f2", Some("noreply@example.com"), None, "lbl_rec"),
    ];
    let groups = OverlapDetector::group_filters(&filters, &label_map());
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].filters.len(), 2);
}

/// Fixture: partial -from: exclusion (excludes SOME but not all specific senders)
#[test]
fn test_fixture_partial_exclusion_still_overlaps() {
    let filters = vec![
        // Catch-all excludes only orders@, NOT shipping@
        make_filter_with_query(
            "f1", None,
            Some("from:(*@amazon.com.au) -from:(orders@amazon.com.au)"),
            None, "lbl_rec",
        ),
        make_filter("f2", Some("orders@amazon.com.au"), None, "lbl_fin"),
        make_filter("f3", Some("shipping@amazon.com.au"), None, "lbl_oth"),
    ];
    let groups = OverlapDetector::group_filters(&filters, &label_map());
    // f1 is disjoint with f2 (excluded), but OVERLAPS with f3 (not excluded)
    // f2 is disjoint with f1 and f3 (different senders)
    // So: f1 and f3 form a group. f2 is isolated.
    assert_eq!(groups.len(), 1, "f1 and f3 should be grouped (f1 catches shipping@)");
    assert_eq!(groups[0].filters.len(), 2);
    let ids: Vec<&str> = groups[0].filters.iter().map(|f| f.id.as_str()).collect();
    assert!(ids.contains(&"f1") && ids.contains(&"f3"),
        "Group should contain f1 and f3, got: {:?}", ids);
}

/// Fixture: chain of overlaps A<->B<->C where A and C are disjoint
#[test]
fn test_fixture_transitive_chain() {
    // A: *@big-corp.com (domain)
    // B: noreply@big-corp.com (specific, subsumed by A)
    // C: alerts@big-corp.com (specific, subsumed by A)
    // A overlaps B, A overlaps C, B and C are disjoint
    // All three should be in one group (connected component via A)
    let filters = vec![
        make_filter("fA", Some("big-corp.com"), None, "lbl_fin"),
        make_filter("fB", Some("noreply@big-corp.com"), None, "lbl_rec"),
        make_filter("fC", Some("alerts@big-corp.com"), None, "lbl_oth"),
    ];
    let groups = OverlapDetector::group_filters(&filters, &label_map());
    assert_eq!(groups.len(), 1, "Transitive chain should produce one group");
    assert_eq!(groups[0].filters.len(), 3);
}

/// Fixture: completely independent domains — should never group
#[test]
fn test_fixture_independent_domains() {
    let filters = vec![
        make_filter("f1", Some("github.com"), None, "lbl_rec"),
        make_filter("f2", Some("noreply@github.com"), None, "lbl_fin"),
        make_filter("f3", Some("gitlab.com"), None, "lbl_rec"),
        make_filter("f4", Some("alerts@gitlab.com"), None, "lbl_oth"),
    ];
    let groups = OverlapDetector::group_filters(&filters, &label_map());
    // github.com group: f1 + f2 (domain subsumes specific)
    // gitlab.com group: f3 + f4 (domain subsumes specific)
    assert_eq!(groups.len(), 2, "Two separate domains = two groups");
}
```

**Step 2: Run fixture tests**

Run: `cargo test -p gmail-automation test_fixture_ -- --nocapture 2>&1 | tail -20`
Expected: All pass

**Step 3: Commit**

```bash
git add src/filter_remediation.rs
git commit -m "test: add fixture tests for real-world overlap grouping scenarios"
```

---

### Task 4: Property-based tests for grouping invariants

**Files:**
- Modify: `src/filter_remediation.rs` (proptest section, after existing P8)

**Step 1: Expand the proptest strategy to include exclusions**

The existing `filter_group_strategy` only generates positive `from:` values. We need a richer strategy. Add alongside the existing strategies:

```rust
/// Strategy for generating a diverse set of filters across multiple domains,
/// including catch-all + exclusion patterns, subject exclusions, etc.
fn diverse_filter_set_strategy() -> impl Strategy<Value = Vec<ExistingFilterInfo>> {
    // Generate 3-8 filters with varied patterns
    proptest::collection::vec(
        (
            // Domain choice
            prop_oneof![
                Just("amazon.com.au".to_string()),
                Just("cba.com.au".to_string()),
                Just("github.com".to_string()),
            ],
            // Filter type
            prop_oneof![
                Just("domain"),       // domain catch-all
                Just("specific"),     // specific sender
                Just("catchall_ex"),  // catch-all with -from: exclusions
                Just("subject_pos"), // specific sender with subject:(X)
                Just("subject_neg"), // specific sender with -subject:(X)
            ],
            // Label
            label_strategy(),
        ),
        3..=8,
    )
    .prop_map(|configs| {
        configs
            .iter()
            .enumerate()
            .map(|(i, (domain, filter_type, label))| {
                match filter_type.as_str() {
                    "domain" => make_filter_with_query(
                        &format!("f{}", i),
                        Some(domain),
                        Some(&format!("from:(*@{})", domain)),
                        None,
                        label,
                    ),
                    "specific" => {
                        let sender = format!("noreply@{}", domain);
                        make_filter_with_query(
                            &format!("f{}", i),
                            Some(&sender),
                            Some(&format!("from:({})", sender)),
                            None,
                            label,
                        )
                    }
                    "catchall_ex" => make_filter_with_query(
                        &format!("f{}", i),
                        None,
                        Some(&format!(
                            "from:(*@{}) -from:(noreply@{})",
                            domain, domain
                        )),
                        None,
                        label,
                    ),
                    "subject_pos" => {
                        let sender = format!("noreply@{}", domain);
                        make_filter_with_query(
                            &format!("f{}", i),
                            Some(&sender),
                            Some(&format!("from:({}) subject:(receipt)", sender)),
                            None,
                            label,
                        )
                    }
                    "subject_neg" => {
                        let sender = format!("noreply@{}", domain);
                        make_filter_with_query(
                            &format!("f{}", i),
                            Some(&sender),
                            Some(&format!("from:({}) -subject:(receipt)", sender)),
                            None,
                            label,
                        )
                    }
                    _ => unreachable!(),
                }
            })
            .collect()
    })
}
```

Note: `make_filter_with_query` is already defined in the test module. The `label_strategy` function is also already defined.

**Step 2: Add property-based tests**

Add inside the existing `proptest!` block:

```rust
// P9: No group contains a pair of disjoint filters as its ONLY members
// (If a group has only 2 filters, they must not be disjoint)
#[test]
fn prop_no_two_filter_group_is_disjoint(filters in diverse_filter_set_strategy()) {
    let label_map = test_label_map();
    let groups = OverlapDetector::group_filters(&filters, &label_map);
    let analyzer = FilterOverlapAnalyzer::new();

    for group in &groups {
        if group.filters.len() == 2 {
            let expr_a = OverlapDetector::filter_to_expr(&group.filters[0]);
            let expr_b = OverlapDetector::filter_to_expr(&group.filters[1]);
            let relation = analyzer.analyze_expr_relation(&expr_a, &expr_b);
            prop_assert!(
                !matches!(relation, PatternRelation::Disjoint),
                "Two-filter group '{}' contains disjoint filters: {:?} vs {:?} = {:?}",
                group.group_id,
                group.filters[0].id, group.filters[1].id, relation,
            );
        }
    }
}

// P10: Every filter in a group is non-disjoint with at least one other member
#[test]
fn prop_every_member_overlaps_something(filters in diverse_filter_set_strategy()) {
    let label_map = test_label_map();
    let groups = OverlapDetector::group_filters(&filters, &label_map);
    let analyzer = FilterOverlapAnalyzer::new();

    for group in &groups {
        let exprs: Vec<FilterExpr> = group.filters.iter()
            .map(OverlapDetector::filter_to_expr)
            .collect();

        for (i, _) in group.filters.iter().enumerate() {
            let has_overlap = (0..group.filters.len())
                .filter(|&j| j != i)
                .any(|j| {
                    !matches!(
                        analyzer.analyze_expr_relation(&exprs[i], &exprs[j]),
                        PatternRelation::Disjoint
                    )
                });
            prop_assert!(
                has_overlap,
                "Filter '{}' in group '{}' is disjoint with all other members",
                group.filters[i].id, group.group_id,
            );
        }
    }
}

// P11: Groups form valid connected components (if A and B are in the same group,
// there exists a path of non-disjoint pairs connecting them)
#[test]
fn prop_groups_are_connected_components(filters in diverse_filter_set_strategy()) {
    let label_map = test_label_map();
    let groups = OverlapDetector::group_filters(&filters, &label_map);
    let analyzer = FilterOverlapAnalyzer::new();

    for group in &groups {
        let n = group.filters.len();
        let exprs: Vec<FilterExpr> = group.filters.iter()
            .map(OverlapDetector::filter_to_expr)
            .collect();

        // Build adjacency and check connectivity via BFS
        let mut adj = vec![vec![]; n];
        for i in 0..n {
            for j in (i+1)..n {
                if !matches!(analyzer.analyze_expr_relation(&exprs[i], &exprs[j]), PatternRelation::Disjoint) {
                    adj[i].push(j);
                    adj[j].push(i);
                }
            }
        }

        // BFS from node 0
        let mut visited = vec![false; n];
        let mut queue = std::collections::VecDeque::new();
        visited[0] = true;
        queue.push_back(0);
        while let Some(node) = queue.pop_front() {
            for &neighbor in &adj[node] {
                if !visited[neighbor] {
                    visited[neighbor] = true;
                    queue.push_back(neighbor);
                }
            }
        }

        prop_assert!(
            visited.iter().all(|&v| v),
            "Group '{}' is not a connected component — some filters unreachable from filter 0",
            group.group_id,
        );
    }
}

// P12: No two groups share a filter
#[test]
fn prop_no_filter_in_multiple_groups(filters in diverse_filter_set_strategy()) {
    let label_map = test_label_map();
    let groups = OverlapDetector::group_filters(&filters, &label_map);

    let mut seen_ids = std::collections::HashSet::new();
    for group in &groups {
        for f in &group.filters {
            prop_assert!(
                seen_ids.insert(f.id.clone()),
                "Filter '{}' appears in multiple groups",
                f.id,
            );
        }
    }
}

// P13: Grouping is deterministic (same input -> same output)
#[test]
fn prop_grouping_deterministic(filters in diverse_filter_set_strategy()) {
    let label_map = test_label_map();
    let groups1 = OverlapDetector::group_filters(&filters, &label_map);
    let groups2 = OverlapDetector::group_filters(&filters, &label_map);

    prop_assert_eq!(groups1.len(), groups2.len(), "Different number of groups on same input");
    for (g1, g2) in groups1.iter().zip(groups2.iter()) {
        let ids1: Vec<&str> = g1.filters.iter().map(|f| f.id.as_str()).collect();
        let ids2: Vec<&str> = g2.filters.iter().map(|f| f.id.as_str()).collect();
        prop_assert_eq!(ids1, ids2, "Groups differ on same input");
    }
}
```

**Step 2: Run proptest suite**

Run: `cargo test -p gmail-automation prop_ -- --nocapture 2>&1 | tail -20`
Expected: All properties hold (P1-P13)

**Step 3: Commit**

```bash
git add src/filter_remediation.rs
git commit -m "test: add property-based tests for union-find overlap grouping"
```

---

### Task 5: Fix decision counter reactivity

**Files:**
- Modify: `ui/src/stores/app.ts:208-213`

**Step 1: Replace createMemo with plain functions**

In `ui/src/stores/app.ts`, change lines 208-213 from:

```typescript
  decidedCount: createMemo(() => Object.keys(remediationDecisions()).length),
  allDecided: createMemo(() => {
    const groups = remediationGroups();
    const decisions = remediationDecisions();
    return groups.length > 0 && groups.every(g => g.group_id in decisions);
  }),
```

to:

```typescript
  decidedCount: () => Object.keys(remediationDecisions()).length,
  allDecided: () => {
    const groups = remediationGroups();
    const decisions = remediationDecisions();
    return groups.length > 0 && groups.every(g => g.group_id in decisions);
  },
```

**Step 2: Remove unused createMemo import if no longer needed**

Check if `createMemo` is used elsewhere in `app.ts`. The `review` store has `undecidedCount: createMemo(...)` at line 90, so the import stays.

**Step 3: Verify it compiles**

Run: `cd ui && npx tsc --noEmit 2>&1 | tail -10`
Expected: No errors (or check the project's existing type-check command)

**Step 4: Commit**

```bash
git add ui/src/stores/app.ts
git commit -m "fix: replace createMemo with plain functions for decision counter reactivity"
```

---

### Task 6: Run full test suite and verify

**Step 1: Run Rust tests**

Run: `cargo test 2>&1 | tail -20`
Expected: All tests pass

**Step 2: Build Tauri app**

Run: `cd src-tauri && cargo build 2>&1 | tail -10`
Expected: Compiles

**Step 3: Commit any fixes**

If anything broke, fix and commit.
