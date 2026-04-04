# Fix Remediation Filter Bugs — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Fix 4 bugs in the remediation pipeline that cause mass mis-labeling of emails, and add proptest property-based tests to prevent regressions.

**Architecture:** All changes are in `src/filter_remediation.rs`. Add `extract_from_pattern` and `reconstruct_filter_query` helpers. Change `LabelSwap` to carry a full Gmail query instead of just a domain. Refactor `collect_swaps` to produce per-loser-filter swaps. Add defensive subject extraction fallback. Add proptest-based property tests. Update Tauri callers and existing tests that reference `LabelSwap.from_pattern`.

**Tech Stack:** Rust, proptest (already a dev-dependency), async-trait, serde

---

### Task 1: Add `extract_from_pattern` helper

This helper extracts a `FilterRule`-compatible from-pattern string from an `ExistingFilterInfo`, checking both the `from` field and the `query` field. This is the fix for Bug 1.

**Files:**
- Modify: `src/filter_remediation.rs:419-453` (add helper near `parse_from_key`)

**Step 1: Write the failing test**

In `src/filter_remediation.rs`, inside `mod tests`, add after the existing `make_filter` helper:

```rust
    fn make_filter_with_query(
        id: &str,
        from: Option<&str>,
        query: Option<&str>,
        subject: Option<&str>,
        label: &str,
    ) -> ExistingFilterInfo {
        ExistingFilterInfo {
            id: id.to_string(),
            query: query.map(|s| s.to_string()),
            from: from.map(|s| s.to_string()),
            to: None,
            subject: subject.map(|s| s.to_string()),
            add_label_ids: vec![label.to_string()],
            remove_label_ids: vec![],
        }
    }

    #[test]
    fn test_extract_from_pattern_from_field() {
        let filter = make_filter("f1", Some("noreply@cba.com.au"), None, "lbl_fin");
        let (from_pattern, is_specific) = OverlapDetector::extract_from_pattern(&filter);
        assert_eq!(from_pattern, Some("noreply@cba.com.au".to_string()));
        assert!(is_specific);
    }

    #[test]
    fn test_extract_from_pattern_domain_field() {
        let filter = make_filter("f1", Some("cba.com.au"), None, "lbl_fin");
        let (from_pattern, is_specific) = OverlapDetector::extract_from_pattern(&filter);
        assert_eq!(from_pattern, Some("cba.com.au".to_string()));
        assert!(!is_specific);
    }

    #[test]
    fn test_extract_from_pattern_from_query_field() {
        let filter = make_filter_with_query(
            "f1",
            None,
            Some("from:(*@cba.com.au) subject:(statement)"),
            None,
            "lbl_fin",
        );
        let (from_pattern, is_specific) = OverlapDetector::extract_from_pattern(&filter);
        assert!(from_pattern.is_some(), "Should extract from_pattern from query field");
        let fp = from_pattern.unwrap();
        assert!(fp.contains("cba.com.au"), "from_pattern should contain domain: {}", fp);
        assert!(!is_specific);
    }

    #[test]
    fn test_extract_from_pattern_specific_sender_in_query() {
        let filter = make_filter_with_query(
            "f1",
            None,
            Some("from:(noreply@cba.com.au)"),
            None,
            "lbl_fin",
        );
        let (from_pattern, is_specific) = OverlapDetector::extract_from_pattern(&filter);
        assert!(from_pattern.is_some());
        let fp = from_pattern.unwrap();
        assert!(fp.contains("noreply@cba.com.au"), "Should extract specific sender: {}", fp);
        assert!(is_specific);
    }

    #[test]
    fn test_extract_from_pattern_none_when_no_from() {
        let filter = make_filter_with_query("f1", None, Some("subject:(hello)"), None, "lbl_fin");
        let (from_pattern, _is_specific) = OverlapDetector::extract_from_pattern(&filter);
        assert!(from_pattern.is_none());
    }
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --lib filter_remediation::tests::test_extract_from_pattern -- 2>&1 | tail -5`
Expected: FAIL — `extract_from_pattern` method doesn't exist

**Step 3: Implement the helper**

In `src/filter_remediation.rs`, add this method to the `impl OverlapDetector` block, after `parse_from_key` (after line 453):

```rust
    /// Extract a FilterRule-compatible from_pattern from an ExistingFilterInfo.
    /// Checks `filter.from` first, falls back to parsing `filter.query`.
    /// Returns (from_pattern, is_specific_sender).
    pub fn extract_from_pattern(filter: &ExistingFilterInfo) -> (Option<String>, bool) {
        // Try filter.from first (direct Gmail API field)
        if let Some(ref from) = filter.from {
            if !from.trim().is_empty() {
                let is_specific = from.contains('@');
                return (Some(from.clone()), is_specific);
            }
        }
        // Fall back to parsing the query field
        if let Some(ref query) = filter.query {
            let expr = parse_gmail_query(query);
            if let Some(ref from_clause) = expr.from_clause {
                return match from_clause {
                    FromClause::Domain(dp) => {
                        (Some(format!("*@{}", dp.domain)), false)
                    }
                    FromClause::SpecificSender(ep) => {
                        (Some(ep.full_address()), true)
                    }
                };
            }
        }
        (None, false)
    }
```

