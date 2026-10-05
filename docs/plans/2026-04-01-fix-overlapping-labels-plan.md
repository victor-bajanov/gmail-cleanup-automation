# Fix Overlapping Automanaged Labels — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Ensure emails from a single sender never receive multiple overlapping Automanaged labels by making subject-based filter routing mutually exclusive.

**Architecture:** Add `excluded_subject_patterns` field to `EmailCluster` and `FilterRule`. Propagate extracted subject patterns as exclusions on remainder clusters, emit `-subject:` terms in Gmail queries, and add a safety-net overlap check in deduplication. All changes in `src/` library layer — both CLI and Tauri GUI consume these types.

**Tech Stack:** Rust, serde, Gmail query syntax

---

### Task 1: Add `excluded_subject_patterns` field to `FilterRule`

**Files:**
- Modify: `src/models.rs:42-54` (FilterRule struct)

**Step 1: Write the failing test**

In `src/models.rs`, add to the existing `mod tests` block:

```rust
#[test]
fn test_filter_rule_has_excluded_subject_patterns() {
    let filter = FilterRule {
        id: None,
        name: "Test".to_string(),
        from_pattern: Some("*@cba.com.au".to_string()),
        is_specific_sender: false,
        excluded_senders: vec![],
        subject_keywords: vec![],
        excluded_subject_patterns: vec!["statement".to_string()],
        target_label_id: "label-id".to_string(),
        should_archive: false,
        estimated_matches: 10,
    };
    assert_eq!(filter.excluded_subject_patterns, vec!["statement".to_string()]);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --lib models::tests::test_filter_rule_has_excluded_subject_patterns`
Expected: FAIL — `excluded_subject_patterns` field doesn't exist

**Step 3: Add the field to `FilterRule`**

In `src/models.rs`, add after `subject_keywords: Vec<String>,` (line 50):

```rust
    /// Subject patterns to exclude (for remainder clusters that coexist with subject-specific clusters)
    pub excluded_subject_patterns: Vec<String>,
```

**Step 4: Fix all existing `FilterRule` construction sites**

Every place that builds a `FilterRule` needs `excluded_subject_patterns: vec![]`. These are:

- `src/filter_manager.rs:877` — test `test_build_gmail_query` (the base filter and its derivatives)
- `src/filter_manager.rs` — `build_filter_rule` method (search for `FilterRule {`)
- `src/cli.rs:1598` — decision-to-filter conversion
- `src-tauri/src/commands/filters.rs:172` — Tauri `generate_proposed_filters`

Add `excluded_subject_patterns: vec![],` to each.

**Step 5: Run tests to verify it passes**

Run: `cargo test --lib models::tests::test_filter_rule_has_excluded_subject_patterns`
Expected: PASS

**Step 6: Commit**

```bash
git add src/models.rs src/filter_manager.rs src/cli.rs src-tauri/src/commands/filters.rs
git commit -m "feat: add excluded_subject_patterns field to FilterRule"
```

---

### Task 2: Add `excluded_subject_patterns` field to `EmailCluster`

**Files:**
- Modify: `src/interactive.rs:22-50` (EmailCluster struct)
- Modify: `src/interactive.rs:1268-1334` (build_cluster_with_subject)

**Step 1: Write the failing test**

In `src/interactive.rs`, add to the existing `mod tests` block:

```rust
#[test]
fn test_cluster_has_excluded_subject_patterns() {
    let cluster = EmailCluster {
        sender_domain: "cba.com.au".to_string(),
        sender_email: "noreply@cba.com.au".to_string(),
        is_specific_sender: true,
        excluded_senders: vec![],
        subject_pattern: None,
        excluded_subject_patterns: vec!["statement".to_string(), "receipt".to_string()],
        message_ids: vec!["1".to_string()],
        suggested_category: EmailCategory::Other,
        suggested_label: "auto/other".to_string(),
        confidence: 0.9,
        sample_subjects: vec![],
        should_archive: false,
        existing_filter_id: None,
        existing_filter_label_id: None,
        existing_filter_label: None,
        existing_filter_archive: None,
        source: ClusterSource::EmailScan,
        default_action: None,
    };
    assert_eq!(cluster.excluded_subject_patterns.len(), 2);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --lib interactive::tests::test_cluster_has_excluded_subject_patterns`
