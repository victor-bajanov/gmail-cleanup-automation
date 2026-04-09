# Filter Editor UI — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a top-level "Editor" view for searching, editing (archive toggle + labels), and deleting Gmail filters with a queued dry-run/apply workflow.

**Architecture:** New `src/filter_editor.rs` for pure logic (dry-run diffs, apply orchestration). New `src-tauri/src/commands/editor.rs` for Tauri commands. New `EditorView.tsx` SolidJS component. One new `GmailClient` trait method (`create_filter_from_info`) to recreate filters from raw `ExistingFilterInfo`.

**Tech Stack:** Rust (async, serde), Tauri v2, SolidJS, TailwindCSS

---

### Task 1: Add `create_filter_from_info` to GmailClient trait

We need a way to recreate a filter from its raw `ExistingFilterInfo` (for update = delete + recreate). This is the only trait extension needed.

**Files:**
- Modify: `src/client.rs:161-225` (trait definition)
- Modify: `src/client.rs:797-845` (ProductionGmailClient impl)
- Modify: `src/client.rs:1186-1200` (RateLimitedGmailClient delegation)
- Modify: `tests/remediation_test.rs:32-119` (MockRemediationClient — add stub)

**Step 1: Add trait method**

In `src/client.rs`, add to the `GmailClient` trait (after `update_filter` at line 187):

```rust
    /// Create a filter from raw ExistingFilterInfo (for editor: delete + recreate with modifications)
    async fn create_filter_from_info(&self, info: &ExistingFilterInfo) -> Result<String>;
```

**Step 2: Implement for ProductionGmailClient**

In `src/client.rs`, add after the `update_filter` impl (around line 937):

```rust
    async fn create_filter_from_info(&self, info: &ExistingFilterInfo) -> Result<String> {
        let info = info.clone();
        let _quota_permit = self.quota_limiter.acquire(QuotaCost::Write).await;

        self.with_retry("create_filter_from_info", 3, || async {
            let criteria = FilterCriteria {
                query: info.query.clone(),
                from: info.from.clone(),
                to: info.to.clone(),
                subject: info.subject.clone(),
                exclude_chats: Some(true),
                ..Default::default()
            };

            let action = FilterAction {
                add_label_ids: if info.add_label_ids.is_empty() { None } else { Some(info.add_label_ids.clone()) },
                remove_label_ids: if info.remove_label_ids.is_empty() { None } else { Some(info.remove_label_ids.clone()) },
                ..Default::default()
            };

            let gmail_filter = Filter {
                criteria: Some(criteria),
                action: Some(action),
                ..Default::default()
            };

            let (_, created_filter) = self
                .hub
                .users()
                .settings_filters_create(gmail_filter, "me")
                .add_scope("https://www.googleapis.com/auth/gmail.settings.basic")
                .doit()
                .await?;

            created_filter
                .id
                .ok_or_else(|| GmailError::FilterError("Created filter has no ID".to_string()))
        })
        .await
    }
```

**Step 3: Delegate in RateLimitedGmailClient**

In `src/client.rs`, add after the `update_filter` delegation (around line 1200):

```rust
    async fn create_filter_from_info(&self, info: &ExistingFilterInfo) -> Result<String> {
        self.as_ref().create_filter_from_info(info).await
    }
```

**Step 4: Add stub to MockRemediationClient**

In `tests/remediation_test.rs`, add to the `GmailClient` impl block (after `update_filter`):

```rust
    async fn create_filter_from_info(&self, _info: &ExistingFilterInfo) -> Result<String> {
        unimplemented!()
    }
```

**Step 5: Verify it compiles**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: Compiles with existing warnings only (no new errors)

**Step 6: Commit**

```bash
git add src/client.rs tests/remediation_test.rs
git commit -m "feat: add create_filter_from_info to GmailClient trait"
```

---

### Task 2: Create `filter_editor.rs` with types and dry-run logic (TDD)

Pure logic module — no API calls, fully testable.

**Files:**
- Create: `src/filter_editor.rs`
- Modify: `src/lib.rs:62-80` (add `pub mod filter_editor;`)

**Step 1: Write the failing tests**

Create `src/filter_editor.rs` with types and tests first, empty impls:

```rust
use serde::{Deserialize, Serialize};
use crate::client::ExistingFilterInfo;

// ============ Types ============

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FilterAction {
    UpdateArchive { filter_id: String, new_value: bool },
    UpdateLabels { filter_id: String, add: Vec<String>, remove: Vec<String> },
    Delete { filter_id: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionDiff {
    pub filter_id: String,
    pub description: String,
    pub action_type: String,
    pub changes: Vec<FieldChange>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldChange {
    pub field: String,
    pub before: String,
    pub after: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditorApplyResult {
    pub succeeded: usize,
    pub failed: Vec<(String, String)>,
}

// ============ Dry Run ============

const INBOX_LABEL: &str = "INBOX";

pub fn dry_run(actions: &[FilterAction], filters: &[ExistingFilterInfo]) -> Vec<ActionDiff> {
    todo!()
}

fn find_filter<'a>(filters: &'a [ExistingFilterInfo], id: &str) -> Option<&'a ExistingFilterInfo> {
    filters.iter().find(|f| f.id == id)
}

fn describe_filter(filter: &ExistingFilterInfo) -> String {
    filter.query.clone()
        .or_else(|| filter.from.clone())
        .unwrap_or_else(|| filter.id.clone())
}

fn has_archive(filter: &ExistingFilterInfo) -> bool {
    filter.remove_label_ids.contains(&INBOX_LABEL.to_string())
}

// ============ Tests ============

#[cfg(test)]
mod tests {
    use super::*;

    fn make_filter(id: &str, query: &str, labels: Vec<&str>, archive: bool) -> ExistingFilterInfo {
        ExistingFilterInfo {
            id: id.to_string(),
            query: Some(query.to_string()),
            from: None,
            to: None,
            subject: None,
            add_label_ids: labels.into_iter().map(|s| s.to_string()).collect(),
            remove_label_ids: if archive { vec!["INBOX".to_string()] } else { vec![] },
        }
    }

    #[test]
    fn test_dry_run_archive_toggle_on() {
        let filters = vec![make_filter("f1", "from:(*@github.com)", vec!["lbl_gh"], false)];
        let actions = vec![FilterAction::UpdateArchive {
            filter_id: "f1".into(),
            new_value: true,
        }];

        let diffs = dry_run(&actions, &filters);
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].filter_id, "f1");
        assert_eq!(diffs[0].action_type, "update");
        assert_eq!(diffs[0].changes.len(), 1);
        assert_eq!(diffs[0].changes[0].field, "archive");
        assert_eq!(diffs[0].changes[0].before, "OFF");
        assert_eq!(diffs[0].changes[0].after, "ON");
    }

    #[test]
    fn test_dry_run_archive_toggle_off() {
        let filters = vec![make_filter("f1", "from:(*@github.com)", vec!["lbl_gh"], true)];
        let actions = vec![FilterAction::UpdateArchive {
            filter_id: "f1".into(),
            new_value: false,
        }];

        let diffs = dry_run(&actions, &filters);
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].changes[0].before, "ON");
        assert_eq!(diffs[0].changes[0].after, "OFF");
    }

    #[test]
    fn test_dry_run_label_update() {
        let filters = vec![make_filter("f1", "from:(*@github.com)", vec!["lbl_gh", "lbl_old"], false)];
        let actions = vec![FilterAction::UpdateLabels {
            filter_id: "f1".into(),
            add: vec!["lbl_new".into()],
            remove: vec!["lbl_old".into()],
        }];

        let diffs = dry_run(&actions, &filters);
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].action_type, "update");
        assert_eq!(diffs[0].changes.len(), 1);
        assert_eq!(diffs[0].changes[0].field, "labels");
        assert_eq!(diffs[0].changes[0].before, "lbl_gh, lbl_old");
        assert_eq!(diffs[0].changes[0].after, "lbl_gh, lbl_new");
    }

    #[test]
    fn test_dry_run_delete() {
        let filters = vec![make_filter("f1", "from:(*@github.com)", vec!["lbl_gh"], false)];
        let actions = vec![FilterAction::Delete {
            filter_id: "f1".into(),
        }];

        let diffs = dry_run(&actions, &filters);
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].action_type, "delete");
        assert!(diffs[0].changes.is_empty());
    }

    #[test]
    fn test_dry_run_nonexistent_filter() {
        let filters = vec![make_filter("f1", "from:(*@github.com)", vec!["lbl_gh"], false)];
        let actions = vec![FilterAction::Delete {
            filter_id: "f_nonexistent".into(),
        }];

        let diffs = dry_run(&actions, &filters);
        assert_eq!(diffs.len(), 0); // silently skip missing filters
    }

    #[test]
    fn test_dry_run_empty_queue() {
        let filters = vec![make_filter("f1", "from:(*@github.com)", vec!["lbl_gh"], false)];
        let diffs = dry_run(&[], &filters);
        assert!(diffs.is_empty());
    }

    #[test]
    fn test_dry_run_noop_archive_already_on() {
        let filters = vec![make_filter("f1", "from:(*@github.com)", vec!["lbl_gh"], true)];
        let actions = vec![FilterAction::UpdateArchive {
            filter_id: "f1".into(),
            new_value: true, // already ON
        }];

        let diffs = dry_run(&actions, &filters);
        assert_eq!(diffs.len(), 0); // no-op, skip
    }
}
```

**Step 2: Register the module**

In `src/lib.rs`, add after line 73 (`pub mod filter_remediation;`):

```rust
pub mod filter_editor;
```

And add re-exports at the bottom (after the filter_remediation re-exports):

```rust
// Filter editor types
pub use filter_editor::{ActionDiff, EditorApplyResult, FieldChange, FilterAction};
```

**Step 3: Run tests to verify they fail**

Run: `cargo test --lib filter_editor -- --nocapture`
Expected: FAIL — all tests panic with `not yet implemented`

**Step 4: Implement `dry_run`**

Replace the `todo!()` in `dry_run`:

```rust
pub fn dry_run(actions: &[FilterAction], filters: &[ExistingFilterInfo]) -> Vec<ActionDiff> {
    actions.iter().filter_map(|action| {
        match action {
            FilterAction::UpdateArchive { filter_id, new_value } => {
                let filter = find_filter(filters, filter_id)?;
                let currently_archived = has_archive(filter);
                if currently_archived == *new_value {
                    return None; // no-op
                }
                Some(ActionDiff {
                    filter_id: filter_id.clone(),
                    description: describe_filter(filter),
                    action_type: "update".into(),
                    changes: vec![FieldChange {
                        field: "archive".into(),
                        before: if currently_archived { "ON" } else { "OFF" }.into(),
                        after: if *new_value { "ON" } else { "OFF" }.into(),
                    }],
                })
            }
            FilterAction::UpdateLabels { filter_id, add, remove } => {
                let filter = find_filter(filters, filter_id)?;
                let before_labels = filter.add_label_ids.clone();
                let mut after_labels: Vec<String> = before_labels
                    .iter()
                    .filter(|l| !remove.contains(l))
                    .cloned()
                    .collect();
                for label in add {
                    if !after_labels.contains(label) {
                        after_labels.push(label.clone());
                    }
                }
                if before_labels == after_labels {
                    return None; // no-op
                }
                Some(ActionDiff {
                    filter_id: filter_id.clone(),
                    description: describe_filter(filter),
                    action_type: "update".into(),
                    changes: vec![FieldChange {
                        field: "labels".into(),
                        before: before_labels.join(", "),
                        after: after_labels.join(", "),
                    }],
                })
            }
            FilterAction::Delete { filter_id } => {
                let filter = find_filter(filters, filter_id)?;
                Some(ActionDiff {
                    filter_id: filter_id.clone(),
                    description: describe_filter(filter),
                    action_type: "delete".into(),
                    changes: vec![],
                })
            }
        }
    }).collect()
}
```

**Step 5: Run tests to verify they pass**

Run: `cargo test --lib filter_editor -- --nocapture`
Expected: All 7 tests PASS

**Step 6: Commit**

```bash
git add src/filter_editor.rs src/lib.rs
git commit -m "feat: add filter_editor module with dry_run logic and tests"
```

---

### Task 3: Add `apply` function to `filter_editor.rs` (TDD)

The apply function orchestrates actual Gmail API calls.

**Files:**
- Modify: `src/filter_editor.rs` (add `apply` function)
- Create: `tests/editor_test.rs` (integration test with mock client)

**Step 1: Write the integration test**

Create `tests/editor_test.rs`:

```rust
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use gmail_automation::client::{ExistingFilterInfo, GmailClient, LabelInfo, ProgressCallback};
use gmail_automation::error::Result;
use gmail_automation::filter_editor::{EditorApplyResult, FilterAction};
use gmail_automation::models::FilterRule;
use gmail_automation::rate_limiter::QuotaStats;

struct MockEditorClient {
    initial_filters: Vec<ExistingFilterInfo>,
    deleted: Arc<Mutex<Vec<String>>>,
    created_from_info: Arc<Mutex<Vec<ExistingFilterInfo>>>,
    fail_on_filter_id: Option<String>,
}

impl MockEditorClient {
    fn new(filters: Vec<ExistingFilterInfo>) -> Self {
        Self {
            initial_filters: filters,
            deleted: Arc::new(Mutex::new(vec![])),
            created_from_info: Arc::new(Mutex::new(vec![])),
            fail_on_filter_id: None,
        }
    }

    fn with_failure(mut self, filter_id: &str) -> Self {
        self.fail_on_filter_id = Some(filter_id.to_string());
        self
    }
}

#[async_trait]
impl GmailClient for MockEditorClient {
    async fn list_filters(&self) -> Result<Vec<ExistingFilterInfo>> {
        Ok(self.initial_filters.clone())
    }

    async fn delete_filter(&self, filter_id: &str) -> Result<()> {
        if self.fail_on_filter_id.as_deref() == Some(filter_id) {
            return Err(gmail_automation::GmailError::FilterError(
                format!("Simulated failure for {}", filter_id),
            ));
        }
        self.deleted.lock().unwrap().push(filter_id.to_string());
        Ok(())
    }

    async fn create_filter_from_info(&self, info: &ExistingFilterInfo) -> Result<String> {
        let mut created = self.created_from_info.lock().unwrap();
        let id = format!("f_new_{}", created.len() + 1);
        created.push(info.clone());
        Ok(id)
    }

    // Stubs for unused trait methods
    async fn create_filter(&self, _filter: &FilterRule) -> Result<String> { unimplemented!() }
    async fn update_filter(&self, _id: &str, _filter: &FilterRule) -> Result<String> { unimplemented!() }
    async fn list_message_ids(&self, _query: &str) -> Result<Vec<String>> { unimplemented!() }
    async fn get_message(&self, _id: &str) -> Result<gmail_automation::models::MessageMetadata> { unimplemented!() }
    async fn list_labels(&self) -> Result<Vec<LabelInfo>> { unimplemented!() }
    async fn create_label(&self, _name: &str) -> Result<String> { unimplemented!() }
    async fn delete_label(&self, _label_id: &str) -> Result<()> { unimplemented!() }
    async fn apply_label(&self, _msg: &str, _lbl: &str) -> Result<()> { unimplemented!() }
    async fn remove_label(&self, _msg: &str, _lbl: &str) -> Result<()> { unimplemented!() }
    async fn batch_remove_label(&self, _ids: &[String], _lbl: &str) -> Result<usize> { unimplemented!() }
    async fn batch_add_label(&self, _ids: &[String], _lbl: &str) -> Result<usize> { unimplemented!() }
    async fn batch_modify_labels(&self, _ids: &[String], _add: &[String], _rm: &[String]) -> Result<usize> { unimplemented!() }
    async fn fetch_messages_batch(&self, _ids: Vec<String>) -> Result<Vec<gmail_automation::models::MessageMetadata>> { unimplemented!() }
    async fn fetch_messages_with_progress(&self, _ids: Vec<String>, _cb: ProgressCallback) -> Result<Vec<gmail_automation::models::MessageMetadata>> { unimplemented!() }
    async fn quota_stats(&self) -> QuotaStats { unimplemented!() }
}

fn make_filter(id: &str, query: &str, labels: Vec<&str>, archive: bool) -> ExistingFilterInfo {
    ExistingFilterInfo {
        id: id.to_string(),
        query: Some(query.to_string()),
        from: None,
        to: None,
        subject: None,
        add_label_ids: labels.into_iter().map(|s| s.to_string()).collect(),
        remove_label_ids: if archive { vec!["INBOX".to_string()] } else { vec![] },
    }
}

#[tokio::test]
async fn test_apply_archive_toggle() {
    let filters = vec![make_filter("f1", "from:(*@github.com)", vec!["lbl_gh"], false)];
    let client = MockEditorClient::new(filters.clone());
    let actions = vec![FilterAction::UpdateArchive {
        filter_id: "f1".into(),
        new_value: true,
    }];

    let result = gmail_automation::filter_editor::apply(&client, &actions, &filters).await;
    assert_eq!(result.succeeded, 1);
    assert!(result.failed.is_empty());

    let deleted = client.deleted.lock().unwrap();
    assert_eq!(deleted.len(), 1);
    assert_eq!(deleted[0], "f1");

    let created = client.created_from_info.lock().unwrap();
    assert_eq!(created.len(), 1);
    assert!(created[0].remove_label_ids.contains(&"INBOX".to_string()));
}

#[tokio::test]
async fn test_apply_label_update() {
    let filters = vec![make_filter("f1", "from:(*@github.com)", vec!["lbl_gh", "lbl_old"], false)];
    let client = MockEditorClient::new(filters.clone());
    let actions = vec![FilterAction::UpdateLabels {
        filter_id: "f1".into(),
        add: vec!["lbl_new".into()],
        remove: vec!["lbl_old".into()],
    }];

    let result = gmail_automation::filter_editor::apply(&client, &actions, &filters).await;
    assert_eq!(result.succeeded, 1);

    let created = client.created_from_info.lock().unwrap();
    assert_eq!(created[0].add_label_ids, vec!["lbl_gh".to_string(), "lbl_new".to_string()]);
}

#[tokio::test]
async fn test_apply_delete() {
    let filters = vec![make_filter("f1", "from:(*@github.com)", vec!["lbl_gh"], false)];
    let client = MockEditorClient::new(filters.clone());
    let actions = vec![FilterAction::Delete {
        filter_id: "f1".into(),
    }];

    let result = gmail_automation::filter_editor::apply(&client, &actions, &filters).await;
    assert_eq!(result.succeeded, 1);

    let deleted = client.deleted.lock().unwrap();
    assert_eq!(deleted.len(), 1);

    let created = client.created_from_info.lock().unwrap();
    assert!(created.is_empty()); // delete only, no recreate
}

#[tokio::test]
async fn test_apply_partial_failure() {
    let filters = vec![
        make_filter("f1", "from:(*@github.com)", vec!["lbl_gh"], false),
        make_filter("f2", "from:(*@slack.com)", vec!["lbl_sl"], false),
    ];
    let client = MockEditorClient::new(filters.clone()).with_failure("f1");
    let actions = vec![
        FilterAction::Delete { filter_id: "f1".into() },
        FilterAction::Delete { filter_id: "f2".into() },
    ];

    let result = gmail_automation::filter_editor::apply(&client, &actions, &filters).await;
    assert_eq!(result.succeeded, 1);
    assert_eq!(result.failed.len(), 1);
    assert_eq!(result.failed[0].0, "f1");
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --test editor_test -- --nocapture`
Expected: FAIL — `apply` function doesn't exist yet

