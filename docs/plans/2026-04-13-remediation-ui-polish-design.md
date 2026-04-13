# Remediation UI Polish — Design

## Context

The remediation UI (RemediationView.tsx) is functional but has four usability gaps identified in `docs/plans/remediation-ui-todos.md`. This design addresses all four for an advanced, keyboard-first user processing high volumes of overlap groups.

## 1. Progress indicator during apply

**Problem:** Only a generic spinner during execution. No per-group progress or time estimate.

**Design:**

- Wire `execute_with_progress()` (filter_remediation.rs) to emit Tauri events via `app_handle.emit()` on each tick. Payload: `{ done: usize, total: usize, group_id: String }`.
- Frontend listens via `listen()` from `@tauri-apps/api/event` and updates a reactive signal.
- Display: compact inline counter `3/12 groups — ~15s remaining` with a thin progress bar.
- ETA estimation: track timestamps of each tick, compute rolling average duration per group, multiply by remaining count. Suppress ETA display until 3+ ticks have landed to avoid wild early estimates.
- The Tauri command `execute_remediation` switches from calling `execute()` to `execute_with_progress()`, passing a closure that emits events.

## 2. Label IDs to names in filter rows

**Problem:** Filter rows show raw label IDs (e.g. `Label_42`) instead of human-readable names.

**Design:**

- The `detect_overlaps` command already builds a `id_to_name: HashMap<String, String>`. Return it alongside the groups as a separate field in the response.
- Frontend stores the map in the remediation store.
- All places that render `filter.add_label_ids` resolve through this map at display time. Fall back to the raw ID if not found.

## 3. Compact label display with segment-level diff highlighting

**Problem:** Automanaged label names are verbose (e.g. `automanaged/other/airbnb-com`). When labels conflict, differences are hard to spot in a comma-separated list.

**Design:**

### Prefix stripping

- The label prefix is available from config (`label_prefix`). Strip it from display everywhere in the remediation UI.
- Example: `automanaged/other/airbnb-com` with prefix `automanaged` displays as `other/airbnb-com`.
- Non-managed labels (no matching prefix) display as-is.
- Show the full label in a tooltip for disambiguation.

### Segment-level diff highlighting

When a group has conflicting labels (PickWinner or MechanicalFix with different labels), highlight the differing segments:

1. Split each label (after prefix stripping) by `/` into segments.
2. Compare segments pairwise across labels. Any segment index where values differ gets highlighted.
3. Render: shared segments in muted/gray text, differing segments in bold accent color.

Examples:
- `other/airbnb-com` vs `marketing/airbnb-com` — highlight `other` and `marketing` (segment 0 differs)
- `other/airbnb-com` vs `other/airbnb-com-au` — highlight `airbnb-com` and `airbnb-com-au` (segment 1 differs)
- `other/airbnb-com` vs `marketing/airbnb-com-au` — highlight both segments in both labels (segments 0 and 1 differ)

Edge cases:
- Labels with different segment counts: treat missing segments as differing.
- Labels with special characters: segment splitting is purely on `/`, no escaping needed (Gmail label names can contain most characters except `/`).

### Label format summary

Labels have at most 3 segments: `prefix/category/domain`. After prefix stripping, display shows at most `category/domain`. Non-managed labels nest up to 2 deep (e.g. `Promotions/Groupon`).

## 4. Pick winner override on MechanicalFix cards

**Problem:** MechanicalFix groups only offer Accept Fix or Skip. Sometimes the user wants to just pick a winner instead.

**Design:**

- MechanicalFix cards show the subject pattern detail as today, plus each filter's label is rendered as a clickable element.
- Clicking a filter's label sends a `KeepOne` decision for that filter (overriding the MechanicalFix).
- Keyboard shortcuts on a focused MechanicalFix card:
  - `A` — accept the mechanical fix (ReplaceWithExclusive decision)
  - `S` — skip
  - `1`, `2`, ... — pick winner by filter index (KeepOne decision for that filter)
- This unifies the interaction model: the mechanical fix is the default/recommended action, but picking a winner is always one keypress away.
- The backend already supports `KeepOne` decisions for any group regardless of resolution type, so no backend changes needed for this.

## Testing strategy

- **Frontend (Puppeteer):** Use existing Puppeteer test infrastructure to test all four changes against mocked Tauri invoke responses.
- **Property-based testing:** Use property-based tests (fast-check or similar) for:
  - Label ID to name resolution (arbitrary ID strings, missing entries, special characters)
  - Prefix stripping (arbitrary prefixes, labels that don't match prefix, empty prefix)
  - Segment diff highlighting (arbitrary segment counts, special characters, identical labels, completely different labels)
- **Rust backend:** Property-based tests (proptest) for the progress event emission and label map construction.
- **TDD approach:** Red-green-TDD — write failing tests first, then implement.
