# Tauri UI Bugfixes Design

Date: 2026-04-10

## Overview

Six bugs/improvements in the Tauri GUI, mostly in the review flow and overlap analysis.

## Fix 1: OR clause parsing in `parse_gmail_query`

**Problem:** `FromClause` only supports `Domain` or `SpecificSender`. When `parse_gmail_query` encounters `from:(a@x.com OR b@x.com)`, `EmailPattern::parse()` gets the full string including "OR", splits on `@`, gets 3+ parts, returns `None`. The `from_clause` becomes `None`, and the overlap engine treats `None` as "matches all senders" (`filter_overlap.rs:322`), producing false subsumption conflicts.

**Solution:** Add `MultipleSenders(Vec<FromClause>)` variant to `FromClause`.

Files to change:
- `src/filter_ast.rs` — Add `MultipleSenders` variant, update `describe()`, `domain()`
- `src/filter_overlap.rs` — Update `parse_gmail_query` to split on ` OR ` inside `from:(...)` and parse each part; update `analyze_from_relation` to handle `MultipleSenders`: subsumes B if any sender in the list subsumes B; subsumed by B if B subsumes every sender in the list
- `src/filter_overlap.rs` — Update `to_gmail_query` for the new variant

## Fix 2: Label ID vs name comparison in overlap engine

**Problem:** `analysis.rs:167` passes raw label IDs (e.g., `Label_41`) into `FilterActions::with_label()` for existing filters. Proposed filters use `target_label_id` which may be a human-readable path. `conflicts_with()` then compares mismatched identifiers, reporting false label conflicts.

**Solution:** Resolve label IDs to human-readable names before building `Filter` objects in `analysis.rs`. Pass the resolved name into `FilterActions::with_label()` for both existing and proposed filters. ~3 line change.

Files to change:
- `src-tauri/src/commands/analysis.rs` — Use `label_name` instead of `label_id` when constructing `FilterActions` (lines ~167, ~197)

## Fix 3: "A" shortcut to toggle auto-archive

**Problem:** The `'a'` keyboard case in `ReviewView.tsx` is a no-op.

**Solution:** On `'a'` press, clone the clusters array, flip `should_archive` on the current cluster, call `setClusters()`. No backend call needed — `should_archive` is only sent when a decision is submitted (`archive: cluster.should_archive`).

Files to change:
- `ui/src/components/views/ReviewView.tsx` — Implement the `'a'` case

## Fix 4: Custom label prepopulation + category quick-change

**Problem:** Pressing L opens an empty text input. User must type the full label from scratch.

**Solution:**
- Prepopulate `customLabel()` with `currentCluster().suggested_label`
- Add a category dropdown above the text input:
  - Parse categories dynamically from existing labels matching the prefix pattern (unique middle segments from `Prefix/Category/Rest`)
  - Selecting a category replaces only the middle segment, preserving prefix and domain
  - Text input remains fully editable for freetext override
- Need a Tauri command or reuse of label list to extract unique categories

Files to change:
- `ui/src/components/views/ReviewView.tsx` — Prepopulate on L press; add category selector UI
- `ui/src/lib/api.ts` — May need to expose label list or add category extraction
- Possibly `src-tauri/src/commands/` — If a new command is needed for categories

## Fix 5: Collapsible overlap analysis section

**Problem:** Hundreds of conflict cards shown at once (many false positives), overwhelming the UI.

**Solution:** Wrap the entire conflicts list in a collapsible section:
- Header always visible, showing summary counts ("3 errors, 12 warnings, 45 info")
- Click to expand/collapse the full list
- Default: collapsed
- Simple boolean signal for open/closed state

Files to change:
- `ui/src/components/views/FiltersView.tsx` — Wrap conflict list in collapsible container

## Fix 6: Apply filters progress in the GUI

**Problem:** `handleApply` awaits `applyFilters()` with no intermediate feedback. Backend already emits `FilterProgress` events.

**Solution:**
- Add `filterProgress` signal to track current progress
- Subscribe to `onFilterProgress` before calling `applyFilters()`
- Show progress inline: "Creating filter 3/15: filter-name..."
- Disable buttons during apply
- Unsubscribe on completion

Files to change:
- `ui/src/components/views/FiltersView.tsx` — Add progress state, subscribe to events, render progress bar
