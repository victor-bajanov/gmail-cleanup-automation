# Fix Label Display and Overlap Detection Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Fix two bugs: (1) label names not shown in remediation view because label cache is cold, (2) overlap detection false positives when filters use `-subject:` exclusions.

**Architecture:** Bug 1 is fixed by preloading label+filter caches async after `initialize_client`. Bug 2 is fixed by adding `subject_exclusions_resolve_overlap()` to the overlap analyzer, mirroring the existing `exclusions_resolve_overlap` pattern for FROM exclusions.

**Tech Stack:** Rust, Tauri, Gmail API client

---

### Task 1: Add `with_subject_exclusion` builder to FilterExpr

**Files:**
- Modify: `src/filter_ast.rs:97-110` (after `with_exclusion`)

**Step 1: Add the builder method**

After the existing `with_exclusion` method (line 110), add:

```rust
/// Adds a negative subject keyword (-subject:(...))
pub fn with_subject_exclusion(mut self, keyword: impl Into<String>) -> Self {
    self.subject_exclusions.push(keyword.into());
    self
}
```

**Step 2: Verify it compiles**

Run: `cargo check -p gmail-automation 2>&1 | tail -5`
Expected: no errors

**Step 3: Commit**

```bash
git add src/filter_ast.rs
git commit -m "feat: add with_subject_exclusion builder to FilterExpr"
```

---

### Task 2: Write failing tests for subject exclusion overlap detection

**Files:**
- Modify: `src/filter_overlap.rs` (test module, after `test_exclusions_resolve_overlap` at line 1145)

**Step 1: Write the failing tests**

Add after the existing `test_exclusions_resolve_overlap` test:

```rust
#[test]
fn test_subject_exclusions_resolve_overlap_exact() {
    // subject:(Y) vs -subject:(Y) with same FROM = mutually exclusive
    let analyzer = FilterOverlapAnalyzer::new();

    let expr_a = FilterExpr::from_sender("noreply@example.com")
        .with_subject_keywords(vec!["Notification".to_string()], SubjectMatchMode::Any);

    let expr_b = FilterExpr::from_sender("noreply@example.com")
        .with_subject_exclusion("Notification");

    let relation = analyzer.analyze_expr_relation(&expr_a, &expr_b);
    assert!(
        matches!(relation, PatternRelation::Disjoint),
        "subject:(X) vs -subject:(X) should be disjoint, got: {:?}",
        relation
    );
}

#[test]
fn test_subject_exclusions_resolve_overlap_symmetric() {
    // Same as above but arguments reversed
    let analyzer = FilterOverlapAnalyzer::new();

    let expr_a = FilterExpr::from_sender("noreply@example.com")
        .with_subject_exclusion("Notification");

    let expr_b = FilterExpr::from_sender("noreply@example.com")
        .with_subject_keywords(vec!["Notification".to_string()], SubjectMatchMode::Any);

    let relation = analyzer.analyze_expr_relation(&expr_a, &expr_b);
    assert!(
        matches!(relation, PatternRelation::Disjoint),
        "-subject:(X) vs subject:(X) should be disjoint, got: {:?}",
        relation
    );
}

#[test]
fn test_subject_exclusions_all_keywords_covered() {
    // subject:(A B) vs -subject:(A) -subject:(B) = disjoint
    // Filter A requires ALL of {A, B}. Filter B excludes A and B.
    // If B excludes A, no email matching "all of {A, B}" can pass B.
    let analyzer = FilterOverlapAnalyzer::new();

    let expr_a = FilterExpr::from_sender("noreply@example.com")
        .with_subject_keywords(
            vec!["Order".to_string(), "Confirmation".to_string()],
            SubjectMatchMode::All,
        );

    let expr_b = FilterExpr::from_sender("noreply@example.com")
        .with_subject_exclusion("Order")
        .with_subject_exclusion("Confirmation");

    let relation = analyzer.analyze_expr_relation(&expr_a, &expr_b);
    assert!(
        matches!(relation, PatternRelation::Disjoint),
        "subject:(A AND B) vs -subject:(A) -subject:(B) should be disjoint, got: {:?}",
        relation
    );
}

#[test]
fn test_subject_exclusions_partial_no_disjoint() {
    // subject:(A B) with match_mode Any vs -subject:(A) = NOT disjoint
    // Filter A matches emails with A OR B. Filter B excludes A but allows B.
    // An email with subject "B" matches both filters.
    let analyzer = FilterOverlapAnalyzer::new();

    let expr_a = FilterExpr::from_sender("noreply@example.com")
        .with_subject_keywords(
            vec!["Order".to_string(), "Shipping".to_string()],
            SubjectMatchMode::Any,
        );

    let expr_b = FilterExpr::from_sender("noreply@example.com")
        .with_subject_exclusion("Order");

    let relation = analyzer.analyze_expr_relation(&expr_a, &expr_b);
    assert!(
        !matches!(relation, PatternRelation::Disjoint),
        "subject:(A OR B) vs -subject:(A) should NOT be disjoint (B still overlaps), got: {:?}",
        relation
    );
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p gmail-automation test_subject_exclusions -- --nocapture 2>&1 | tail -20`
Expected: `test_subject_exclusions_resolve_overlap_exact` and the symmetric/all_keywords tests FAIL; `test_subject_exclusions_partial_no_disjoint` may pass since the current code already doesn't return Disjoint.

