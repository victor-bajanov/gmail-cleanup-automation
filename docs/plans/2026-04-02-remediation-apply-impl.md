# Remediation Apply Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** After remediation filter changes, swap labels on existing emails for PickWinner groups (remove loser labels, add winner label).

**Architecture:** New `LabelSwap`, `ApplyResult` structs and `RemediationApplicator` in `src/filter_remediation.rs`. CLI gets `--no-apply` flag and a second confirmation prompt. Tauri gets two new commands.

**Tech Stack:** Rust, async_trait, crossterm (CLI), Tauri commands, existing `GmailClient::batch_modify_labels` and `list_message_ids`.

---

### Task 1: Add LabelSwap and ApplyResult structs + collect_swaps

**Files:**
- Modify: `src/filter_remediation.rs` (add structs after `RemediationResult`, add `RemediationApplicator` impl)
- Modify: `src/lib.rs:130-132` (add re-exports)

**Step 1: Write the failing test**

Add to the `mod tests` block in `src/filter_remediation.rs`:

```rust
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
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --lib filter_remediation::tests::test_collect_swaps -- -v`
Expected: FAIL — `RemediationApplicator`, `LabelSwap` not found

**Step 3: Write minimal implementation**

Add after `RemediationResult` in `src/filter_remediation.rs`:

```rust
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
    /// Extract label swaps from an executed plan.
    /// Only PickWinner/KeepOne decisions produce swaps.
    pub fn collect_swaps(plan: &RemediationPlan) -> Vec<LabelSwap> {
        let mut swaps = Vec::new();

        for (group, decision) in &plan.groups {
            if let GroupDecision::KeepOne { keep_filter_id } = decision {
                // Find the winner's label
                let winner_label = group
                    .filters
                    .iter()
                    .find(|f| f.id == *keep_filter_id)
                    .and_then(|f| f.add_label_ids.first().cloned());

                let Some(winner) = winner_label else {
                    continue;
                };

                // Collect loser labels (deduplicated, excluding winner)
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
```

Update `src/lib.rs` re-exports to add `ApplyResult, LabelSwap, RemediationApplicator`:

```rust
pub use filter_remediation::{
    ApplyResult, GroupDecision, LabelSwap, OverlapGroup, RemediationApplicator,
    RemediationPlan, RemediationResult, ResolutionType,
};
```

**Step 4: Run tests to verify they pass**

Run: `cargo test --lib filter_remediation::tests::test_collect_swaps`
Expected: 3 tests PASS

**Step 5: Commit**

```bash
git add src/filter_remediation.rs src/lib.rs
git commit -m "feat: add LabelSwap, ApplyResult, and collect_swaps"
```

---

### Task 2: Implement RemediationApplicator::apply

**Files:**
- Modify: `src/filter_remediation.rs` (add `apply` method to `RemediationApplicator`)

**Step 1: Write the failing test**

Add to `mod tests` in `src/filter_remediation.rs`. This needs a new mock client that implements `list_message_ids` and `batch_modify_labels`:

```rust
struct MockApplyClient {
    /// Messages returned by list_message_ids, keyed by query substring
    message_ids: HashMap<String, Vec<String>>,
    /// Track batch_modify_labels calls
    modifications: Arc<Mutex<Vec<(Vec<String>, Vec<String>, Vec<String>)>>>,
}

#[async_trait]
impl crate::client::GmailClient for MockApplyClient {
    async fn list_message_ids(&self, query: &str) -> crate::error::Result<Vec<String>> {
        // Return matching messages for any query containing a known key
        for (pattern, ids) in &self.message_ids {
            if query.contains(pattern) {
                return Ok(ids.clone());
            }
        }
        Ok(vec![])
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
    // All other trait methods: unimplemented!()
    // (Copy the unimplemented stubs from MockExecuteClient)
}
```

Then the test:

```rust
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
    assert_eq!(mods[0].0, vec!["msg1", "msg2", "msg3"]); // message_ids
    assert_eq!(mods[0].1, vec!["lbl_fin"]);                // add
    assert_eq!(mods[0].2, vec!["lbl_rec"]);                // remove
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
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --lib filter_remediation::tests::test_apply`
Expected: FAIL — no method `apply` on `RemediationApplicator`

**Step 3: Write minimal implementation**

Add to `impl RemediationApplicator` in `src/filter_remediation.rs`:

```rust
/// Execute label swaps: for each swap, query Gmail for matching
/// messages then batch_modify_labels to add winner and remove losers.
pub async fn apply(
    client: &dyn GmailClient,
    swaps: &[LabelSwap],
) -> crate::error::Result<ApplyResult> {
    let mut result = ApplyResult::default();

    for swap in swaps {
        // Query for messages from this sender that have any loser label
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
                &[swap.add_label_id.clone()],
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
```

**Step 4: Run tests to verify they pass**

Run: `cargo test --lib filter_remediation::tests::test_apply`
Expected: 2 tests PASS

**Step 5: Commit**

```bash
git add src/filter_remediation.rs
git commit -m "feat: implement RemediationApplicator::apply"
```

---

### Task 3: Add --no-apply flag and CLI apply phase

**Files:**
- Modify: `src/cli.rs:135-139` (add `no_apply` flag to `Remediate` variant)
- Modify: `src/main.rs:786-798` (add apply phase after filter execution)

**Step 1: Add `--no-apply` flag**

In `src/cli.rs`, update the `Remediate` variant:

```rust
    Remediate {
        /// Preview changes without applying them
        #[arg(long)]
        dry_run: bool,
        /// Skip applying label changes to existing emails
        #[arg(long)]
        no_apply: bool,
    },
```