**Step 3: Implement `apply`**

Add to `src/filter_editor.rs`:

```rust
use crate::client::GmailClient;

pub async fn apply(
    client: &dyn GmailClient,
    actions: &[FilterAction],
    filters: &[ExistingFilterInfo],
) -> EditorApplyResult {
    let mut succeeded = 0;
    let mut failed: Vec<(String, String)> = Vec::new();

    for action in actions {
        let result = apply_single(client, action, filters).await;
        match result {
            Ok(()) => succeeded += 1,
            Err(e) => {
                let filter_id = match action {
                    FilterAction::UpdateArchive { filter_id, .. } => filter_id,
                    FilterAction::UpdateLabels { filter_id, .. } => filter_id,
                    FilterAction::Delete { filter_id } => filter_id,
                };
                failed.push((filter_id.clone(), e));
            }
        }
    }

    EditorApplyResult { succeeded, failed }
}

async fn apply_single(
    client: &dyn GmailClient,
    action: &FilterAction,
    filters: &[ExistingFilterInfo],
) -> std::result::Result<(), String> {
    match action {
        FilterAction::UpdateArchive { filter_id, new_value } => {
            let filter = find_filter(filters, filter_id)
                .ok_or_else(|| format!("Filter {} not found", filter_id))?;
            let mut updated = filter.clone();
            if *new_value {
                if !updated.remove_label_ids.contains(&INBOX_LABEL.to_string()) {
                    updated.remove_label_ids.push(INBOX_LABEL.to_string());
                }
            } else {
                updated.remove_label_ids.retain(|l| l != INBOX_LABEL);
            }
            client.delete_filter(filter_id).await.map_err(|e| e.to_string())?;
            client.create_filter_from_info(&updated).await.map_err(|e| e.to_string())?;
            Ok(())
        }
        FilterAction::UpdateLabels { filter_id, add, remove } => {
            let filter = find_filter(filters, filter_id)
                .ok_or_else(|| format!("Filter {} not found", filter_id))?;
            let mut updated = filter.clone();
            updated.add_label_ids.retain(|l| !remove.contains(l));
            for label in add {
                if !updated.add_label_ids.contains(label) {
                    updated.add_label_ids.push(label.clone());
                }
            }
            client.delete_filter(filter_id).await.map_err(|e| e.to_string())?;
            client.create_filter_from_info(&updated).await.map_err(|e| e.to_string())?;
            Ok(())
        }
        FilterAction::Delete { filter_id } => {
            client.delete_filter(filter_id).await.map_err(|e| e.to_string())?;
            Ok(())
        }
    }
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test --test editor_test -- --nocapture`
Expected: All 4 tests PASS

**Step 5: Commit**

```bash
git add src/filter_editor.rs tests/editor_test.rs
git commit -m "feat: add filter editor apply function with integration tests"
```

---

### Task 4: Add Tauri commands for editor

Wire up the backend to the frontend via Tauri commands.

**Files:**
- Create: `src-tauri/src/commands/editor.rs`
- Modify: `src-tauri/src/commands/mod.rs` (add `pub mod editor;` + `pub use editor::*;`)
- Modify: `src-tauri/src/main.rs:41-98` (register new commands)

**Step 1: Create the editor commands module**

Create `src-tauri/src/commands/editor.rs`:

```rust
//! Filter editor commands — search, dry-run, and apply filter modifications

use crate::state::AppState;
use gmail_automation::client::ExistingFilterInfo;
use gmail_automation::filter_editor::{self, ActionDiff, EditorApplyResult, FilterAction};
use tauri::State;

/// Dry-run: preview what changes would be made without touching Gmail
#[tauri::command]
pub async fn editor_dry_run(
    actions: Vec<FilterAction>,
    state: State<'_, AppState>,
) -> Result<Vec<ActionDiff>, String> {
    let filters = state.get_existing_filters();
    Ok(filter_editor::dry_run(&actions, &filters))
}

/// Apply queued actions to Gmail filters
#[tauri::command]
pub async fn editor_apply(
    actions: Vec<FilterAction>,
    state: State<'_, AppState>,
) -> Result<EditorApplyResult, String> {
    let client = state
        .get_client()
        .ok_or_else(|| "Not authenticated".to_string())?;
    let filters = state.get_existing_filters();

    let result = filter_editor::apply(client.as_ref(), &actions, &filters).await;
    Ok(result)
}
```

**Step 2: Check that `AppState` has `get_existing_filters`**

Look at `src-tauri/src/state.rs` for the method. If it doesn't exist, add it:

```rust
    pub fn get_existing_filters(&self) -> Vec<ExistingFilterInfo> {
        self.existing_filters.read().clone()
    }
```

**Step 3: Register the module and commands**

In `src-tauri/src/commands/mod.rs`, add:
- `pub mod editor;` after the other module declarations
- `pub use editor::*;` after the other re-exports

In `src-tauri/src/main.rs`, add in the `generate_handler!` macro (after the remediation commands block):

```rust
            // Editor commands
            commands::editor_dry_run,
            commands::editor_apply,
```

**Step 4: Verify it compiles**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: Compiles successfully

**Step 5: Commit**

```bash
git add src-tauri/src/commands/editor.rs src-tauri/src/commands/mod.rs src-tauri/src/main.rs src-tauri/src/state.rs
git commit -m "feat: add Tauri commands for filter editor dry-run and apply"
```

---

### Task 5: Add TypeScript types and API functions

Frontend plumbing — types and Tauri invoke wrappers.

