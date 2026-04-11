# Remaining Bugs — Design Document

Date: 2026-04-11

## Overview

Three bugs to fix in the Tauri GUI, ordered by impact:

1. **Bug 1:** `apply_filters` never creates labels or resolves label IDs (filters silently fail)
2. **Bug 3:** `parse_gmail_query` breaks on display names and `-subject:()` (false overlap reports)
3. **Bug 2:** "Exclude Forever" doesn't persist (exclusions lost on restart)

## Bug 1: apply_filters Missing Label Creation

### Problem

The GUI's `apply_filters` (`src-tauri/src/commands/filters.rs:315`) passes human-readable label names (e.g. `AutoManaged/Other/github.com`) as `target_label_id` directly to Gmail's filter creation API. Gmail expects actual label IDs (e.g. `Label_123`).

The CLI (`src/cli.rs:1520-1804`) has a working 3-step pipeline:
1. Load existing labels into cache via `LabelManager::load_existing_labels()`
2. Create missing labels via `LabelManager::create_label_direct()`
3. Resolve names to Gmail IDs via a `label_name_to_id` HashMap before calling `create_filter()`

### Design

Extract a shared function in `src/label_manager.rs`:

```rust
pub async fn create_labels_and_resolve_ids(
    label_manager: &mut LabelManager,
    filters: &[FilterRule],
    dry_run: bool,
) -> Result<(HashMap<String, String>, LabelCreationStats)>
```

This function:
1. Collects unique label names from `filters[].target_label_id`
2. Calls `load_existing_labels()` to populate the cache
3. For each label: checks cache, creates if missing, builds `name_to_id` map
4. Returns the map + stats (created/skipped counts)

The GUI's `apply_filters` command becomes:
1. Call `create_labels_and_resolve_ids()`
2. Clone each filter, replace `target_label_id` with the resolved Gmail ID
3. Call `FilterManager::create_filter()` with the resolved filter

The CLI's Step 8 calls the same function, replacing its inline loop.

The `AppState.label_cache` gets populated as a side effect for future use.

### Files Changed

- `src/label_manager.rs` — add `create_labels_and_resolve_ids()` + `LabelCreationStats` struct
- `src-tauri/src/commands/filters.rs` — update `apply_filters` to call shared function
- `src/cli.rs` — refactor Step 8 to call shared function (optional, can defer)

## Bug 3: parse_gmail_query Needs a Proper Tokenizer

### Problem

The current parser in `src/filter_overlap.rs:882-964` uses `string.find()` to locate field patterns. This breaks in three ways:

1. **Display names** like `from:(iiNET Support)` have no `@`, so `from_clause` becomes `None` — the filter appears to "match all senders"
2. **`-subject:(Foo)`** is not parsed, AND `subject:(` is found inside `-subject:(` — negative subjects become positive matches
3. These produce false "Identical" and "Subsumes" conflict reports

### Design

**Loose coupling:** Extract parsing behind a `QueryParser` trait so the implementation can be swapped later:

```rust
pub trait QueryParser: Send + Sync {
    fn parse(&self, query: &str) -> FilterExpr;
}
```

The current `parse_gmail_query` function delegates to a default implementation. Code that calls `parse_gmail_query` continues to work unchanged — the trait is for future extensibility (e.g. testing with mock parsers, or swapping to a recursive-descent parser later).

**Regex-based implementation (`RegexQueryParser`):**

Replace the `string.find()` approach with a single regex scan:

```
(-?)(from|subject|to):\(([^)]*)\)
```

For each match:
- Group 1 (`-` or empty) determines positive vs negative
- Group 2 is the field name
- Group 3 is the content inside parens

**Display name handling:**
- `from:` patterns without `@` and without `*@` are treated as opaque display names
- Add a `FromClause::DisplayName(String)` variant, or treat as opaque by mapping to a new sentinel
- Overlap analysis treats display-name clauses as **Disjoint** with everything (safe default — we can't know if "iiNET Support" overlaps with "support@iinet.com")

**Subject exclusion handling:**
- When the regex matches `-subject:(...)`, parse keywords the same way as positive subjects
- Store in a new field `FilterExpr.excluded_subjects: Vec<String>` (or `subject_exclusions`)
- Overlap analysis accounts for subject exclusions when comparing filters

### Files Changed

- `src/filter_overlap.rs` — add `QueryParser` trait, `RegexQueryParser` struct, refactor `parse_gmail_query` to delegate
- `src/filter_ast.rs` — add `FromClause::DisplayName(String)` variant, add `excluded_subjects` to `FilterExpr`
- `src/filter_overlap.rs` (overlap logic) — handle `DisplayName` as Disjoint, handle `excluded_subjects` in comparisons
- Existing tests updated + new tests for display names, `-subject:()`, trait interface

## Bug 2: Exclude Forever Doesn't Persist

### Problem

The GUI's `submit_cluster_decision` (`src-tauri/src/commands/clusters.rs:181`) stores `DecisionAction::Exclude` in the in-memory `gui_decisions` list. The `ExclusionManager` in `src/exclusions.rs` has full save/load/query support but is never used in the Tauri app.

### Design

1. Add `exclusion_manager: RwLock<ExclusionManager>` to `AppState`
2. On `AppState::new()`, load from `.gmail-automation/exclusions.json` via `ExclusionManager::load_sync()`
3. In `submit_cluster_decision`, when action is `Exclude`:
   - Derive the cluster key (same format as CLI: `*@domain.com` or `email@domain.com|subject:Pattern`)
   - Call `exclusion_manager.add(cluster_key, reason)`
   - Call `exclusion_manager.save()` to persist
4. When returning clusters to the frontend (e.g. `get_clusters` or equivalent), filter out any cluster whose key is in the exclusion set

### Files Changed

- `src-tauri/src/state.rs` — add `exclusion_manager` field, load on init
- `src-tauri/src/commands/clusters.rs` — integrate exclusion on Exclude action, filter on cluster retrieval

## Implementation Order

1. Bug 1 (filters don't work at all)
2. Bug 3 (false positives confuse users)
3. Bug 2 (convenience feature)