**Step 2: Update the match arm in `src/main.rs`**

In `src/main.rs`, update the `Commands::Remediate` match arm to destructure `no_apply`, then add after the existing result reporting (after line ~796):

```rust
        Commands::Remediate { dry_run, no_apply } => {
            // ... existing code unchanged until after result reporting ...

            // --- Apply phase: swap labels on existing emails ---
            let swaps = gmail_automation::RemediationApplicator::collect_swaps(&plan);

            if swaps.is_empty() {
                println!("\nNo label changes needed for existing emails.");
            } else if no_apply {
                println!("\nSkipping label application (--no-apply).");
                println!("  {} swap(s) would affect existing emails.", swaps.len());
            } else {
                println!("\nLabel swaps for existing emails:");
                for swap in &swaps {
                    let query = format!("from:{}", swap.from_pattern);
                    let count = client.list_message_ids(&query).await.unwrap_or_default().len();
                    let remove_names: Vec<&str> = swap.remove_label_ids.iter()
                        .map(|id| label_map.get(id).map(|s| s.as_str()).unwrap_or(id))
                        .collect();
                    let add_name = label_map.get(&swap.add_label_id)
                        .map(|s| s.as_str())
                        .unwrap_or(&swap.add_label_id);
                    println!(
                        "  {} — {} → {} (~{} emails)",
                        swap.from_pattern,
                        remove_names.join(", "),
                        add_name,
                        count
                    );
                }

                println!("\nApply label changes? [y]es [n]o");
                let confirm_apply = matches!(read_key(), Some('y'));

                if confirm_apply {
                    let apply_result = gmail_automation::RemediationApplicator::apply(
                        &client, &swaps,
                    )
                    .await?;
                    println!("\nLabel application complete:");
                    println!("  Relabeled: {} messages", apply_result.messages_relabeled);
                    if apply_result.messages_failed > 0 {
                        println!("  Failed: {} messages", apply_result.messages_failed);
                    }
                    if !apply_result.errors.is_empty() {
                        for e in &apply_result.errors {
                            println!("    - {}", e);
                        }
                    }
                } else {
                    println!("Skipped label application.");
                }
            }

            Ok(())
        }
```

**Step 3: Verify compilation**

Run: `cargo check`
Expected: compiles successfully

**Step 4: Commit**

```bash
git add src/cli.rs src/main.rs
git commit -m "feat: add --no-apply flag and CLI apply phase for remediation"
```

---

### Task 4: Add Tauri commands for apply phase

**Files:**
- Modify: `src-tauri/src/commands/remediation.rs` (add `collect_swaps` and `apply_swaps` commands)
- Modify: `src-tauri/src/main.rs` (register new commands)

**Step 1: Add Tauri commands**

Append to `src-tauri/src/commands/remediation.rs`:

```rust
use gmail_automation::filter_remediation::{
    ApplyResult, GroupDecision, LabelSwap, OverlapDetector, OverlapGroup,
    RemediationApplicator, RemediationPlan, RemediationResult,
};

#[tauri::command]
pub async fn collect_remediation_swaps(
    state: State<'_, AppState>,
) -> Result<Vec<LabelSwap>, String> {
    let groups = state.remediation_groups.read().clone();
    let decisions = state.remediation_decisions.read().clone();

    let mut plan = RemediationPlan::new();
    for group in groups {
        let decision = decisions
            .get(&group.group_id)
            .cloned()
            .unwrap_or(GroupDecision::Skip);
        plan.add(group, decision);
    }

    Ok(RemediationApplicator::collect_swaps(&plan))
}

#[tauri::command]
pub async fn apply_remediation_swaps(
    state: State<'_, AppState>,
) -> Result<ApplyResult, String> {
    let client = state
        .get_client()
        .ok_or_else(|| "Not authenticated".to_string())?;

    let groups = state.remediation_groups.read().clone();
    let decisions = state.remediation_decisions.read().clone();

    let mut plan = RemediationPlan::new();
    for group in groups {
        let decision = decisions
            .get(&group.group_id)
            .cloned()
            .unwrap_or(GroupDecision::Skip);
        plan.add(group, decision);
    }

    let swaps = RemediationApplicator::collect_swaps(&plan);
    RemediationApplicator::apply(client.as_ref(), &swaps)
        .await
        .map_err(|e| format!("Apply failed: {}", e))
}
```

Update the import block at the top of the file to use the expanded import (replacing the existing one).

**Step 2: Register commands in Tauri**

In `src-tauri/src/main.rs`, add `collect_remediation_swaps` and `apply_remediation_swaps` to the `invoke_handler` list alongside the existing remediation commands.

**Step 3: Verify compilation**

Run: `cd src-tauri && cargo check`
Expected: compiles successfully

**Step 4: Commit**

```bash
git add src-tauri/src/commands/remediation.rs src-tauri/src/main.rs
git commit -m "feat: add Tauri commands for remediation label swaps"
```

---

### Task 5: Run full test suite and clippy

**Step 1: Run all tests**

Run: `cargo test`
Expected: all tests pass (including the new collect_swaps and apply tests)

**Step 2: Run clippy**

Run: `cargo clippy --all-targets`
Expected: no new warnings from our changes

**Step 3: Run Tauri clippy**

Run: `cd src-tauri && cargo clippy --all-targets`
Expected: no new warnings from our changes

**Step 4: Commit if any fixes needed**

```bash
git add -A && git commit -m "fix: address clippy/test issues"
```