**Files:**
- Modify: `ui/src/types/index.ts` (add editor types)
- Modify: `ui/src/lib/api.ts` (add editor API functions)

**Step 1: Add TypeScript types**

In `ui/src/types/index.ts`, add before the `AppView` type:

```typescript
// Editor types
export type FilterAction =
  | { type: 'update_archive'; filter_id: string; new_value: boolean }
  | { type: 'update_labels'; filter_id: string; add: string[]; remove: string[] }
  | { type: 'delete'; filter_id: string };

export interface ActionDiff {
  filter_id: string;
  description: string;
  action_type: string;
  changes: FieldChange[];
}

export interface FieldChange {
  field: string;
  before: string;
  after: string;
}

export interface EditorApplyResult {
  succeeded: number;
  failed: [string, string][];
}

export interface EditorFilter {
  id: string;
  query: string | null;
  from: string | null;
  to: string | null;
  subject: string | null;
  add_label_ids: string[];
  remove_label_ids: string[];
}
```

**Step 2: Update `AppView` type**

Change the `AppView` type to include `'editor'`:

```typescript
export type AppView = 'auth' | 'scan' | 'review' | 'filters' | 'coverage' | 'editor' | 'settings';
```

**Step 3: Add API functions**

In `ui/src/lib/api.ts`, add a new section before the Settings section:

```typescript
// ============ Editor Commands ============

export async function editorDryRun(actions: FilterAction[]): Promise<ActionDiff[]> {
  return invoke<ActionDiff[]>('editor_dry_run', { actions });
}

export async function editorApply(actions: FilterAction[]): Promise<EditorApplyResult> {
  return invoke<EditorApplyResult>('editor_apply', { actions });
}
```

Add the new types to the import:

```typescript
import type {
  // ... existing imports ...
  FilterAction,
  ActionDiff,
  EditorApplyResult,
} from '../types';
```

**Step 4: Verify frontend compiles**

Run: `cd ui && npx tsc --noEmit`
Expected: No errors (or only pre-existing ones)

**Step 5: Commit**

```bash
git add ui/src/types/index.ts ui/src/lib/api.ts
git commit -m "feat: add TypeScript types and API functions for filter editor"
```

---

### Task 6: Add editor store and wire up navigation

Add the SolidJS store for editor state and add "Editor" to the sidebar nav.

**Files:**
- Modify: `ui/src/stores/app.ts` (add editor store)
- Modify: `ui/src/components/layout/Sidebar.tsx` (add Editor nav item)
- Modify: `ui/src/App.tsx` (add EditorView route)

**Step 1: Add editor store**

In `ui/src/stores/app.ts`, add after the filter state section (after line 109):

```typescript
// ============ Editor State ============

const [editorFilters, setEditorFilters] = createSignal<EditorFilter[]>([]);
const [editorSearch, setEditorSearch] = createSignal('');
const [editorQueue, setEditorQueue] = createSignal<FilterAction[]>([]);
const [editorLoading, setEditorLoading] = createSignal(false);

export const editor = {
  filters: editorFilters,
  search: editorSearch,
  queue: editorQueue,
  isLoading: editorLoading,
  setFilters: setEditorFilters,
  setSearch: setEditorSearch,
  setQueue: setEditorQueue,
  setLoading: setEditorLoading,
  queueCount: createMemo(() => editorQueue().length),
  addAction: (action: FilterAction) => {
    setEditorQueue((prev) => {
      // Replace existing action for same filter_id + same type, or append
      const filterId = action.filter_id;
      const filtered = prev.filter((a) => a.filter_id !== filterId);
      return [...filtered, action];
    });
  },
  removeAction: (filterId: string) => {
    setEditorQueue((prev) => prev.filter((a) => a.filter_id !== filterId));
  },
  clearQueue: () => setEditorQueue([]),
  reset: () => {
    setEditorFilters([]);
    setEditorSearch('');
    setEditorQueue([]);
    setEditorLoading(false);
  },
};
```

Add the new types to the import at the top:

```typescript
import type {
  // ... existing imports ...
  EditorFilter,
  FilterAction,
} from '../types';
```

Also add `editor.reset()` to the `resetAllState` function.

**Step 2: Add Editor to sidebar**

In `ui/src/components/layout/Sidebar.tsx`, add to the `navItems` array (after coverage):

```typescript
  { id: 'editor', label: 'Editor', icon: '✏️' },
```

**Step 3: Add EditorView route placeholder**

In `ui/src/App.tsx`, add import:

```typescript
import EditorView from './components/views/EditorView';
```

Add route match (after the coverage match):

```tsx
            <Match when={navigation.currentView() === 'editor'}>
              <EditorView />
            </Match>
```

**Step 4: Create placeholder EditorView**

Create `ui/src/components/views/EditorView.tsx`:

```tsx
import { Component } from 'solid-js';

const EditorView: Component = () => {
  return (
    <div class="space-y-6">
      <h2 class="text-2xl font-bold text-gray-900 dark:text-white">
        Filter Editor
      </h2>
      <p class="text-gray-600 dark:text-gray-400">
        Search, edit, and delete Gmail filters. Coming soon.
      </p>
    </div>
  );
};

export default EditorView;
```

**Step 5: Verify it compiles and renders**

Run: `cd ui && npx tsc --noEmit`
Expected: No errors

**Step 6: Commit**

```bash
git add ui/src/stores/app.ts ui/src/components/layout/Sidebar.tsx ui/src/App.tsx ui/src/components/views/EditorView.tsx
git commit -m "feat: add editor store, navigation, and placeholder view"
```

---

### Task 7: Build the EditorView UI

The main feature UI — search, filter list, inline editing, action queue, dry-run modal, and apply.

**Files:**
- Modify: `ui/src/components/views/EditorView.tsx` (full implementation)

**Step 1: Implement the full EditorView**

Replace `ui/src/components/views/EditorView.tsx` with the full implementation. This is a large component — key sections:

```tsx
import { Component, createMemo, createSignal, For, Show } from 'solid-js';
import { editor } from '../../stores/app';
import * as api from '../../lib/api';
import type { EditorFilter, FilterAction, ActionDiff } from '../../types';

const EditorView: Component = () => {
  const [expandedId, setExpandedId] = createSignal<string | null>(null);
  const [dryRunResults, setDryRunResults] = createSignal<ActionDiff[] | null>(null);
  const [showDryRun, setShowDryRun] = createSignal(false);
  const [isApplying, setIsApplying] = createSignal(false);
  const [applyMessage, setApplyMessage] = createSignal<string | null>(null);

  // Load filters on mount
  const loadFilters = async () => {
    editor.setLoading(true);
    try {
      const filters = await api.getExistingFilters();
      // Convert FilterView[] to EditorFilter[]
      const editorFilters: EditorFilter[] = filters.map((f) => ({
        id: f.id ?? '',
        query: f.query || null,
        from: null,
        to: null,
        subject: null,
        add_label_ids: f.label ? [f.label] : [],
        remove_label_ids: f.archive ? ['INBOX'] : [],
      }));
      editor.setFilters(editorFilters);
    } catch (e) {
      console.error('Failed to load filters:', e);
    } finally {
      editor.setLoading(false);
    }
  };

  // Load on first render if empty
  if (editor.filters().length === 0) {
    loadFilters();
  }

  // Filtered list based on search
  const filteredFilters = createMemo(() => {
    const search = editor.search().toLowerCase();
    if (!search) return editor.filters();
    return editor.filters().filter((f) => {
      const fields = [f.query, f.from, f.to, f.subject, ...f.add_label_ids].filter(Boolean);
      return fields.some((field) => field!.toLowerCase().includes(search));
    });
  });

  // Check if a filter has a pending action
  const getQueuedAction = (filterId: string): FilterAction | undefined => {
    return editor.queue().find((a) => a.filter_id === filterId);
  };

  const hasArchive = (f: EditorFilter) => f.remove_label_ids.includes('INBOX');

  const toggleArchive = (f: EditorFilter) => {
    const currentlyArchived = hasArchive(f);
    editor.addAction({
      type: 'update_archive',
      filter_id: f.id,
      new_value: !currentlyArchived,
    });
  };

  const queueDelete = (f: EditorFilter) => {
    editor.addAction({
      type: 'delete',
      filter_id: f.id,
    });
  };

  const handleDryRun = async () => {
    try {
      const results = await api.editorDryRun(editor.queue());
      setDryRunResults(results);
      setShowDryRun(true);
    } catch (e) {
      console.error('Dry run failed:', e);
    }
  };

  const handleApply = async () => {
    setIsApplying(true);
    setApplyMessage(null);
    try {
      const result = await api.editorApply(editor.queue());
      const msg = result.failed.length > 0
        ? `${result.succeeded} succeeded, ${result.failed.length} failed`
        : `${result.succeeded} changes applied successfully`;
      setApplyMessage(msg);
      editor.clearQueue();
      await loadFilters(); // refresh
    } catch (e) {
      setApplyMessage(`Apply failed: ${e}`);
    } finally {
      setIsApplying(false);
    }
  };

  return (
    <div class="flex gap-6 h-full">
      {/* Main panel: search + filter list */}
      <div class="flex-1 flex flex-col min-w-0">
        <div class="flex items-center justify-between mb-4">
          <h2 class="text-2xl font-bold text-gray-900 dark:text-white">Filter Editor</h2>
          <button
            onClick={loadFilters}
            disabled={editor.isLoading()}
            class="px-3 py-1.5 text-sm bg-gray-100 dark:bg-gray-700 rounded-lg hover:bg-gray-200 dark:hover:bg-gray-600 disabled:opacity-50"
          >
            {editor.isLoading() ? 'Loading...' : 'Refresh'}
          </button>
        </div>

        {/* Search */}
        <input
          type="text"
          placeholder="Search filters (from, subject, labels, query...)"
          value={editor.search()}
          onInput={(e) => editor.setSearch(e.currentTarget.value)}
          class="w-full px-4 py-2 mb-4 border rounded-lg bg-white dark:bg-gray-800 border-gray-300 dark:border-gray-600 text-gray-900 dark:text-white placeholder-gray-400 focus:ring-2 focus:ring-primary-500 focus:border-transparent"
        />

        {/* Filter count */}
        <p class="text-sm text-gray-500 dark:text-gray-400 mb-2">
          {filteredFilters().length} of {editor.filters().length} filters
        </p>

        {/* Filter list */}
        <div class="flex-1 overflow-auto space-y-1">
          <Show when={!editor.isLoading()} fallback={<p class="text-gray-500">Loading filters...</p>}>
            <For each={filteredFilters()}>
              {(filter) => {
                const queued = () => getQueuedAction(filter.id);
                const isExpanded = () => expandedId() === filter.id;
                const isQueuedDelete = () => queued()?.type === 'delete';

                return (
                  <div
                    class="border rounded-lg transition-colors"
                    classList={{
                      'border-red-300 dark:border-red-700 bg-red-50 dark:bg-red-900/20': isQueuedDelete(),
                      'border-orange-300 dark:border-orange-700 bg-orange-50 dark:bg-orange-900/20': !!queued() && !isQueuedDelete(),
                      'border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-800': !queued(),
                    }}
                  >
                    {/* Filter row header */}
                    <button
                      onClick={() => setExpandedId(isExpanded() ? null : filter.id)}
                      class="w-full flex items-center gap-3 px-4 py-3 text-left"
                    >
                      <span class="flex-1 text-sm font-mono text-gray-900 dark:text-white truncate">
                        {filter.query || filter.from || filter.id}
                      </span>
                      <Show when={filter.add_label_ids.length > 0}>
                        <span class="text-xs px-2 py-0.5 rounded bg-blue-100 dark:bg-blue-900 text-blue-700 dark:text-blue-300">
                          {filter.add_label_ids.join(', ')}
                        </span>
                      </Show>
                      <span
                        class="text-xs px-2 py-0.5 rounded"
                        classList={{
                          'bg-green-100 dark:bg-green-900 text-green-700 dark:text-green-300': hasArchive(filter),
                          'bg-gray-100 dark:bg-gray-700 text-gray-500 dark:text-gray-400': !hasArchive(filter),
                        }}
                      >
                        {hasArchive(filter) ? 'Archive' : 'Inbox'}
                      </span>
                      <Show when={queued()}>
                        <span class="text-xs px-2 py-0.5 rounded bg-yellow-100 dark:bg-yellow-900 text-yellow-700 dark:text-yellow-300">
                          Queued
                        </span>
                      </Show>
                    </button>

                    {/* Expanded edit panel */}
                    <Show when={isExpanded()}>
                      <div class="px-4 pb-3 border-t border-gray-200 dark:border-gray-700 pt-3 space-y-3">
                        {/* Filter details */}
                        <div class="grid grid-cols-2 gap-2 text-sm">
                          <Show when={filter.from}><div><span class="text-gray-500">From:</span> {filter.from}</div></Show>
                          <Show when={filter.to}><div><span class="text-gray-500">To:</span> {filter.to}</div></Show>
                          <Show when={filter.subject}><div><span class="text-gray-500">Subject:</span> {filter.subject}</div></Show>
                          <Show when={filter.query}><div class="col-span-2"><span class="text-gray-500">Query:</span> <span class="font-mono">{filter.query}</span></div></Show>
                        </div>

                        {/* Action buttons */}
                        <div class="flex items-center gap-3">
                          <button
                            onClick={() => toggleArchive(filter)}
                            class="px-3 py-1.5 text-sm rounded-lg bg-primary-100 dark:bg-primary-900 text-primary-700 dark:text-primary-300 hover:bg-primary-200 dark:hover:bg-primary-800"
                          >
                            {hasArchive(filter) ? 'Disable Archive' : 'Enable Archive'}
                          </button>
                          <button
                            onClick={() => queueDelete(filter)}
                            class="px-3 py-1.5 text-sm rounded-lg bg-red-100 dark:bg-red-900 text-red-700 dark:text-red-300 hover:bg-red-200 dark:hover:bg-red-800"
                          >
                            Delete
                          </button>
                          <Show when={queued()}>
                            <button
                              onClick={() => editor.removeAction(filter.id)}
                              class="px-3 py-1.5 text-sm rounded-lg bg-gray-100 dark:bg-gray-700 text-gray-600 dark:text-gray-300 hover:bg-gray-200 dark:hover:bg-gray-600"
                            >
                              Undo
                            </button>
                          </Show>
                        </div>
                      </div>
                    </Show>
                  </div>
                );
              }}
            </For>
          </Show>
        </div>
      </div>

      {/* Right sidebar: action queue */}
      <div class="w-80 flex flex-col border-l border-gray-200 dark:border-gray-700 pl-6">
        <div class="flex items-center justify-between mb-4">
          <h3 class="text-lg font-semibold text-gray-900 dark:text-white">
            Actions
            <Show when={editor.queueCount() > 0}>
              <span class="ml-2 text-sm font-normal px-2 py-0.5 rounded-full bg-primary-100 dark:bg-primary-900 text-primary-700 dark:text-primary-300">
                {editor.queueCount()}
              </span>
            </Show>
          </h3>
          <Show when={editor.queueCount() > 0}>
            <button
              onClick={() => editor.clearQueue()}
              class="text-sm text-gray-500 hover:text-gray-700 dark:hover:text-gray-300"
            >
              Clear All
            </button>
          </Show>
        </div>

        {/* Queue list */}
        <div class="flex-1 overflow-auto space-y-2 mb-4">
          <Show when={editor.queueCount() === 0}>
            <p class="text-sm text-gray-400 dark:text-gray-500">No actions queued. Click a filter to edit it.</p>
          </Show>
          <For each={editor.queue()}>
            {(action) => {
              const label = () => {
                switch (action.type) {
                  case 'update_archive': return `${action.new_value ? 'Enable' : 'Disable'} archive`;
                  case 'update_labels': return `Update labels`;
                  case 'delete': return 'Delete';
                }
              };
              return (
                <div class="flex items-center justify-between p-2 rounded-lg bg-gray-50 dark:bg-gray-800 text-sm">
                  <div>
                    <div class="font-medium text-gray-900 dark:text-white">{label()}</div>
                    <div class="text-xs text-gray-500 truncate max-w-[200px]">{action.filter_id}</div>
                  </div>
                  <button
                    onClick={() => editor.removeAction(action.filter_id)}
                    class="text-gray-400 hover:text-red-500"
                  >
                    x
                  </button>
                </div>
              );
            }}
          </For>
        </div>

        {/* Action buttons */}
        <div class="space-y-2">
          <Show when={applyMessage()}>
            <div class="p-2 text-sm rounded-lg bg-green-50 dark:bg-green-900/30 text-green-700 dark:text-green-300">
              {applyMessage()}
            </div>
          </Show>
          <button
            onClick={handleDryRun}
            disabled={editor.queueCount() === 0}
            class="w-full px-4 py-2 text-sm font-medium rounded-lg bg-gray-200 dark:bg-gray-700 text-gray-700 dark:text-gray-300 hover:bg-gray-300 dark:hover:bg-gray-600 disabled:opacity-50 disabled:cursor-not-allowed"
          >
            Dry Run
          </button>
          <button
            onClick={handleApply}
            disabled={editor.queueCount() === 0 || isApplying()}
            class="w-full px-4 py-2 text-sm font-medium rounded-lg bg-primary-600 text-white hover:bg-primary-700 disabled:opacity-50 disabled:cursor-not-allowed"
          >
            {isApplying() ? 'Applying...' : `Apply ${editor.queueCount()} Changes`}
          </button>
        </div>
      </div>

      {/* Dry Run Modal */}
      <Show when={showDryRun()}>
        <div class="fixed inset-0 bg-black/50 flex items-center justify-center z-50" onClick={() => setShowDryRun(false)}>
          <div class="bg-white dark:bg-gray-800 rounded-xl shadow-xl max-w-lg w-full mx-4 max-h-[80vh] overflow-auto" onClick={(e) => e.stopPropagation()}>
            <div class="p-4 border-b border-gray-200 dark:border-gray-700 flex items-center justify-between">
              <h3 class="text-lg font-semibold text-gray-900 dark:text-white">Dry Run Preview</h3>
              <button onClick={() => setShowDryRun(false)} class="text-gray-400 hover:text-gray-600">&times;</button>
            </div>
            <div class="p-4 space-y-4">
              <Show when={dryRunResults()?.length === 0}>
                <p class="text-gray-500">No changes to apply (all actions are no-ops).</p>
              </Show>
              <For each={dryRunResults() ?? []}>
                {(diff) => (
                  <div class="border rounded-lg p-3 border-gray-200 dark:border-gray-700">
                    <div class="flex items-center gap-2 mb-2">
                      <span class="text-xs px-2 py-0.5 rounded"
                        classList={{
                          'bg-blue-100 text-blue-700': diff.action_type === 'update',
                          'bg-red-100 text-red-700': diff.action_type === 'delete',
                        }}
                      >
                        {diff.action_type}
                      </span>
                      <span class="text-sm font-mono text-gray-900 dark:text-white">{diff.description}</span>
                    </div>
                    <For each={diff.changes}>
                      {(change) => (
                        <div class="text-sm ml-4">
                          <span class="text-gray-500">{change.field}:</span>{' '}
                          <span class="text-red-500 line-through">{change.before}</span>{' '}
                          <span class="text-green-600">{change.after}</span>
                        </div>
                      )}
                    </For>
                  </div>
                )}
              </For>
            </div>
          </div>
        </div>
      </Show>
    </div>
  );
};

export default EditorView;
```