**Step 4: Run tests to verify they pass**

Run: `cargo test --lib filter_remediation::tests::test_extract_from_pattern`
Expected: ALL PASS (5 tests)

**Step 5: Commit**

```bash
git add src/filter_remediation.rs
git commit -m "feat: add extract_from_pattern helper for ExistingFilterInfo"
```

---

### Task 2: Fix `classify_groups` to use `extract_from_pattern`

Replace the buggy `filter.from.clone()` on line 548 with the new helper.

**Files:**
- Modify: `src/filter_remediation.rs:548-552`

**Step 1: Write the failing test**

In `src/filter_remediation.rs` `mod tests`, add:

```rust
    #[test]
    fn test_classify_mechanical_fix_from_in_query_field() {
        // Filters where from: is stored in query field, not from field
        let filters = vec![
            make_filter_with_query(
                "f1",
                None,
                Some("from:(*@cba.com.au) subject:(statement)"),
                Some("statement"),
                "lbl_fin",
            ),
            make_filter_with_query(
                "f2",
                None,
                Some("from:(*@cba.com.au)"),
                None,
                "lbl_oth",
            ),
        ];
        let mut groups = OverlapDetector::group_filters(&filters, &label_map());
        OverlapDetector::classify_groups(&mut groups);
        assert_eq!(groups.len(), 1);

        if let ResolutionType::MechanicalFix { ref proposed_replacements } = groups[0].resolution_type {
            for replacement in proposed_replacements {
                assert!(
                    replacement.from_pattern.is_some(),
                    "Replacement should have from_pattern, got None. name={}",
                    replacement.name,
                );
                let fp = replacement.from_pattern.as_ref().unwrap();
                assert!(
                    fp.contains("cba.com.au"),
                    "from_pattern should contain domain, got: {}",
                    fp,
                );
            }
        } else {
            panic!("Expected MechanicalFix, got {:?}", groups[0].resolution_type);
        }
    }
```

**Step 2: Run test to verify it fails**

Run: `cargo test --lib filter_remediation::tests::test_classify_mechanical_fix_from_in_query_field`
Expected: FAIL — `from_pattern` is `None` for filters with `from` only in `query` field

**Step 3: Replace lines 548-552**

In `src/filter_remediation.rs`, replace:

```rust
                let from_pattern = filter.from.clone();
                let is_specific_sender = from_pattern
                    .as_ref()
                    .map(|f| f.contains('@'))
                    .unwrap_or(false);
```

With:

```rust
                let (from_pattern, is_specific_sender) =
                    Self::extract_from_pattern(filter);
```

**Step 4: Run tests to verify they pass**

Run: `cargo test --lib filter_remediation::tests::test_classify_mechanical_fix_from_in_query_field`
Expected: PASS

Run: `cargo test --lib filter_remediation::tests`
Expected: ALL PASS (existing tests unaffected)

**Step 5: Commit**

```bash
git add src/filter_remediation.rs
git commit -m "fix: use extract_from_pattern in classify_groups to prevent from_pattern loss"
```

---

### Task 3: Add `reconstruct_filter_query` helper

Builds a full Gmail search query from an `ExistingFilterInfo`'s criteria.

**Files:**
- Modify: `src/filter_remediation.rs`

**Step 1: Write the failing test**

In `src/filter_remediation.rs` `mod tests`, add:

```rust
    #[test]
    fn test_reconstruct_query_from_query_field() {
        let filter = make_filter_with_query(
            "f1",
            None,
            Some("from:(*@cba.com.au) subject:(statement)"),
            None,
            "lbl_fin",
        );
        let query = OverlapDetector::reconstruct_filter_query(&filter);
        assert_eq!(query, "from:(*@cba.com.au) subject:(statement)");
    }

    #[test]
    fn test_reconstruct_query_from_individual_fields() {
        let filter = make_filter("f1", Some("cba.com.au"), Some("statement"), "lbl_fin");
        let query = OverlapDetector::reconstruct_filter_query(&filter);
        assert!(query.contains("from:(cba.com.au)"), "query={}", query);
        assert!(query.contains("subject:(statement)"), "query={}", query);
    }

    #[test]
    fn test_reconstruct_query_from_only() {
        let filter = make_filter("f1", Some("cba.com.au"), None, "lbl_fin");
        let query = OverlapDetector::reconstruct_filter_query(&filter);
        assert!(query.contains("from:(cba.com.au)"), "query={}", query);
        assert!(!query.contains("subject:"), "query should not have subject: {}", query);
    }

    #[test]
    fn test_reconstruct_query_prefers_query_field() {
        // When both query and from/subject fields exist, prefer query field
        let filter = make_filter_with_query(
            "f1",
            Some("cba.com.au"),
            Some("from:(*@cba.com.au) subject:(statement)"),
            Some("statement"),
            "lbl_fin",
        );
        let query = OverlapDetector::reconstruct_filter_query(&filter);
        assert_eq!(query, "from:(*@cba.com.au) subject:(statement)");
    }
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --lib filter_remediation::tests::test_reconstruct_query`
Expected: FAIL — `reconstruct_filter_query` doesn't exist

