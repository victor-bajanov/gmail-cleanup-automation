# Remaining Bugs — Session Prompt

Paste this into a new Claude Code session on branch `feature/tauri-gui`.

---

There are 4 remaining bugs to fix in this Tauri GUI app. Read the known bugs file at `.claude/projects/-Users-victorb-code-gmail-cleanup/memory/project_known_bugs.md` for full context, but here's the summary:

## Bug 1: apply_filters never creates labels or resolves label IDs

**Priority: HIGH — filters silently don't work**

The GUI's `apply_filters` in `src-tauri/src/commands/filters.rs:315` passes human-readable label names (e.g., `AutoManaged/Other/github.com`) straight to the Gmail API as label IDs. The CLI has a two-step process: (1) create labels via LabelManager, (2) resolve names to Gmail IDs before creating filters. See `src/cli.rs:1520-1804` for the working implementation.

**Fix approach:** Extract the full pipeline (create labels → build name→ID map → resolve IDs → create filters) from the CLI into a shared library function in `src/filter_manager.rs` or `src/label_manager.rs`. Both CLI and GUI should call the same function. The GUI's `apply_filters` command and the CLI's Step 8+9 should both be thin wrappers around this shared function.

## Bug 2: "Exclude Forever" doesn't persist

The GUI's `submit_cluster_decision` stores `DecisionAction::Exclude` in memory only. The CLI has a working `ExclusionManager` in `src/exclusions.rs` that saves to `.gmail-automation/exclusions.json`. The GUI backend never integrates with it.

**Fix approach:** Add `ExclusionManager` to `AppState` in `src-tauri/src/state.rs`. On Exclude decisions in `src-tauri/src/commands/clusters.rs`, call `exclusion_manager.add()` and `save()`. When loading clusters, filter out excluded ones.

## Bug 3: `parse_gmail_query` needs a proper tokenizer

The current parser is a series of `query.find("from:(")` string searches that keeps failing:
- Display names like `from:(iiNET Support)` have no `@`, so `from_clause` becomes `None` → "matches all"
- `-subject:(Foo)` is not parsed, AND `subject:(` is found inside `-subject:(` so negative subjects get parsed as positive
- These produce false "Identical" and false "Subsumes" conflicts

**Fix approach:** Replace the string-search parser in `src/filter_overlap.rs:parse_gmail_query` with a proper tokenizer that handles:
- Positive/negative prefixes (`-`)
- Field names (`from:`, `subject:`, `to:`)
- Parenthesized groups with OR (already partially handled via `MultipleSenders`)
- Display names (no `@`) vs email addresses — treat unparseable from patterns as opaque/Disjoint, not None
- Subject exclusions as a new `ExclusionClause` variant or similar

There are existing tests in the file — make sure they still pass and add tests for the new cases.

## Suggested order

1. Bug 1 first (filters don't work at all — highest impact)
2. Bug 3 second (false positives are confusing but not destructive)  
3. Bug 2 third (exclude is a convenience feature)

Use `/brainstorming` before starting each bug to design the approach.
