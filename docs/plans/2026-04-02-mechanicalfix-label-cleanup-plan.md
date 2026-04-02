# MechanicalFix Label Cleanup: Remove Stale Labels from Existing Emails

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** After MechanicalFix remediation replaces overlapping filters with exclusive ones, clean up existing emails so each message only carries the one label that its new exclusive filter would assign — removing the stale labels from the old overlapping filters.

**Architecture:** Extend `RemediationApplicator::collect_swaps` to also produce swaps for `ReplaceWithExclusive` decisions. For each replacement filter, query Gmail for matching messages and ensure they have only that filter's label, removing all other labels from the group.

**Tech Stack:** Rust, async, existing `GmailClient::list_message_ids` + `batch_modify_labels`, `FilterManager::build_gmail_query_static`.

---

## Background: The Problem in Detail

When the tool previously created overlapping filters like:

```
from:(*@edm.cba.com.au) subject:(statement)     → Financial/Cba-com-au
from:(*@edm.cba.com.au) subject:(receipt)        → Receipts/Cba-com-au
from:(*@edm.cba.com.au)                          → Other/Cba-com-au    (catch-all)
```

The catch-all matched ALL emails from that domain (including ones with "statement" and "receipt" subjects), so a "statement" email got BOTH `Financial/Cba-com-au` AND `Other/Cba-com-au`. After remediation, the filters are fixed with `-subject:` exclusions, but the stale labels remain on existing emails.

After MechanicalFix remediation, the new exclusive filters are:

```
from:(*@edm.cba.com.au) subject:(statement)                                → Financial/Cba-com-au
from:(*@edm.cba.com.au) subject:(receipt)                                  → Receipts/Cba-com-au
from:(*@edm.cba.com.au) -subject:(statement) -subject:(receipt)            → Other/Cba-com-au
```

Now we need to fix existing emails: for each replacement filter, find matching messages and ensure they ONLY have that filter's label — removing all other labels from the same overlap group.

## Key Insight: How to Build the Swaps

For a `ReplaceWithExclusive` decision, the plan's `replacement_filters` field contains `Vec<FilterRule>`. Each `FilterRule` has:
- `from_pattern` — sender domain
- `subject_keywords` — subject filter (empty for catch-all)
- `excluded_subject_patterns` — subjects to exclude (for catch-all)
- `target_label_id` — the correct label for matching messages

The group's `filters` field contains all the original overlapping `ExistingFilterInfo` entries, each with `add_label_ids`.

For each replacement filter, we can:
1. Build the Gmail query using `FilterManager::build_gmail_query_static(replacement)` — this produces the exact query that the new filter matches
2. The `add_label_id` (winner) is `replacement.target_label_id`
3. The `remove_label_ids` (losers) are all OTHER label IDs from the group's original filters, excluding the winner

This means one `LabelSwap` per replacement filter in the group.

## Existing Code Context

### `RemediationApplicator::collect_swaps` (src/filter_remediation.rs ~line 355)

Currently only handles `KeepOne`:

```rust
pub fn collect_swaps(plan: &RemediationPlan) -> Vec<LabelSwap> {
    let mut swaps = Vec::new();
    for (group, decision) in &plan.groups {
        if let GroupDecision::KeepOne { keep_filter_id } = decision {
            // ... builds one LabelSwap per group
        }
    }
    swaps
}
```

### `LabelSwap` struct (src/filter_remediation.rs ~line 300)

```rust
pub struct LabelSwap {
    pub from_pattern: String,      // Gmail query to find affected messages
    pub add_label_id: String,      // winner label
    pub remove_label_ids: Vec<String>, // loser label(s)
}
```

**Problem:** `from_pattern` is currently used as a simple `from:{domain}` query. For MechanicalFix swaps, we need the FULL Gmail query (including subject keywords and exclusions). The field name is misleading but the apply code just does `format!("from:{}", swap.from_pattern)` — we need to change this.

### `RemediationApplicator::apply` (src/filter_remediation.rs ~line 316)

Currently builds the query as:
```rust
let query = format!("from:{}", swap.from_pattern);
```

This needs to accept a full pre-built query instead.