**Step 2: Verify it compiles**

Run: `cd ui && npx tsc --noEmit`
Expected: No type errors

**Step 3: Verify full app compiles**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: Compiles (warnings only)

**Step 4: Commit**

```bash
git add ui/src/components/views/EditorView.tsx
git commit -m "feat: implement EditorView with search, inline editing, queue, dry-run, and apply"
```

---

### Task 8: Fix `get_existing_filters` to correctly populate archive status

The existing `get_existing_filters` command hardcodes `archive: false` (line 106 of `filters.rs`). The editor needs real data. Also, the editor needs raw `ExistingFilterInfo` from the API, not the simplified `FilterView`.

**Files:**
- Modify: `src-tauri/src/commands/filters.rs:95-111` (fix archive field)
- Modify: `src-tauri/src/commands/editor.rs` (add command to get raw filters)

**Step 1: Fix the archive field in `get_existing_filters`**

In `src-tauri/src/commands/filters.rs`, change line 106:

From:
```rust
            archive: false, // ExistingFilterInfo doesn't track this
```

To:
```rust
            archive: f.remove_label_ids.contains(&"INBOX".to_string()),
```

**Step 2: Add a command to get raw filter data for the editor**

In `src-tauri/src/commands/editor.rs`, add:

```rust
/// Get raw filter data for the editor (includes all fields)
#[tauri::command]
pub async fn editor_get_filters(
    state: State<'_, AppState>,
) -> Result<Vec<ExistingFilterInfo>, String> {
    let client = state
        .get_client()
        .ok_or_else(|| "Not authenticated".to_string())?;

    let filters = client
        .list_filters()
        .await
        .map_err(|e| format!("Failed to fetch filters: {}", e))?;

    state.set_existing_filters(filters.clone());
    Ok(filters)
}
```

