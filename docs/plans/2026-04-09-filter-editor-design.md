# Filter Editor UI — Design Document

**Date:** 2026-04-09
**Branch:** feature/tauri-gui

## Goal

Add a new top-level "Editor" view that lets users search, edit, and delete existing Gmail filters. Actions are queued with dry-run preview before applying.

## Requirements

- Full-text search across all filter fields (from, to, subject, query, label names)
- Toggle archive on/off per filter
- Edit label assignments (add/remove labels)
- Delete filters (e.g. to resolve "merge these" suggestions from coverage analyser)
- Action queue with dry-run (detailed before/after diff) and apply
- Queue persists across view switches (session-scoped, clears on app restart)
- TDD approach; fix compiler warnings at end

## Architecture

Three layers matching existing project patterns:

- **Backend (Rust):** New `src/filter_editor.rs` for action types and dry-run logic. New `src-tauri/src/commands/editor.rs` for Tauri commands.
- **Frontend (SolidJS):** New `EditorView.tsx`, new `editor` store in `stores/app.ts`, new API functions in `lib/api.ts`.
- **No new crate dependencies** — uses existing `GmailClient` trait methods (`list_filters`, `update_filter`, `delete_filter`).

### Data Flow

```
Load filters -> Search/filter list -> User queues edit/delete actions
-> Dry run (preview diffs) -> Apply (execute via GmailClient) -> Refresh list
```

## Backend Types (`src/filter_editor.rs`)

```rust
enum FilterAction {
    UpdateArchive { filter_id: String, new_value: bool },
    UpdateLabels { filter_id: String, add: Vec<String>, remove: Vec<String> },
    Delete { filter_id: String },
}

struct ActionDiff {
    filter_id: String,
    description: String,       // e.g. "from:(*@github.com)"
    action_type: String,       // "update" | "delete"
    changes: Vec<FieldChange>, // before/after pairs
}

struct FieldChange {
    field: String,    // "archive", "labels"
    before: String,   // "ON" / "GitHub, Notifications"
    after: String,    // "OFF" / "GitHub"
}

struct ApplyResult {
    succeeded: usize,
    failed: Vec<(String, String)>, // (filter_id, error)
}
```

**Key decisions:**
- Actions reference filters by ID — queue is lightweight
- `dry_run` is a pure function: takes queue + current filters, produces `Vec<ActionDiff>` — no API calls
- `apply` executes actions sequentially (respecting rate limiter), returns results
- Queue lives in frontend state only (SolidJS signal) — backend is stateless for this feature

## Tauri Commands (`src-tauri/src/commands/editor.rs`)

- `editor_dry_run(actions, filters) -> Vec<ActionDiff>` — pure, no state needed
- `editor_apply(actions, state) -> ApplyResult` — executes via GmailClient

## Frontend

### EditorView Layout

1. **Search bar + filter list** (main area)
   - Text input with full-text search across all fields
   - Filter rows show: query pattern, labels, archive status badge
   - Click to expand inline edit: archive toggle, label multi-select, delete button
   - Queued actions shown as colored indicator (orange = edit, red = delete)

2. **Action queue panel** (sidebar)
   - List of queued actions with human-readable descriptions
   - Remove individual actions, "Clear All" button
   - Count badge

3. **Action bar** (bottom)
   - "Dry Run" button -> modal with ActionDiff details
   - "Apply" button -> executes with confirmation
   - Both disabled when queue is empty

### Store (`stores/app.ts`)

- `editorFilters: ExistingFilterInfo[]` — loaded filter list
- `editorSearch: string` — search term
- `editorQueue: FilterAction[]` — pending actions
- `editorLoading: boolean`

### API (`lib/api.ts`)

- `loadEditorFilters()` — reuses existing `get_existing_filters`
- `dryRunActions(queue, filters)` — calls `editor_dry_run`
- `applyEditorActions(queue)` — calls `editor_apply`

## Testing Strategy (TDD)

**Rust unit tests** in `src/filter_editor.rs`:
- `dry_run` pure function: correct `ActionDiff` for each action type
- Edge cases: nonexistent filter ID, duplicate actions on same filter, empty queue

**Rust integration tests** in `tests/editor_test.rs`:
- Mock `GmailClient` (reuse pattern from `remediation_test.rs`)
- `apply` executes correct `delete_filter` / `update_filter` calls
- Partial failure handling

**No frontend tests** — matching existing project convention.

## Post-Feature

Fix all compiler warnings (12 currently).