Expected: FAIL — field doesn't exist

**Step 3: Add the field to `EmailCluster`**

In `src/interactive.rs`, add after `pub subject_pattern: Option<String>,` (line 31):

```rust
    /// Subject patterns to exclude from this cluster (patterns that have their own clusters)
    pub excluded_subject_patterns: Vec<String>,
```

**Step 4: Add default value in `build_cluster_with_subject`**

In the `EmailCluster { ... }` construction at line 1316, add after `subject_pattern,`:

```rust
        excluded_subject_patterns: vec![],
```

**Step 5: Fix all other `EmailCluster` construction sites**

Add `excluded_subject_patterns: vec![],` to:

- `src/cli.rs:697` — synthetic cluster from orphaned filter
- `src/interactive.rs:1522` — test `test_cluster_decision`

**Step 6: Run tests to verify it passes**

Run: `cargo test --lib interactive::tests::test_cluster_has_excluded_subject_patterns`
Expected: PASS

**Step 7: Commit**

```bash
git add src/interactive.rs src/cli.rs
git commit -m "feat: add excluded_subject_patterns field to EmailCluster"
```

---

### Task 3: Propagate subject pattern exclusions to remainder clusters

**Files:**
- Modify: `src/interactive.rs:1081-1171` (create_clusters function)
- Modify: `src/interactive.rs:1250-1265` (build_cluster signature)
- Modify: `src/interactive.rs:1268-1275` (build_cluster_with_subject signature)

**Step 1: Write the failing test**

In `src/interactive.rs`, add to the existing `mod tests`:

```rust
#[test]
fn test_remainder_cluster_excludes_subject_patterns() {
    // Create messages from one sender with different subject patterns
    let mut messages = Vec::new();
    let mut classifications = Vec::new();

    // 3 "statement" emails → Financial
    for i in 0..3 {
        let msg = create_test_message(&format!("s{}", i), "noreply@cba.com.au", "Your statement is ready");
        let mut class = create_test_classification(&msg);
        class.category = EmailCategory::Financial;
        class.suggested_label = "auto/financial".to_string();
        classifications.push((msg.clone(), class));
        messages.push(msg);
    }

    // 3 "receipt" emails → Receipt
    for i in 0..3 {
        let msg = create_test_message(&format!("r{}", i), "noreply@cba.com.au", "Your payment receipt");
        let mut class = create_test_classification(&msg);
        class.category = EmailCategory::Receipt;
        class.suggested_label = "auto/receipts".to_string();
        classifications.push((msg.clone(), class));
        messages.push(msg);
    }

    // 3 generic emails → Other (these become the remainder cluster)
    for i in 0..3 {
        let msg = create_test_message(&format!("g{}", i), "noreply@cba.com.au", &format!("Notification {}", i));
        let mut class = create_test_classification(&msg);
        class.category = EmailCategory::Other;
        class.suggested_label = "auto/other".to_string();
        classifications.push((msg.clone(), class));
        messages.push(msg);
    }

    let clusters = create_clusters(&messages, &classifications, 3);

    // Find the remainder cluster (no subject_pattern)
    let remainder = clusters.iter().find(|c| c.subject_pattern.is_none());
    assert!(remainder.is_some(), "Should have a remainder cluster");

    let remainder = remainder.unwrap();
    assert!(!remainder.excluded_subject_patterns.is_empty(),
        "Remainder cluster should have excluded_subject_patterns");

    // The excluded patterns should include the subject patterns from the specific clusters
    let subject_clusters: Vec<_> = clusters.iter()
        .filter(|c| c.subject_pattern.is_some())
        .collect();
    assert!(subject_clusters.len() >= 2, "Should have at least 2 subject-pattern clusters");

    for sc in &subject_clusters {
        let pattern = sc.subject_pattern.as_ref().unwrap();
        assert!(
            remainder.excluded_subject_patterns.contains(pattern),
            "Remainder should exclude pattern '{}', but excluded_subject_patterns = {:?}",
            pattern,
            remainder.excluded_subject_patterns
        );
    }
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --lib interactive::tests::test_remainder_cluster_excludes_subject_patterns`
Expected: FAIL — `excluded_subject_patterns` is empty on remainder cluster

**Step 3: Update `build_cluster` and `build_cluster_with_subject` signatures**