Register in `src-tauri/src/main.rs` (add to the Editor commands section):

```rust
            commands::editor_get_filters,
```

**Step 3: Add frontend API function**

In `ui/src/lib/api.ts`, add to the Editor section:

```typescript
export async function editorGetFilters(): Promise<EditorFilter[]> {
  return invoke<EditorFilter[]>('editor_get_filters');
}
```

**Step 4: Update EditorView to use `editorGetFilters`**

In `ui/src/components/views/EditorView.tsx`, change `loadFilters` to call `api.editorGetFilters()` directly instead of converting from `FilterView`:

```typescript
  const loadFilters = async () => {
    editor.setLoading(true);
    try {
      const filters = await api.editorGetFilters();
      editor.setFilters(filters);
    } catch (e) {
      console.error('Failed to load filters:', e);
    } finally {
      editor.setLoading(false);
    }
  };
```

**Step 5: Verify it compiles**

Run: `cargo check --manifest-path src-tauri/Cargo.toml && cd ui && npx tsc --noEmit`
Expected: Compiles

**Step 6: Commit**

```bash
git add src-tauri/src/commands/filters.rs src-tauri/src/commands/editor.rs src-tauri/src/main.rs ui/src/lib/api.ts ui/src/components/views/EditorView.tsx
git commit -m "feat: add editor_get_filters command with correct archive status"
```

---

### Task 9: Fix compiler warnings

Address all 12 warnings.

**Files:**
- Modify: `src-tauri/src/events.rs` (dead code warnings)
- Modify: `src-tauri/src/state.rs` (unused fields/methods)

**Step 1: Identify all warnings with locations**

Run: `cargo check --manifest-path src-tauri/Cargo.toml 2>&1 | grep "warning\b"`

**Step 2: For each warning, either:**
- Remove dead code (unused variants, structs, methods) if truly unused
- Add `#[allow(dead_code)]` if the code is intended for future use
- Fix the lifetime elision warnings per compiler suggestion

Approach: Read each file, understand what's unused, and remove or annotate.

The warnings from the earlier check:
1. `FilterOperation` variants `AnalyzingOverlaps`, `ApplyingRetroactive`, `Deleting` — never constructed
2. `LabelProgress` struct — never constructed
3. `LabelOperation` enum — never used
4. `AuthEvent` struct — never constructed
5. `ClusterEvent` variants `Selected`, `AllReviewed` — never constructed
6. `ErrorEvent` struct (in events.rs) — never constructed
7. Several `emit_*` methods never used
8. `processing_state` field never read
9. `has_client`, `get_classifications`, `cache_label`, `get_cached_label`, `get_stats` never used
10. `SessionStats` struct — never constructed
11-12. Lifetime elision warnings — use `'_` as suggested

**Step 3: Apply fixes**

For dead code: remove the unused items if they have no callers. For lifetime warnings: add explicit `'_`.

**Step 4: Verify zero warnings**

Run: `cargo check --manifest-path src-tauri/Cargo.toml 2>&1 | grep "warning"`
Expected: Only the "generated N warnings" summary line, ideally 0 warnings

**Step 5: Commit**

```bash
git add src-tauri/src/events.rs src-tauri/src/state.rs
git commit -m "fix: resolve all compiler warnings (dead code, unused fields, lifetime elision)"
```

---

### Task 10: Final verification

**Step 1: Run all Rust tests**

Run: `cargo test`
Expected: All tests pass

**Step 2: Run Tauri build check**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: 0 warnings, 0 errors

**Step 3: Run frontend type check**

Run: `cd ui && npx tsc --noEmit`
Expected: No errors

**Step 4: Manual smoke test**

Run: `cargo tauri dev`
Expected: App launches, Editor tab appears in sidebar, clicking it shows the Editor view

**Step 5: Commit any final fixes and push**

```bash
git push
```