**Step 3: Implement the helper**

In `src/filter_remediation.rs`, add to `impl OverlapDetector`, after `extract_from_pattern`:

```rust
    /// Reconstruct a full Gmail search query from an ExistingFilterInfo.
    /// Prefers the raw `query` field (already a valid Gmail query).
    /// Falls back to building from `from` + `subject` individual fields.
    pub fn reconstruct_filter_query(filter: &ExistingFilterInfo) -> String {
        // Prefer the raw query field — it's exactly what Gmail uses
        if let Some(ref query) = filter.query {
            if !query.is_empty() {
                return query.clone();
            }
        }
        // Fallback: build from individual fields
        let mut parts = Vec::new();
        if let Some(ref from) = filter.from {
            if !from.is_empty() {
                parts.push(format!("from:({})", from));
            }
        }
        if let Some(ref subject) = filter.subject {
            if !subject.is_empty() {
                parts.push(format!("subject:({})", subject));
            }
        }
        parts.join(" ")
    }
```

**Step 4: Run tests to verify they pass**

Run: `cargo test --lib filter_remediation::tests::test_reconstruct_query`
Expected: ALL PASS (4 tests)

**Step 5: Commit**

```bash
git add src/filter_remediation.rs
git commit -m "feat: add reconstruct_filter_query helper for ExistingFilterInfo"
```

---

### Task 4: Refactor `LabelSwap` and `collect_swaps` — per-loser-filter swaps with full query

Change `LabelSwap` to carry `query: String` instead of `from_pattern: String`. Refactor `collect_swaps` to produce one swap per loser filter, scoped to the loser's full query. Update `apply` to use `swap.query`. This is the fix for Bugs 2 and 3.

**Files:**
- Modify: `src/filter_remediation.rs:300-399` (LabelSwap struct, collect_swaps, apply)
- Modify: `src-tauri/src/commands/remediation.rs` (Tauri callers — no logic change, just field rename)

**Step 1: Write the failing test**

In `src/filter_remediation.rs` `mod tests`, add:

```rust
    #[test]
    fn test_collect_swaps_uses_full_query() {
        let group = OverlapGroup {
            group_id: "cba.com.au".to_string(),
            from_pattern: "cba.com.au".to_string(),
            filters: vec![
                make_filter("f1", Some("cba.com.au"), None, "lbl_fin"),
                make_filter_with_query(
                    "f2",
                    None,
                    Some("from:(*@cba.com.au) subject:(receipt)"),
                    Some("receipt"),
                    "lbl_rec",
                ),
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

        // The swap must use the loser filter's full query, not just the domain
        let swap = &swaps[0];
        assert!(
            swap.query.contains("from:"),
            "Swap query should contain from: clause, got: '{}'",
            swap.query,
        );
        assert!(
            swap.query.contains("subject:"),
            "Swap query should contain subject: clause for loser filter that has subject, got: '{}'",
            swap.query,
        );
    }

    #[test]
    fn test_collect_swaps_per_loser_filter() {
        let group = OverlapGroup {
            group_id: "cba.com.au".to_string(),
            from_pattern: "cba.com.au".to_string(),
            filters: vec![
                make_filter("f1", Some("cba.com.au"), None, "lbl_fin"),
                make_filter("f2", Some("cba.com.au"), Some("statement"), "lbl_rec"),
                make_filter("f3", Some("cba.com.au"), Some("receipt"), "lbl_per"),
            ],
            label_names: vec!["Financial".to_string(), "Receipts".to_string(), "Personal".to_string()],
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
        // Should produce one swap per loser, not one swap per group
        assert_eq!(swaps.len(), 2, "Should have one swap per loser filter");
    }
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --lib filter_remediation::tests::test_collect_swaps_uses_full_query`
Expected: FAIL — `LabelSwap` has `from_pattern` not `query`

Run: `cargo test --lib filter_remediation::tests::test_collect_swaps_per_loser`
Expected: FAIL — produces 1 swap (per group) not 2 (per loser)

**Step 3: Update `LabelSwap` struct**

In `src/filter_remediation.rs`, replace lines 299-304:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabelSwap {
    pub from_pattern: String,
    pub add_label_id: String,
    pub remove_label_ids: Vec<String>,
}
```

With:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabelSwap {
    /// Full Gmail search query scoped to the loser filter's criteria
    pub query: String,
    pub add_label_id: String,
    pub remove_label_ids: Vec<String>,
}
```

**Step 4: Rewrite `collect_swaps` to produce per-loser-filter swaps**

Replace the entire `collect_swaps` method (lines 363-399) with:

```rust
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

                // Produce one swap per loser filter, scoped to that filter's full query
                for loser in group.filters.iter().filter(|f| f.id != *keep_filter_id) {
                    let loser_labels: Vec<String> = loser
                        .add_label_ids
                        .iter()
                        .filter(|l| **l != winner)
                        .cloned()
                        .collect();

                    if loser_labels.is_empty() {
                        continue;
                    }

                    let query = OverlapDetector::reconstruct_filter_query(loser);
                    if query.is_empty() {
                        continue;
                    }

                    swaps.push(LabelSwap {
                        query,
                        add_label_id: winner.clone(),
                        remove_label_ids: loser_labels,
                    });
                }
            }
        }

        swaps
    }
```

**Step 5: Update `apply` to use `swap.query`**

In `src/filter_remediation.rs`, in the `apply` method, replace lines 322-333:

```rust
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
```

With:

```rust
        for swap in swaps {
            let message_ids = match client.list_message_ids(&swap.query).await {
                Ok(ids) => ids,
                Err(e) => {
                    result.errors.push(format!(
                        "Failed to query messages for '{}': {}",
                        swap.query, e
                    ));
                    continue;
                }
            };
```

Also update the error message on lines 350-354 — replace `swap.from_pattern` with `swap.query`:

```rust
                    result.errors.push(format!(
                        "Failed to swap labels for '{}' ({} messages): {}",
                        swap.query,
                        message_ids.len(),
                        e
                    ));
```

**Step 6: Fix existing tests that reference `LabelSwap.from_pattern`**

In `test_collect_swaps_pick_winner_produces_swap` (line 1072), change:

```rust
        assert_eq!(swaps[0].from_pattern, "cba.com.au");
```

To:

```rust
        assert!(swaps[0].query.contains("from:(cba.com.au)"), "query={}", swaps[0].query);
```

Also update `test_apply_swaps_labels` (line 1232). Change the swap construction:

```rust
        let swaps = vec![LabelSwap {
            from_pattern: "cba.com.au".to_string(),
            add_label_id: "lbl_fin".to_string(),
            remove_label_ids: vec!["lbl_rec".to_string()],
        }];
```

To:

```rust
        let swaps = vec![LabelSwap {
            query: "from:(cba.com.au)".to_string(),
            add_label_id: "lbl_fin".to_string(),
            remove_label_ids: vec!["lbl_rec".to_string()],
        }];
```

And update the `MockApplyClient.list_message_ids` to match against the new query format. The existing mock checks `if query.contains(pattern)` where pattern is `"cba.com.au"`, so this should still match.

In `test_collect_swaps_pick_winner_produces_swap`, also note the swap count may change since we now produce per-loser swaps. In this test there's 1 winner and 1 loser, so we still get 1 swap. No change needed.

**Step 7: Run tests to verify they pass**

Run: `cargo test --lib filter_remediation::tests`
Expected: ALL PASS

**Step 8: Commit**

```bash
git add src/filter_remediation.rs
git commit -m "fix: scope LabelSwap to full filter query instead of domain-only"
```

---

### Task 5: Update Tauri callers for `LabelSwap` field rename

The Tauri frontend may reference `from_pattern` on `LabelSwap`. Update to `query`.

