# Fix Label Display and Overlap Detection

Date: 2026-04-13

## Bug 1: Label names not shown in remediation view

### Problem

Filter rows in the remediation view display raw IDs (e.g. `Label_22`, `Label_171`) and group headers show `Labels:` with nothing after it. The `label_cache` in `AppState` is empty when `detect_overlaps` reads it because it's only populated during filter fetching, which may not have happened yet.

### Design

Preload label and filter caches asynchronously right after `initialize_client` succeeds (`src-tauri/src/commands/auth.rs:203`). Once the Gmail client is available, spawn an async task that:

1. Fetches all Gmail labels and populates `state.label_cache` (name→id map)
2. Fetches existing filters into whatever filter cache exists

This makes the caches warm by the time the user navigates to any view. The `detect_overlaps` command continues to read the cache as-is — it just won't be empty anymore.

### Key files

- `src-tauri/src/commands/auth.rs` — `initialize_client`, add preload spawn after line 203
- `src-tauri/src/state.rs` — `label_cache` field (line 50)
- `src-tauri/src/commands/filters.rs:363` — existing cache population logic to reuse

## Bug 2: Overlap detection false positives with `-subject:` exclusions

### Problem

Overlap detection falsely flags filters with `-subject:` exclusions as overlapping. Example:
- Filter A: `from:(x) subject:(Y)` 
- Filter B: `from:(x) -subject:(Y)`

These are mutually exclusive (one catches subject matches, the other catches everything else), but detection reports Filter A as a subset of Filter B.

### Root cause

`analyze_expr_relation()` in `filter_overlap.rs:318-352` handles `subject_clause` (positive) and `exclusions` (negative FROM via `exclusions_resolve_overlap`), but completely ignores the `subject_exclusions` field on `FilterExpr`.

When Filter B has `-subject:(Y)` and no `subject_clause`, the code hits the `(Some(_), None)` arm at line 336, returning `SubsumedBy` — treating "no positive subject" as "matches all subjects", when B actually excludes those subjects.

### Design

Add a `subject_exclusions_resolve_overlap()` method mirroring the existing `exclusions_resolve_overlap` pattern:

1. Check if A has `subject_clause` with keywords K and B has `subject_exclusions` containing all of K → disjoint (B excludes everything A requires)
2. Check the symmetric case (B has `subject_clause`, A has `subject_exclusions`)
3. Call this in `analyze_expr_relation` alongside the existing FROM exclusion check at line 346

This mirrors Gmail semantics: `subject:(Y)` requires Y in the subject; `-subject:(Y)` requires Y NOT in the subject. Same keywords = mutually exclusive.

### Key files

- `src/filter_overlap.rs` — `analyze_expr_relation` (line 318), new `subject_exclusions_resolve_overlap` method
- `src/filter_ast.rs` — `FilterExpr.subject_exclusions` field (line 48)
- `src/filter_overlap.rs` — `exclusions_resolve_overlap` (line 544) as pattern to follow