**Step 3: Commit**

```bash
git add src/filter_overlap.rs
git commit -m "test: add failing tests for subject exclusion overlap detection"
```

---

### Task 3: Implement `subject_exclusions_resolve_overlap`

**Files:**
- Modify: `src/filter_overlap.rs:544-572` (after `exclusions_resolve_overlap`)

**Step 1: Add the new method**

After `exclusions_resolve_overlap` (line 572), add:

```rust
/// Checks if subject exclusions make two filters mutually exclusive.
///
/// Gmail semantics: `subject:(Y)` requires Y in subject; `-subject:(Y)` requires Y NOT in subject.
/// If one filter requires keywords that the other explicitly excludes, they are disjoint.
pub fn subject_exclusions_resolve_overlap(&self, expr_a: &FilterExpr, expr_b: &FilterExpr) -> bool {
    // Check if B's subject_exclusions exclude A's subject_clause
    if let Some(ref subj_a) = expr_a.subject_clause {
        if !expr_b.subject_exclusions.is_empty() {
            let excluded = self.subject_keywords_excluded(&subj_a, &expr_b.subject_exclusions);
            if excluded {
                return true;
            }
        }
    }

    // Check if A's subject_exclusions exclude B's subject_clause
    if let Some(ref subj_b) = expr_b.subject_clause {
        if !expr_a.subject_exclusions.is_empty() {
            let excluded = self.subject_keywords_excluded(&subj_b, &expr_a.subject_exclusions);
            if excluded {
                return true;
            }
        }
    }

    false
}

/// Returns true if the given subject exclusions make the subject clause unmatchable.
///
/// For SubjectMatchMode::Any: disjoint only if ALL required keywords are excluded
///   (because any single non-excluded keyword could still match)
/// For SubjectMatchMode::All: disjoint if ANY required keyword is excluded
///   (because the email must contain all keywords, and one is forbidden)
fn subject_keywords_excluded(&self, subject: &SubjectClause, exclusions: &[String]) -> bool {
    match subject.match_mode {
        SubjectMatchMode::All => {
            // ALL mode: email must have every keyword. If any keyword is excluded, impossible.
            subject.keywords.iter().any(|kw| {
                exclusions.iter().any(|ex| ex.to_lowercase() == kw.to_lowercase())
            })
        }
        SubjectMatchMode::Any => {
            // ANY mode: email needs just one keyword. Only disjoint if all keywords are excluded.
            subject.keywords.iter().all(|kw| {
                exclusions.iter().any(|ex| ex.to_lowercase() == kw.to_lowercase())
            })
        }
    }
}
```

**Step 2: Wire it into `analyze_expr_relation`**

In `analyze_expr_relation` (line 345-348), change:

```rust
        // Check if exclusions resolve overlap
        if self.exclusions_resolve_overlap(expr_a, expr_b) {
            return PatternRelation::Disjoint;
        }
```

to:

```rust
        // Check if exclusions resolve overlap
        if self.exclusions_resolve_overlap(expr_a, expr_b) {
            return PatternRelation::Disjoint;
        }

        // Check if subject exclusions resolve overlap
        if self.subject_exclusions_resolve_overlap(expr_a, expr_b) {
            return PatternRelation::Disjoint;
        }
```

**Step 3: Run tests to verify they pass**

Run: `cargo test -p gmail-automation test_subject_exclusions -- --nocapture 2>&1 | tail -20`
Expected: All four `test_subject_exclusions_*` tests PASS