Add `excluded_subject_patterns: Vec<String>` parameter to both functions.

In `build_cluster` (line 1250):

```rust
fn build_cluster(
    domain: &str,
    sender_email: &str,
    is_specific_sender: bool,
    excluded_senders: Vec<String>,
    excluded_subject_patterns: Vec<String>,
    msgs: &[(&MessageMetadata, &Classification)],
) -> EmailCluster {
    build_cluster_with_subject(
        domain,
        sender_email,
        is_specific_sender,
        excluded_senders,
        excluded_subject_patterns,
        None,
        msgs,
    )
}
```

In `build_cluster_with_subject` (line 1268):

```rust
fn build_cluster_with_subject(
    domain: &str,
    sender_email: &str,
    is_specific_sender: bool,
    excluded_senders: Vec<String>,
    excluded_subject_patterns: Vec<String>,
    subject_pattern: Option<String>,
    msgs: &[(&MessageMetadata, &Classification)],
) -> EmailCluster {
```

And in the `EmailCluster { ... }` construction inside `build_cluster_with_subject`, change to:

```rust
        excluded_subject_patterns,
```

**Step 4: Update `create_clusters` to collect and pass subject pattern exclusions**

In `create_clusters` (line 1113), track extracted patterns per sender:

```rust
        for (sender_email, sender_msgs) in sender_map {
            let subject_patterns = detect_subject_patterns(&sender_msgs, min_emails);

            let mut sender_remaining: Vec<(&MessageMetadata, &Classification)> =
                sender_msgs.clone();

            // Track which subject patterns were extracted for this sender
            let mut extracted_patterns: Vec<String> = Vec::new();

            for (pattern, pattern_msgs) in subject_patterns {
                if pattern_msgs.len() >= min_emails {
                    let cluster = build_cluster_with_subject(
                        &domain,
                        &sender_email,
                        true,
                        vec![],
                        vec![],  // subject-specific clusters don't need exclusions
                        Some(pattern.clone()),
                        &pattern_msgs,
                    );
                    clusters.push(cluster);

                    // Track this pattern for exclusion on the remainder cluster
                    extracted_patterns.push(pattern);

                    let pattern_ids: HashSet<String> =
                        pattern_msgs.iter().map(|(m, _)| m.id.clone()).collect();
                    sender_remaining.retain(|(m, _)| !pattern_ids.contains(&m.id));
                }
            }

            if sender_remaining.len() >= min_emails {
                specific_senders.push(sender_email.clone());

                let cluster = build_cluster(
                    &domain,
                    &sender_email,
                    true,
                    vec![],
                    extracted_patterns,  // pass subject exclusions to remainder
                    &sender_remaining,
                );
                clusters.push(cluster);
            } else {
                remaining_msgs.extend(sender_remaining);
            }
        }
```

Also update the domain-level remainder cluster (line 1162) to pass empty exclusions:

```rust
        if remaining_msgs.len() >= min_emails {
            let cluster = build_cluster(
                &domain,
                "",
                false,
                specific_senders.clone(),
                vec![],  // domain clusters don't need subject exclusions (they use sender exclusions)
                &remaining_msgs,
            );
            clusters.push(cluster);
        }
```

**Step 5: Run tests to verify it passes**

Run: `cargo test --lib interactive::tests::test_remainder_cluster_excludes_subject_patterns`
Expected: PASS

Run: `cargo test --lib interactive::tests`
Expected: ALL PASS (existing tests still work)

**Step 6: Commit**

```bash
git add src/interactive.rs
git commit -m "feat: propagate subject pattern exclusions to remainder clusters"
```

---

### Task 4: Emit `-subject:` exclusions in Gmail query builder

**Files:**
- Modify: `src/filter_manager.rs:245-284` (build_gmail_query_static)

**Step 1: Write the failing test**

In `src/filter_manager.rs`, add a new test inside `mod tests`:

```rust
#[test]
fn test_build_gmail_query_with_excluded_subject_patterns() {
    let mock_client = MockTestGmailClient::new();
    let manager = FilterManager::new(Box::new(mock_client));

    let filter = FilterRule {
        id: None,
        name: "CBA remainder".to_string(),
        from_pattern: Some("noreply@cba.com.au".to_string()),
        is_specific_sender: true,
        excluded_senders: vec![],
        subject_keywords: vec![],
        excluded_subject_patterns: vec!["statement".to_string(), "receipt".to_string()],
        target_label_id: "label-id".to_string(),
        should_archive: false,
        estimated_matches: 5,
    };

    let query = manager.build_gmail_query(&filter);
    assert!(query.contains("from:(noreply@cba.com.au)"));
    assert!(query.contains("-subject:(statement)"));
    assert!(query.contains("-subject:(receipt)"));
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --lib filter_manager::tests::test_build_gmail_query_with_excluded_subject_patterns`
Expected: FAIL — query doesn't contain `-subject:` terms

**Step 3: Add exclusion logic to `build_gmail_query_static`**

In `src/filter_manager.rs`, after the `subject_keywords` block (after line 281), add:

```rust
        // Add subject pattern exclusions (for remainder clusters)
        for excluded_pattern in &filter.excluded_subject_patterns {
            query_parts.push(format!("-subject:({})", excluded_pattern));
        }
```

**Step 4: Run tests to verify it passes**

Run: `cargo test --lib filter_manager::tests::test_build_gmail_query_with_excluded_subject_patterns`
Expected: PASS

Run: `cargo test --lib filter_manager::tests::test_build_gmail_query`
Expected: PASS (existing test unaffected — its filters have empty excluded_subject_patterns)

**Step 5: Commit**

```bash
git add src/filter_manager.rs
git commit -m "feat: emit -subject: exclusions in Gmail query builder"
```

---

### Task 5: Add broad-vs-specific overlap detection in deduplication

**Files:**
- Modify: `src/filter_manager.rs:784-810` (is_redundant_filter)

**Step 1: Write the failing test**

In `src/filter_manager.rs`, add a new test:

```rust
#[test]
fn test_dedup_catches_broad_filter_overlapping_specific() {
    let mock_client = MockTestGmailClient::new();
    let manager = FilterManager::new(Box::new(mock_client));

    // Specific filter: from:noreply@cba.com.au subject:statement → Financial
    let specific_filter = FilterRule {
        id: None,
        name: "CBA statements".to_string(),
        from_pattern: Some("noreply@cba.com.au".to_string()),
        is_specific_sender: true,
        excluded_senders: vec![],
        subject_keywords: vec!["statement".to_string()],
        excluded_subject_patterns: vec![],
        target_label_id: "financial".to_string(),
        should_archive: false,
        estimated_matches: 10,
    };

    // Broad filter: from:noreply@cba.com.au (no subject) → Other
    // This should be caught as redundant because the specific filter
    // already covers a subset and the broad one has no differentiation
    let broad_filter = FilterRule {
        id: None,
        name: "CBA other".to_string(),
        from_pattern: Some("noreply@cba.com.au".to_string()),
        is_specific_sender: true,
        excluded_senders: vec![],
        subject_keywords: vec![],
        excluded_subject_patterns: vec![],
        target_label_id: "other".to_string(),
        should_archive: false,
        estimated_matches: 5,
    };

    let filters = vec![specific_filter, broad_filter];
    let deduplicated = manager.deduplicate_filters(filters);

    // Only the specific filter should survive
    assert_eq!(deduplicated.len(), 1, "Broad filter should be deduplicated");
    assert_eq!(deduplicated[0].subject_keywords, vec!["statement".to_string()]);
}

#[test]
fn test_dedup_keeps_filter_with_subject_exclusions() {
    let mock_client = MockTestGmailClient::new();
    let manager = FilterManager::new(Box::new(mock_client));

    // Specific filter: from:noreply@cba.com.au subject:statement → Financial
    let specific_filter = FilterRule {
        id: None,
        name: "CBA statements".to_string(),
        from_pattern: Some("noreply@cba.com.au".to_string()),
        is_specific_sender: true,
        excluded_senders: vec![],
        subject_keywords: vec!["statement".to_string()],
        excluded_subject_patterns: vec![],
        target_label_id: "financial".to_string(),
        should_archive: false,
        estimated_matches: 10,
    };

    // Remainder filter WITH exclusions: from:noreply@cba.com.au -subject:statement → Other
    // This should NOT be caught as redundant — it has proper exclusions
    let remainder_filter = FilterRule {
        id: None,
        name: "CBA other".to_string(),
        from_pattern: Some("noreply@cba.com.au".to_string()),
        is_specific_sender: true,
        excluded_senders: vec![],
        subject_keywords: vec![],
        excluded_subject_patterns: vec!["statement".to_string()],
        target_label_id: "other".to_string(),
        should_archive: false,
        estimated_matches: 5,
    };

    let filters = vec![specific_filter, remainder_filter];
    let deduplicated = manager.deduplicate_filters(filters);

    // Both should survive — they target different non-overlapping emails
    assert_eq!(deduplicated.len(), 2, "Remainder with exclusions should be kept");
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --lib filter_manager::tests::test_dedup_catches_broad_filter_overlapping_specific`
Expected: FAIL — broad filter is not caught