### `FilterManager::build_gmail_query_static` (src/filter_manager.rs line 245)

Takes a `&FilterRule` and returns the full Gmail query string:
```
from:(*@edm.cba.com.au) subject:(statement)
from:(*@edm.cba.com.au) -subject:(statement) -subject:(receipt)
```

This is exactly what we need to build the per-replacement-filter query.

### CLI apply phase (src/main.rs ~line 798)

Shows swaps and asks for confirmation. Currently shows `swap.from_pattern` — needs to show the full query or a human-readable summary.

### Tauri commands (src-tauri/src/commands/remediation.rs)

`collect_remediation_swaps` and `apply_remediation_swaps` — should work without changes since they call `collect_swaps` and `apply` which we're extending.

---

## Implementation Tasks

### Task 1: Rename `LabelSwap.from_pattern` to `query` and update all references

The field currently holds a domain pattern but will now hold full Gmail queries. Rename for clarity.

**Files:**
- Modify: `src/filter_remediation.rs` — rename field in struct, update `collect_swaps` (where it sets the field), update `apply` (where it reads the field and builds the Gmail query)
- Modify: `src/main.rs` — update CLI display code that reads `swap.from_pattern`

**Steps:**

1. In `src/filter_remediation.rs`, rename the `LabelSwap` struct field:

```rust
pub struct LabelSwap {
    pub query: String,             // Full Gmail query to find affected messages
    pub add_label_id: String,
    pub remove_label_ids: Vec<String>,
}
```

2. In `collect_swaps`, where it currently sets `from_pattern: group.from_pattern.clone()`, change to `query: format!("from:{}", group.from_pattern)` — this preserves existing KeepOne behavior.