**Files:**
- Modify: `src-tauri/src/commands/remediation.rs` (uses `LabelSwap` type but doesn't access fields directly — verify)

**Step 1: Verify compilation**

Run: `cargo check --manifest-path src-tauri/Cargo.toml 2>&1 | head -20`
Expected: Either PASS (the Tauri code only passes `LabelSwap` through, doesn't access fields), or FAIL with specific field errors.

**Step 2: Fix any compilation errors**

If the Tauri code serializes `LabelSwap` to the frontend (it does — `collect_remediation_swaps` returns `Vec<LabelSwap>`), the rename from `from_pattern` to `query` will change the JSON field name. The Tauri commands at `src-tauri/src/commands/remediation.rs:92-98` just pass the type through, so the Rust code compiles — but any frontend JS that reads `.from_pattern` on the response would need updating.

Check for frontend references:

Run: `grep -r "from_pattern" ui/src/ 2>/dev/null | head -10`

Fix any references found in the frontend code.

**Step 3: Verify full build**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: PASS

**Step 4: Commit (if changes needed)**

```bash
git add src-tauri/ ui/
git commit -m "fix: update Tauri callers for LabelSwap query field rename"
```

---

### Task 6: Defensive subject extraction fallback in `classify_groups`

Add a raw regex fallback when `parse_gmail_query` returns no subject clause, to prevent `PickWinner` fallthrough. This is the fix for Bug 4.

**Files:**
- Modify: `src/filter_remediation.rs:503-527` (subject extraction loop in `classify_groups`)

**Step 1: Write the failing test**

In `src/filter_remediation.rs` `mod tests`, add:

```rust
    #[test]
    fn test_classify_mechanical_fix_subject_only_in_query() {
        // Filters where subject is ONLY in the query field, not the subject field
        let filters = vec![
            make_filter_with_query(
                "f1",
                Some("cba.com.au"),
                Some("from:(*@cba.com.au) subject:(statement)"),
                None,  // no subject field!
                "lbl_fin",
            ),
            make_filter_with_query(
                "f2",
                Some("cba.com.au"),
                Some("from:(*@cba.com.au)"),
                None,
                "lbl_oth",
            ),
        ];
        let mut groups = OverlapDetector::group_filters(&filters, &label_map());
        OverlapDetector::classify_groups(&mut groups);
        assert_eq!(groups.len(), 1);
        assert!(
            matches!(groups[0].resolution_type, ResolutionType::MechanicalFix { .. }),
            "Expected MechanicalFix when subject is in query field, got {:?}",
            groups[0].resolution_type,
        );
    }

    #[test]
    fn test_classify_mechanical_fix_subject_no_parens_in_query() {
        // Some filters store subject without parens: subject:statement instead of subject:(statement)
        let filters = vec![
            make_filter_with_query(
                "f1",
                Some("cba.com.au"),
                Some("from:(*@cba.com.au) subject:statement"),
                None,
                "lbl_fin",
            ),
            make_filter_with_query(
                "f2",
                Some("cba.com.au"),
                None,
                None,
                "lbl_oth",
            ),
        ];
        let mut groups = OverlapDetector::group_filters(&filters, &label_map());
        OverlapDetector::classify_groups(&mut groups);
        assert_eq!(groups.len(), 1);
        assert!(
            matches!(groups[0].resolution_type, ResolutionType::MechanicalFix { .. }),
            "Expected MechanicalFix when subject in query without parens, got {:?}",
            groups[0].resolution_type,
        );
    }
```

**Step 2: Run tests to verify they fail (or pass)**

Run: `cargo test --lib filter_remediation::tests::test_classify_mechanical_fix_subject_only_in_query`
Expected: Depends on whether `parse_gmail_query` handles these formats. If it handles them, test passes and no code change needed. If it fails, the test fails.

Run: `cargo test --lib filter_remediation::tests::test_classify_mechanical_fix_subject_no_parens_in_query`
Expected: Likely FAIL for the no-parens case.

**Step 3: Add raw regex fallback for subject extraction**

In `src/filter_remediation.rs`, in `classify_groups`, replace the subject extraction block (lines 503-527):

```rust
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
```

With:

```rust
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

                        // Defensive fallback: raw regex extraction if parser missed it
                        if keywords.is_empty() && query.contains("subject:") {
                            // Match subject:(words) or subject:word
                            let re = regex::Regex::new(
                                r"subject:\(([^)]+)\)|subject:(\S+)"
                            ).unwrap();
                            for cap in re.captures_iter(query) {
                                let value = cap.get(1).or(cap.get(2)).map(|m| m.as_str().to_string());
                                if let Some(v) = value {
                                    keywords.push(v);
                                }
                            }
                        }
                    }
                }

                if !keywords.is_empty() {
                    all_subject_keywords.extend(keywords.clone());
                }
                filter_subjects.push((i, keywords));
            }
```

**Step 4: Check if `regex` is a dependency**

Run: `grep -c '^regex' Cargo.toml` — if it's not a dependency, add it or use a simpler approach.

Alternative without regex (if regex is not a dependency): use string parsing:

```rust
                        // Defensive fallback: raw string extraction if parser missed it
                        if keywords.is_empty() && query.contains("subject:") {
                            for part in query.split_whitespace() {
                                if let Some(rest) = part.strip_prefix("subject:") {
                                    let value = rest
                                        .trim_start_matches('(')
                                        .trim_end_matches(')')
                                        .to_string();
                                    if !value.is_empty() {
                                        keywords.push(value);
                                    }
                                }
                            }
                        }
```

Use the string-parsing approach if `regex` is not a dependency. Use the regex approach if it is.

**Step 5: Run tests to verify they pass**

Run: `cargo test --lib filter_remediation::tests::test_classify_mechanical_fix_subject`
Expected: ALL PASS

Run: `cargo test --lib filter_remediation::tests`
Expected: ALL PASS

**Step 6: Commit**

```bash
git add src/filter_remediation.rs
git commit -m "fix: add defensive subject extraction fallback to prevent PickWinner fallthrough"
```

---

### Task 7: Add proptest strategies and property P1 (from_pattern never lost)

**Files:**
- Modify: `src/filter_remediation.rs` (add proptest module at end of `mod tests`)

**Step 1: Add proptest import and strategies**

At the top of `mod tests` in `src/filter_remediation.rs`, add:

```rust
    use proptest::prelude::*;
```

Then at the end of `mod tests`, add a sub-module:

```rust
    mod property_tests {
        use super::*;
        use proptest::prelude::*;

        #[derive(Debug, Clone)]
        enum FromLocation {
            FromField,
            QueryField,
            Both,
        }

        fn from_location_strategy() -> impl Strategy<Value = FromLocation> {
            prop_oneof![
                Just(FromLocation::FromField),
                Just(FromLocation::QueryField),
                Just(FromLocation::Both),
            ]
        }

        fn from_value_strategy() -> impl Strategy<Value = String> {
            prop_oneof![
                Just("noreply@cba.com.au".to_string()),
                Just("alerts@cba.com.au".to_string()),
                Just("cba.com.au".to_string()),
                Just("*@cba.com.au".to_string()),
            ]
        }

        fn subject_strategy() -> impl Strategy<Value = Option<String>> {
            prop_oneof![
                Just(None),
                Just(Some("statement".to_string())),
                Just(Some("receipt".to_string())),
                Just(Some("payment confirmation".to_string())),
            ]
        }

        fn label_strategy() -> impl Strategy<Value = String> {
            prop_oneof![
                Just("lbl_fin".to_string()),
                Just("lbl_rec".to_string()),
                Just("lbl_per".to_string()),
                Just("lbl_oth".to_string()),
            ]
        }

        /// Build an ExistingFilterInfo from configuration axes.
        /// The `from_value` is normalized: if it contains '@', it goes into the
        /// query as `from:(value)`, otherwise as `from:(*@value)`.
        fn build_test_filter(
            id: &str,
            from_value: &str,
            from_loc: &FromLocation,
            subject: &Option<String>,
            label: &str,
            archives: bool,
        ) -> ExistingFilterInfo {
            let from_query_part = if from_value.contains('@') {
                format!("from:({})", from_value)
            } else {
                format!("from:(*@{})", from_value)
            };
            let subject_query_part = subject
                .as_ref()
                .map(|s| format!(" subject:({})", s));
            let full_query = format!(
                "{}{}",
                from_query_part,
                subject_query_part.as_deref().unwrap_or("")
            );

            let (from_field, query_field) = match from_loc {
                FromLocation::FromField => (Some(from_value.to_string()), None),
                FromLocation::QueryField => (None, Some(full_query)),
                FromLocation::Both => (Some(from_value.to_string()), Some(full_query)),
            };

            ExistingFilterInfo {
                id: id.to_string(),
                from: from_field,
                query: query_field,
                to: None,
                subject: subject.clone(),
                add_label_ids: vec![label.to_string()],
                remove_label_ids: if archives {
                    vec!["INBOX".to_string()]
                } else {
                    vec![]
                },
            }
        }

        /// Domain key for grouping (must match for filters to land in same group)
        fn domain_for(from_value: &str) -> String {
            if from_value.contains('@') {
                from_value
                    .split('@')
                    .next_back()
                    .unwrap_or(from_value)
                    .trim_start_matches('*')
                    .to_string()
            } else {
                from_value.to_string()
            }
        }

        /// Strategy for a pair of filters guaranteed to share a domain
        fn filter_pair_strategy() -> impl Strategy<Value = (ExistingFilterInfo, ExistingFilterInfo)>
        {
            (
                from_value_strategy(),       // shared from_value (same domain)
                from_location_strategy(),    // from location for filter A
                from_location_strategy(),    // from location for filter B
                subject_strategy(),          // subject for filter A
                subject_strategy(),          // subject for filter B
                label_strategy(),            // label for filter A
                label_strategy(),            // label for filter B
                proptest::bool::ANY,         // archives A
                proptest::bool::ANY,         // archives B
            )
                .prop_map(
                    |(from_val, loc_a, loc_b, subj_a, subj_b, lbl_a, lbl_b, arc_a, arc_b)| {
                        let a = build_test_filter("f1", &from_val, &loc_a, &subj_a, &lbl_a, arc_a);
                        let b = build_test_filter("f2", &from_val, &loc_b, &subj_b, &lbl_b, arc_b);
                        (a, b)
                    },
                )
        }

        /// Strategy for a group of 2-4 filters sharing a domain
        fn filter_group_strategy() -> impl Strategy<Value = Vec<ExistingFilterInfo>> {
            (
                from_value_strategy(),
                proptest::collection::vec(
                    (
                        from_location_strategy(),
                        subject_strategy(),
                        label_strategy(),
                        proptest::bool::ANY,
                    ),
                    2..=4,
                ),
            )
                .prop_map(|(from_val, configs)| {
                    configs
                        .iter()
                        .enumerate()
                        .map(|(i, (loc, subj, lbl, arc))| {
                            build_test_filter(
                                &format!("f{}", i),
                                &from_val,
                                loc,
                                subj,
                                lbl,
                                *arc,
                            )
                        })
                        .collect()
                })
        }

        fn test_label_map() -> HashMap<String, String> {
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

        // --- Property P1: from_pattern never lost on MechanicalFix replacements ---

        proptest! {
            #[test]
            fn prop_from_pattern_never_lost(filters in filter_group_strategy()) {
                let label_map = test_label_map();
                let mut groups = OverlapDetector::group_filters(&filters, &label_map);
                OverlapDetector::classify_groups(&mut groups);

                for group in &groups {
                    if let ResolutionType::MechanicalFix { ref proposed_replacements } = group.resolution_type {
                        for r in proposed_replacements {
                            prop_assert!(
                                r.from_pattern.is_some(),
                                "Replacement lost from_pattern in group '{}', filter name '{}'",
                                group.group_id,
                                r.name,
                            );
                        }
                    }
                }
            }
        }
    }
```

**Step 2: Run the property test**

Run: `cargo test --lib filter_remediation::tests::property_tests::prop_from_pattern_never_lost`
Expected: PASS (should pass now that Bug 1 is fixed)

**Step 3: Commit**

```bash
git add src/filter_remediation.rs
git commit -m "test: add proptest strategies and P1 (from_pattern never lost)"
```

---

### Task 8: Add property P5 (MechanicalFix when subjects differ) and P8 (PickWinner only when no subjects)

**Files:**
- Modify: `src/filter_remediation.rs` (add to `property_tests` module)

**Step 1: Add property tests**

In `src/filter_remediation.rs`, inside `mod property_tests`, add inside the `proptest!` block:

```rust
        proptest! {
            // ... existing P1 test ...

            #[test]
            fn prop_mechanical_fix_when_subjects_differ(filters in filter_group_strategy()) {
                let label_map = test_label_map();
                let mut groups = OverlapDetector::group_filters(&filters, &label_map);
                OverlapDetector::classify_groups(&mut groups);

                for group in &groups {
                    // Check: does this group have subject criteria on at least one filter?
                    let has_subject_criteria = group.filters.iter().any(|f| {
                        // Check subject field
                        let has_subject_field = f.subject
                            .as_ref()
                            .map_or(false, |s| !s.is_empty());
                        // Check query field for subject:
                        let has_subject_in_query = f.query
                            .as_ref()
                            .map_or(false, |q| q.contains("subject:"));
                        has_subject_field || has_subject_in_query
                    });

                    let unique_labels: std::collections::HashSet<&str> = group
                        .filters
                        .iter()
                        .flat_map(|f| f.add_label_ids.iter().map(|s| s.as_str()))
                        .collect();

                    // P5: If subjects differ + different labels → must be MechanicalFix
                    if has_subject_criteria && unique_labels.len() > 1 {
                        prop_assert!(
                            matches!(group.resolution_type, ResolutionType::MechanicalFix { .. }),
                            "Group '{}' has subject criteria + different labels but got {:?}",
                            group.group_id,
                            group.resolution_type,
                        );
                    }
                }
            }

            #[test]
            fn prop_pick_winner_only_when_no_subjects(filters in filter_group_strategy()) {
                let label_map = test_label_map();
                let mut groups = OverlapDetector::group_filters(&filters, &label_map);
                OverlapDetector::classify_groups(&mut groups);

                for group in &groups {
                    if matches!(group.resolution_type, ResolutionType::PickWinner) {
                        for f in &group.filters {
                            // P8: No filter in a PickWinner group should have subject criteria
                            prop_assert!(
                                f.subject.as_ref().map_or(true, |s| s.is_empty()),
                                "PickWinner group '{}' has filter with subject field: {:?}",
                                group.group_id,
                                f.subject,
                            );
                            if let Some(ref q) = f.query {
                                prop_assert!(
                                    !q.contains("subject:"),
                                    "PickWinner group '{}' has filter with subject in query: {}",
                                    group.group_id,
                                    q,
                                );
                            }
                        }
                    }
                }
            }
        }
```

Note: You need to merge these into the existing `proptest!` block (all proptest tests go inside one `proptest! { }` macro invocation in the module), OR use separate `proptest!` blocks (both are valid).

**Step 2: Run the property tests**

Run: `cargo test --lib filter_remediation::tests::property_tests`
Expected: ALL PASS

**Step 3: Commit**

```bash
git add src/filter_remediation.rs
git commit -m "test: add P5 (MechanicalFix when subjects differ) and P8 (PickWinner only when no subjects)"
```

---

### Task 9: Add property P2 (apply query includes from), P6 (replacement from_pattern matches group), P7 (replacement mutual exclusivity)

**Files:**
- Modify: `src/filter_remediation.rs` (add to `property_tests` module)

**Step 1: Add property tests**

In the `property_tests` module, add to the `proptest!` block:

```rust
            #[test]
            fn prop_apply_query_includes_from(filters in filter_group_strategy()) {
                let label_map = test_label_map();
                let mut groups = OverlapDetector::group_filters(&filters, &label_map);
                OverlapDetector::classify_groups(&mut groups);

                // Build a plan with KeepOne for any PickWinner groups
                let mut plan = RemediationPlan::new();
                for group in groups {
                    if matches!(group.resolution_type, ResolutionType::PickWinner) {
                        if let Some(winner) = group.filters.first() {
                            plan.add(
                                group.clone(),
                                GroupDecision::KeepOne {
                                    keep_filter_id: winner.id.clone(),
                                },
                            );
                        }
                    }
                }

                let swaps = RemediationApplicator::collect_swaps(&plan);
                for swap in &swaps {
                    // P2: Every swap query must contain a from: clause
                    prop_assert!(
                        swap.query.contains("from:"),
                        "Swap query missing from: clause: '{}'",
                        swap.query,
                    );
                }
            }

            #[test]
            fn prop_replacement_from_matches_group(filters in filter_group_strategy()) {
                let label_map = test_label_map();
                let mut groups = OverlapDetector::group_filters(&filters, &label_map);
                OverlapDetector::classify_groups(&mut groups);

                for group in &groups {
                    if let ResolutionType::MechanicalFix { ref proposed_replacements } = group.resolution_type {
                        for r in proposed_replacements {
                            // P6: Every replacement's from_pattern domain must match group domain
                            if let Some(ref fp) = r.from_pattern {
                                let fp_lower = fp.to_lowercase();
                                let group_domain = group.from_pattern.to_lowercase();
                                prop_assert!(
                                    fp_lower.contains(&group_domain),
                                    "Replacement from_pattern '{}' doesn't match group domain '{}'",
                                    fp,
                                    group.from_pattern,
                                );
                            }
                        }
                    }
                }
            }

            #[test]
            fn prop_replacement_mutual_exclusivity(filters in filter_group_strategy()) {
                let label_map = test_label_map();
                let mut groups = OverlapDetector::group_filters(&filters, &label_map);
                OverlapDetector::classify_groups(&mut groups);

                for group in &groups {
                    if let ResolutionType::MechanicalFix { ref proposed_replacements } = group.resolution_type {
                        // P7: Check all pairs of replacements for mutual exclusivity
                        for (i, a) in proposed_replacements.iter().enumerate() {
                            for b in &proposed_replacements[i + 1..] {
                                let a_has_subject = !a.subject_keywords.is_empty();
                                let b_has_subject = !b.subject_keywords.is_empty();

                                if a_has_subject && b_has_subject {
                                    // Both have subjects — they must differ
                                    prop_assert_ne!(
                                        &a.subject_keywords,
                                        &b.subject_keywords,
                                        "Two replacements have identical subjects in group '{}'",
                                        group.group_id,
                                    );
                                } else if a_has_subject && !b_has_subject {
                                    // b is bare → must exclude a's subjects
                                    for kw in &a.subject_keywords {
                                        prop_assert!(
                                            b.excluded_subject_patterns.contains(kw),
                                            "Remainder missing exclusion for '{}' in group '{}'",
                                            kw,
                                            group.group_id,
                                        );
                                    }
                                } else if !a_has_subject && b_has_subject {
                                    // a is bare → must exclude b's subjects
                                    for kw in &b.subject_keywords {
                                        prop_assert!(
                                            a.excluded_subject_patterns.contains(kw),
                                            "Remainder missing exclusion for '{}' in group '{}'",
                                            kw,
                                            group.group_id,
                                        );
                                    }
                                }
                                // Both bare with no subjects should not happen in MechanicalFix
                            }
                        }
                    }
                }
            }
```

**Step 2: Run the property tests**

Run: `cargo test --lib filter_remediation::tests::property_tests`
Expected: ALL PASS

**Step 3: Commit**

```bash
git add src/filter_remediation.rs
git commit -m "test: add P2 (query includes from), P6 (from matches group), P7 (mutual exclusivity)"
```

---

### Task 10: Add property P9 (archive isolation)

**Files:**
- Modify: `src/filter_remediation.rs` (add to `property_tests` module)

**Step 1: Add property test**

In the `property_tests` module, add to the `proptest!` block:

```rust
            #[test]
            fn prop_archive_isolation(filters in filter_group_strategy()) {
                let label_map = test_label_map();
                let mut groups = OverlapDetector::group_filters(&filters, &label_map);
                OverlapDetector::classify_groups(&mut groups);

                let mut plan = RemediationPlan::new();
                for group in groups {
                    if matches!(group.resolution_type, ResolutionType::PickWinner) {
                        if let Some(winner) = group.filters.first() {
                            plan.add(
                                group.clone(),
                                GroupDecision::KeepOne {
                                    keep_filter_id: winner.id.clone(),
                                },
                            );
                        }
                    }
                }

                let swaps = RemediationApplicator::collect_swaps(&plan);
                for swap in &swaps {
                    // P9: Label swaps must never add or remove INBOX
                    // Archival is a filter-level action, not a swap concern
                    prop_assert!(
                        !swap.remove_label_ids.contains(&"INBOX".to_string()),
                        "Swap removes INBOX (archive leak): query='{}'",
                        swap.query,
                    );
                    prop_assert_ne!(
                        swap.add_label_id,
                        "INBOX",
                        "Swap adds INBOX: query='{}'",
                        swap.query,
                    );
                }
            }
```

**Step 2: Run the property test**

Run: `cargo test --lib filter_remediation::tests::property_tests::prop_archive_isolation`
Expected: PASS

**Step 3: Commit**

```bash
git add src/filter_remediation.rs
git commit -m "test: add P9 (archive isolation in label swaps)"
```

---

### Task 11: Run full test suite and clippy

**Step 1: Run all tests**

Run: `cargo test`
Expected: ALL PASS

**Step 2: Run clippy**

Run: `cargo clippy -- -D warnings 2>&1 | tail -20`
Expected: No warnings

**Step 3: Check Tauri build**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: PASS

**Step 4: Fix any issues found, commit**

```bash
git add -A
git commit -m "chore: fix clippy warnings and ensure full build passes"
```

---

Plan complete and saved to `docs/plans/2026-04-04-fix-remediation-bugs-impl.md`. Two execution options:

**1. Subagent-Driven (this session)** — I dispatch fresh subagent per task, review between tasks, fast iteration

**2. Parallel Session (separate)** — Open new session with executing-plans, batch execution with checkpoints

Which approach?