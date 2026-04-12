# Remediation UI Design

## Overview

Build the Tauri GUI frontend for the existing filter remediation flow. The backend (6 Tauri commands) is already wired. This adds a new top-level `RemediationView` with a phased single-page UI and Puppeteer-based TDD against the Vite dev server.

## Decisions

- **Layout**: Phased single-page (detect → decide → confirm → results)
- **Navigation**: Top-level sidebar item, new `'remediation'` AppView
- **PickWinner UX**: Click-to-select filter rows (not radio buttons or dropdown)
- **Card layout**: All groups shown as cards (not wizard-style one-at-a-time)
- **Testing**: Puppeteer + `node:test` against Vite dev server, Tauri invoke mocked
- **Scope**: Frontend only — no changes to Rust logic

## Types

Added to `ui/src/types/index.ts`:

```typescript
interface OverlapGroup {
  group_id: string;
  from_pattern: string;
  filters: OverlapFilter[];
  label_names: string[];
  resolution_type: ResolutionType;
}

interface OverlapFilter {
  id: string;
  from: string | null;
  query: string | null;
  subject: string | null;
  add_label_ids: string[];
  remove_label_ids: string[];
}

type ResolutionType =
  | { Consolidate: { keep_filter_id: string; remove_filter_ids: string[] } }
  | { MechanicalFix: { proposed_replacements: FilterRule[] } }
  | 'PickWinner';

type GroupDecision =
  | { Consolidate: { keep_filter_id: string; remove_filter_ids: string[] } }
  | { ReplaceWithExclusive: { replacement_filters: FilterRule[] } }
  | { KeepOne: { keep_filter_id: string } }
  | 'Skip'
  | { Rescan: { from_pattern: string } };

interface RemediationResult {
  deleted: string[];
  created: string[];
  skipped: number;
  errors: string[];
}

interface LabelSwap {
  query: string;
  add_label_id: string;
  remove_label_ids: string[];
}

interface ApplyResult {
  messages_relabeled: number;
  messages_failed: number;
  errors: string[];
}
```

## API Layer

6 functions in `ui/src/lib/api.ts` wrapping existing Tauri commands:

- `detectOverlaps()` → `OverlapGroup[]`
- `submitGroupDecision(groupId, decision)` → `void`
- `executeRemediation()` → `RemediationResult`
- `remediationSummary()` → `string`
- `collectRemediationSwaps()` → `LabelSwap[]`
- `applyRemediationSwaps()` → `ApplyResult`

## Store

New `remediation` export in `ui/src/stores/app.ts`:

- `groups: OverlapGroup[]`
- `decisions: Record<string, GroupDecision>`
- `phase: 'idle' | 'detecting' | 'deciding' | 'confirming' | 'executing' | 'results'`
- `summary: string`
- `result: RemediationResult | null`
- `swaps: LabelSwap[]`
- `applyResult: ApplyResult | null`

## View Phases

### Phase 1: Detect (idle / detecting)
- "Detect Overlaps" button
- Loading spinner while scanning
- Empty state: "No overlapping filters found"

### Phase 2: Decide (deciding)
- Header: "X overlap groups found"
- Cards per group:
  - **Consolidate**: "Same label — N redundant". Accept / Skip buttons.
  - **MechanicalFix**: "Auto-fixable" + proposed queries. Accept Fix / Skip buttons.
  - **PickWinner**: Clickable filter rows (highlight winner on click). Skip button.
- Card visual states: undecided (gray left border), decided (green), skipped (yellow)
- Sticky footer: "N/M decided" + "Review Plan" button (enabled when all decided)

### Phase 3: Confirm (confirming)
- Summary text from `remediation_summary`
- Stats: deletions / creations / skips
- "Execute" + "Back to Decisions" buttons

### Phase 4: Results (executing / results)
- Execution results: deleted/created/skipped/errors
- If swaps exist: swap details + "Apply Label Changes" button
- If no swaps: "No label changes needed"
- After apply: relabeled/failed counts
- "Done" button → back to idle

## Navigation Changes

- `AppView` type: add `'remediation'`
- `Sidebar.tsx`: add nav item `{ id: 'remediation', label: 'Remediation', icon: '🔀' }`
- `App.tsx`: add `<Match when={navigation.currentView() === 'remediation'}><RemediationView /></Match>`

## Testing

**Stack**: Puppeteer + `node:test` against Vite dev server (`localhost:1420`).

**Mock**: Replace `window.__TAURI_INTERNALS__.invoke` with canned responses per command.

**Test cases**:

1. Detect phase — renders button, click triggers detect, shows loading, transitions to decide
2. Detect empty — shows "no overlaps" message
3. Consolidate card — accept/skip buttons, accept submits correct decision
4. MechanicalFix card — shows proposed replacements, accept sends ReplaceWithExclusive
5. PickWinner card — clickable rows, click highlights winner, sets KeepOne
6. All decided — "Review Plan" enabled only when all groups have decisions
7. Confirm phase — shows summary, execute button calls execute_remediation
8. Results phase — shows deleted/created/skipped, shows apply button when swaps exist
9. Apply — calls apply_remediation_swaps, shows relabeled count
10. Navigation — sidebar item navigates to remediation view

**Files**:
```
ui/tests/setup.ts           — Vite dev server + Puppeteer browser lifecycle
ui/tests/mock-tauri.ts      — __TAURI_INTERNALS__ mock helper
ui/tests/remediation.test.ts — 10 test cases
```