**Step 4: Run full overlap test suite to check for regressions**

Run: `cargo test -p gmail-automation filter_overlap -- --nocapture 2>&1 | tail -10`
Expected: All existing tests still pass

**Step 5: Commit**

```bash
git add src/filter_overlap.rs
git commit -m "fix: detect subject exclusion mutual exclusivity in overlap analysis"
```

---

### Task 4: Preload label and filter caches after client initialization

**Files:**
- Modify: `src-tauri/src/commands/auth.rs:166-207` (`initialize_client`)
- Modify: `src-tauri/src/state.rs` (need `AppState` to be `'static` + `Send` compatible — verify)

**Step 1: Add async preload to `initialize_client`**

In `src-tauri/src/commands/auth.rs`, after `state.set_config(config);` (line 204), add a background task that loads labels and filters into the state caches. Since `State<'_, AppState>` doesn't have `'static` lifetime, extract the `Arc` references before spawning:

```rust
    state.set_client(client);
    state.set_config(config);

    // Preload label and filter caches in background
    if let Some(client_arc) = state.get_client() {
        let label_cache = state.label_cache_handle();
        tauri::async_runtime::spawn(async move {
            // Load labels
            match client_arc.list_labels().await {
                Ok(labels) => {
                    let mut cache = label_cache.write();
                    for label in &labels {
                        cache.insert(label.name.to_lowercase(), label.id.clone());
                    }
                    tracing::info!("Preloaded {} labels into cache", labels.len());
                }
                Err(e) => {
                    tracing::warn!("Failed to preload labels: {}", e);
                }
            }
        });
    }

    Ok(true)
```

**Step 2: Add `label_cache_handle` to AppState**

In `src-tauri/src/state.rs`, add a method to expose the `RwLock` handle so the background task can write to it without holding a reference to `AppState`:

```rust
/// Returns a cloneable handle to the label cache for background tasks
pub fn label_cache_handle(&self) -> Arc<RwLock<HashMap<String, String>>> {
    self.label_cache_arc.clone()
}
```

This requires changing `label_cache` from `RwLock<HashMap<String, String>>` to `Arc<RwLock<HashMap<String, String>>>` in the struct definition, and updating all existing reads/writes. Check how `label_cache` is currently declared — if it's already behind an `Arc` (AppState is managed by Tauri which wraps it in `Arc`), you may be able to just clone the inner `RwLock` reference. If `label_cache` is `parking_lot::RwLock`, wrapping in `Arc` is the approach.

Alternatively, if `AppState` is already `Arc`-wrapped by Tauri's `manage()`, you can pass `state.inner().clone()` to the spawn if `AppState` implements `Clone`, or extract just the lock. Check the actual types before implementing.

**The simplest approach:** Since Tauri's `State<'_, AppState>` already wraps `AppState` in an `Arc`, use `state.inner().clone()` pattern if available, or restructure `label_cache` to be `Arc<RwLock<...>>`.

**Step 3: Verify it compiles**

Run: `cargo check -p gmail-cleanup-gui 2>&1 | tail -10`
Expected: no errors

**Step 4: Commit**

```bash
git add src-tauri/src/commands/auth.rs src-tauri/src/state.rs
git commit -m "fix: preload label cache async after client initialization"
```

---

### Task 5: Verify both fixes end-to-end

**Step 1: Run the full test suite**

Run: `cargo test 2>&1 | tail -20`
Expected: All tests pass

**Step 2: Build the Tauri app**

Run: `cd src-tauri && cargo build 2>&1 | tail -10`
Expected: Compiles without errors

**Step 3: Commit any remaining fixes**

If any compilation or test issues arose, fix and commit.

---

### Task 6: Update the bugs TODO file

**Files:**
- Modify: `docs/plans/remediation-ui-todos.md`

**Step 1: Mark both bugs as fixed**

Replace the bugs section content with:

```markdown
## Bugs

- ~~Label names still not shown in remediation view~~ — Fixed: label cache preloaded async on client init
- ~~Overlap detection falsely flags filters with `-subject:` exclusions~~ — Fixed: added `subject_exclusions_resolve_overlap` to analyzer
```

**Step 2: Commit**

```bash
git add docs/plans/remediation-ui-todos.md
git commit -m "docs: mark label display and overlap detection bugs as fixed"
```