3. In `apply`, change `let query = format!("from:{}", swap.from_pattern);` to just `let query = &swap.query;` (it's now pre-built).

4. In `src/main.rs` CLI apply phase, update the display. Currently:
```rust
let query = format!("from:{}", swap.from_pattern);
let count = client.list_message_ids(&query).await...;
println!("  {} — {} → {} (~{} emails)", swap.from_pattern, ...);
```
Change to:
```rust
let count = client.list_message_ids(&swap.query).await...;
println!("  {} — {} → {} (~{} emails)", swap.query, ...);
```

5. Update ALL tests that reference `from_pattern` on `LabelSwap` to use `query`.

6. Run `cargo test --lib filter_remediation -p gmail-automation` — all tests pass.

7. Run `cargo check` (including Tauri) — compiles.

8. Commit: `refactor: rename LabelSwap.from_pattern to query for full query support`

### Task 2: Extend `collect_swaps` to handle `ReplaceWithExclusive` decisions

**Files:**
- Modify: `src/filter_remediation.rs` — add ReplaceWithExclusive branch in `collect_swaps`, add import for `FilterManager`

**Steps:**

1. Write the failing test first. Add to `mod tests` in `src/filter_remediation.rs`:

```rust
#[test]
fn test_collect_swaps_mechanical_fix_produces_per_filter_swaps() {
    // Simulates edm.cba.com.au with 3 overlapping filters being replaced
    let filters = vec![
        make_filter("f1", Some("edm.cba.com.au"), Some("statement"), "lbl_fin"),
        make_filter("f2", Some("edm.cba.com.au"), Some("receipt"), "lbl_rec"),
        make_filter("f3", Some("edm.cba.com.au"), None, "lbl_oth"),
    ];

    let replacement1 = FilterRule {
        id: None,
        name: "edm.cba.com.au_lbl_fin".to_string(),
        from_pattern: Some("edm.cba.com.au".to_string()),
        is_specific_sender: false,
        excluded_senders: vec![],
        subject_keywords: vec!["statement".to_string()],
        excluded_subject_patterns: vec![],
        target_label_id: "lbl_fin".to_string(),
        should_archive: false,
        estimated_matches: 0,
    };
    let replacement2 = FilterRule {
        id: None,
        name: "edm.cba.com.au_lbl_rec".to_string(),
        from_pattern: Some("edm.cba.com.au".to_string()),
        is_specific_sender: false,
        excluded_senders: vec![],
        subject_keywords: vec!["receipt".to_string()],
        excluded_subject_patterns: vec![],
        target_label_id: "lbl_rec".to_string(),
        should_archive: false,
        estimated_matches: 0,
    };
    let replacement3 = FilterRule {
        id: None,
        name: "edm.cba.com.au_lbl_oth".to_string(),
        from_pattern: Some("edm.cba.com.au".to_string()),
        is_specific_sender: false,
        excluded_senders: vec![],
        subject_keywords: vec![],
        excluded_subject_patterns: vec!["statement".to_string(), "receipt".to_string()],
        target_label_id: "lbl_oth".to_string(),
        should_archive: false,
        estimated_matches: 0,
    };

    let group = OverlapGroup {
        group_id: "edm.cba.com.au".to_string(),
        from_pattern: "edm.cba.com.au".to_string(),
        filters,
        label_names: vec![
            "Financial".to_string(),
            "Receipts".to_string(),
            "Other".to_string(),
        ],
        resolution_type: ResolutionType::MechanicalFix {
            proposed_replacements: vec![
                replacement1.clone(),
                replacement2.clone(),
                replacement3.clone(),
            ],
        },
    };

    let mut plan = RemediationPlan::new();
    plan.add(
        group,
        GroupDecision::ReplaceWithExclusive {
            replacement_filters: vec![replacement1, replacement2, replacement3],
        },
    );

    let swaps = RemediationApplicator::collect_swaps(&plan);

    // Should produce 3 swaps — one per replacement filter
    assert_eq!(swaps.len(), 3);

    // Each swap's query should be the full Gmail query for that replacement filter
    // (built by FilterManager::build_gmail_query_static)
    
    // The "statement" filter swap should add lbl_fin and remove lbl_rec + lbl_oth
    let statement_swap = swaps.iter().find(|s| s.query.contains("subject:(statement)")).unwrap();
    assert_eq!(statement_swap.add_label_id, "lbl_fin");
    assert!(statement_swap.remove_label_ids.contains(&"lbl_rec".to_string()));
    assert!(statement_swap.remove_label_ids.contains(&"lbl_oth".to_string()));
    assert!(!statement_swap.remove_label_ids.contains(&"lbl_fin".to_string()));

    // The catch-all swap should add lbl_oth and remove lbl_fin + lbl_rec
    let catchall_swap = swaps.iter().find(|s| s.query.contains("-subject:(statement)")).unwrap();
    assert_eq!(catchall_swap.add_label_id, "lbl_oth");
    assert!(catchall_swap.remove_label_ids.contains(&"lbl_fin".to_string()));
    assert!(catchall_swap.remove_label_ids.contains(&"lbl_rec".to_string()));
}
```

2. Run test to verify it fails.

3. Implement: In `collect_swaps`, add a branch for `ReplaceWithExclusive` AFTER the existing `KeepOne` branch:

```rust
if let GroupDecision::ReplaceWithExclusive { replacement_filters } = decision {
    // Collect ALL label IDs from the group's original filters
    let all_label_ids: std::collections::HashSet<String> = group
        .filters
        .iter()
        .flat_map(|f| f.add_label_ids.iter().cloned())
        .collect();

    for replacement in replacement_filters {
        let winner = &replacement.target_label_id;

        // Losers = all group labels except the winner
        let loser_labels: Vec<String> = all_label_ids
            .iter()
            .filter(|l| *l != winner)
            .cloned()
            .collect();

        if loser_labels.is_empty() {
            continue;
        }

        // Build the full Gmail query for this specific replacement filter
        let query = crate::filter_manager::FilterManager::build_gmail_query_static(replacement);

        swaps.push(LabelSwap {
            query,
            add_label_id: winner.clone(),
            remove_label_ids: loser_labels,
        });
    }
}
```

Note: This requires importing `crate::filter_manager::FilterManager` at the top of `filter_remediation.rs`.

4. Run test to verify it passes.

5. Run `cargo test --lib filter_remediation -p gmail-automation` — all tests pass.

6. Commit: `feat: extend collect_swaps to produce label cleanup for MechanicalFix groups`

### Task 3: Handle edge case — skip swaps where replacement has same label as all originals

This handles the case where a MechanicalFix group has filters that ALL map to the same label (which would actually be classified as Consolidate, so this may not happen in practice). But defensively, if `loser_labels` is empty because all original filters had the same label, we should skip.

**This is already handled** by the `if loser_labels.is_empty() { continue; }` guard in Task 2. No additional work needed, but add a test to confirm:

**Files:**
- Modify: `src/filter_remediation.rs` — add edge case test

**Steps:**

1. Add test:

```rust
#[test]
fn test_collect_swaps_mechanical_fix_same_label_no_swap() {
    // Edge case: MechanicalFix where all filters happen to have the same label
    // (in practice this would be Consolidate, but test defensively)
    let filters = vec![
        make_filter("f1", Some("example.com"), Some("alert"), "lbl_fin"),
        make_filter("f2", Some("example.com"), None, "lbl_fin"),
    ];

    let replacement = FilterRule {
        id: None,
        name: "test".to_string(),
        from_pattern: Some("example.com".to_string()),
        is_specific_sender: false,
        excluded_senders: vec![],
        subject_keywords: vec![],
        excluded_subject_patterns: vec!["alert".to_string()],
        target_label_id: "lbl_fin".to_string(),
        should_archive: false,
        estimated_matches: 0,
    };

    let group = OverlapGroup {
        group_id: "example.com".to_string(),
        from_pattern: "example.com".to_string(),
        filters,
        label_names: vec!["Financial".to_string()],
        resolution_type: ResolutionType::MechanicalFix {
            proposed_replacements: vec![replacement.clone()],
        },
    };

    let mut plan = RemediationPlan::new();
    plan.add(
        group,
        GroupDecision::ReplaceWithExclusive {
            replacement_filters: vec![replacement],
        },
    );

    let swaps = RemediationApplicator::collect_swaps(&plan);
    assert_eq!(swaps.len(), 0); // No swaps needed — same label everywhere
}
```

2. Run test to verify it passes (should pass immediately since the guard exists).

3. Commit: `test: add edge case test for MechanicalFix with identical labels`

### Task 4: Update CLI display for MechanicalFix swaps

The CLI apply phase currently shows `swap.query` which will now be a long Gmail query string for MechanicalFix swaps. Make the display more readable.

**Files:**
- Modify: `src/main.rs` — update the swap display loop in the Remediate apply phase

**Steps:**

1. In the CLI apply phase (around line 805), update the swap display. Currently shows:
```
  edm.cba.com.au — Receipts, Other → Financial (~42 emails)
```

For MechanicalFix swaps with full queries, show the query truncated:
```
  from:(*@edm.cba.com.au) subject:(statement) — Receipts, Other → Financial (~42 emails)
```

The query itself is self-explanatory. Truncate at 60 chars if too long. Update the display code:

```rust
println!("\nLabel swaps for existing emails:");
for swap in &swaps {
    let count = client.list_message_ids(&swap.query).await.unwrap_or_default().len();
    let remove_names: Vec<&str> = swap.remove_label_ids.iter()
        .map(|id| label_map.get(id).map(|s| s.as_str()).unwrap_or(id))
        .collect();
    let add_name = label_map.get(&swap.add_label_id)
        .map(|s| s.as_str())
        .unwrap_or(&swap.add_label_id);
    let display_query = if swap.query.len() > 60 {
        format!("{}...", &swap.query[..57])
    } else {
        swap.query.clone()
    };
    println!(
        "  {} — {} → {} (~{} emails)",
        display_query,
        remove_names.join(", "),
        add_name,
        count
    );
}
```

2. Run `cargo check` — compiles.

3. Commit: `feat: improve CLI display for MechanicalFix label swaps`

### Task 5: Run full test suite, clippy, and verify

**Steps:**

1. Run `cargo test -p gmail-automation` — all tests pass
2. Run `cargo clippy --all-targets` — no new warnings
3. Run `cd src-tauri && cargo check` — Tauri compiles
4. Commit if any fixes needed
5. Push: `git push origin feature/tauri-gui`