Run: `cargo test --lib filter_manager::tests::test_dedup_keeps_filter_with_subject_exclusions`
Expected: PASS (this one should already pass since both filters have different dedup keys)

**Step 3: Add overlap check to `is_redundant_filter`**

In `src/filter_manager.rs`, inside `is_redundant_filter` (line 784), add before the final `false`:

```rust
            // Check for broad-vs-specific overlap: if an existing filter has the same
            // from_pattern with subject keywords, and the new filter has the same
            // from_pattern but NO subject keywords and NO subject exclusions,
            // the new filter is a broad catch-all that overlaps the specific one.
            if new_from == existing_from
                && !existing_filter.subject_keywords.is_empty()
                && filter.subject_keywords.is_empty()
                && filter.excluded_subject_patterns.is_empty()
            {
                return true;
            }
```

**Step 4: Run tests to verify they pass**

Run: `cargo test --lib filter_manager::tests::test_dedup_catches_broad_filter_overlapping_specific`
Expected: PASS

Run: `cargo test --lib filter_manager::tests::test_dedup_keeps_filter_with_subject_exclusions`
Expected: PASS

Run: `cargo test --lib filter_manager::tests`
Expected: ALL PASS

**Step 5: Commit**

```bash
git add src/filter_manager.rs
git commit -m "feat: detect broad-vs-specific filter overlaps in deduplication"
```

---

### Task 6: Propagate `excluded_subject_patterns` in CLI filter conversion

**Files:**
- Modify: `src/cli.rs:1565-1610` (decision-to-filter conversion)

**Step 1: Read context**

The `ClusterDecision` struct already carries `subject_pattern: Option<String>` and `excluded_senders: Vec<String>`. We need it to also carry `excluded_subject_patterns`.

**Step 2: Add field to `ClusterDecision`**

In `src/interactive.rs`, add to `ClusterDecision` struct (after `subject_pattern`):

```rust
    /// Subject patterns to exclude from this cluster's filter
    pub excluded_subject_patterns: Vec<String>,
```

**Step 3: Propagate in all `ClusterDecision` construction sites**

In `src/interactive.rs`, every place that builds a `ClusterDecision` from a cluster needs:

```rust
excluded_subject_patterns: cluster.excluded_subject_patterns.clone(),
```

Search for `ClusterDecision {` in `interactive.rs` — there are multiple (accept, reject, delete, exclude, custom actions). Add the field to each.

**Step 4: Propagate in CLI filter conversion**

In `src/cli.rs:1598`, update the `FilterRule` construction to include:

```rust
excluded_subject_patterns: d.excluded_subject_patterns.clone(),
```

**Step 5: Propagate in Tauri filter conversion**

In `src-tauri/src/commands/filters.rs:172`, update the `FilterRule` construction to include:

```rust
excluded_subject_patterns: cluster.excluded_subject_patterns.clone(),
```

**Step 6: Run full test suite**

Run: `cargo test --lib`
Expected: ALL PASS

Run: `cargo test`
Expected: ALL PASS

**Step 7: Commit**

```bash
git add src/interactive.rs src/cli.rs src-tauri/src/commands/filters.rs
git commit -m "feat: propagate excluded_subject_patterns through decisions to filters"
```

---

### Task 7: Final integration verification

**Step 1: Run full test suite**

Run: `cargo test`
Expected: ALL PASS

**Step 2: Run clippy**

Run: `cargo clippy -- -D warnings`
Expected: No warnings

**Step 3: Verify compilation of Tauri app**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: OK

**Step 4: Commit if any fixes needed, then done**
