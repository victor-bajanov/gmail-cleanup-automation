# Remediate Existing Overlapping Gmail Filters

## Problem

Previous runs of the tool created overlapping Gmail filters for the same
sender — e.g., cba.com.au has 4 separate filters pointing to Financial, Other,
Personal, and Receipts labels. All 4 match the same emails, so emails get 4
labels.

The prevention fix (excluded_subject_patterns, see
2026-04-01-fix-overlapping-labels-design.md) stops new overlaps from being
created. This design addresses cleaning up existing overlaps already in Gmail.

## Approach

New library module `src/filter_remediation.rs` with a detect → decide → execute
pipeline. Consumed by both a new CLI `remediate` subcommand and new Tauri
commands.

## Data Model

```rust
/// A group of existing Gmail filters that overlap on the same sender(s)
pub struct OverlapGroup {
    pub group_id: String,                    // e.g., "cba.com.au"
    pub from_pattern: String,                // shared from: pattern
    pub filters: Vec<ExistingFilterInfo>,     // the overlapping filters
    pub label_names: Vec<String>,            // resolved label names for display
    pub resolution_type: ResolutionType,
}

pub enum ResolutionType {
    /// Filters have distinct subject clauses — can mechanically add
    /// -subject: exclusions to make them mutually exclusive
    MechanicalFix {
        proposed_replacements: Vec<FilterRule>,
    },
    /// Filters are identical in criteria — user must pick which label wins
    PickWinner,
}

/// User's decision for one overlap group
pub enum GroupDecision {
    /// Replace all with mutually exclusive filters
    ReplaceWithExclusive { replacement_filters: Vec<FilterRule> },
    /// Keep one filter, delete the rest
    KeepOne { keep_filter_id: String },
    /// Skip this group, don't touch it
    Skip,
    /// Re-scan this sender's emails to generate fresh exclusive filters
    Rescan { from_pattern: String },
}

/// The full remediation plan before execution
pub struct RemediationPlan {
    pub groups: Vec<(OverlapGroup, GroupDecision)>,
}

/// Result of executing the plan
pub struct RemediationResult {
    pub deleted: Vec<String>,      // filter IDs deleted
    pub created: Vec<String>,      // filter IDs created
    pub skipped: usize,
    pub errors: Vec<String>,
}
```

Flow: `detect_overlaps()` → `Vec<OverlapGroup>` → user makes `GroupDecision`
per group → `build_plan()` → `execute_plan()`.

## Detection Logic

```rust
pub struct OverlapDetector;

impl OverlapDetector {
    pub async fn detect(
        client: &dyn GmailClient,
        label_map: &HashMap<String, String>,  // label_id -> label_name
    ) -> Result<Vec<OverlapGroup>>
}
```

### Grouping Algorithm

1. Fetch all filters via `list_filters()`
2. Parse each filter's `from` field (and `query` field as fallback) into a
   `FromClause` using existing `parse_gmail_query()`
3. Group filters that share the same effective sender — same domain, same
   specific sender, or domain-subsumes-sender (e.g., `from:cba.com.au` and
   `from:noreply@cba.com.au`)
4. Discard groups of size 1 (no overlap)
5. For each group, classify:
   - If any filters have distinct `subject` clauses → `MechanicalFix`,
     synthesize replacement filters with `-subject:` exclusions on the
     bare/remainder filter
   - If all filters have identical matching criteria → `PickWinner`

### MechanicalFix Synthesis

- Collect all subject keywords from subject-specific filters in the group
- For the bare `from:`-only filter (the remainder), generate a replacement
  with `-subject:(kw1) -subject:(kw2)` exclusions
- Subject-specific filters stay as-is (they're already narrow)
- Net effect: delete all in group, recreate with exclusions on the remainder

### Reuses

- `FilterOverlapAnalyzer::analyze_from_relation()` for subsumption checks
- `parse_gmail_query()` for parsing existing filter criteria
- `build_gmail_query()` for generating new query strings

## Execution & Rollback

```rust
impl RemediationPlan {
    pub async fn execute(
        &self,
        client: &dyn GmailClient,
    ) -> Result<RemediationResult>
}
```

### Execution Order (per group)

1. Delete all filters in the group (for `KeepOne`, skip deleting the winner)
2. Create replacement filters
3. On error mid-group, log what was deleted/created and continue to next group

### Rollback

Write a rollback entry before executing so the existing `gmail-filters rollback`
command works as-is. No new rollback infrastructure needed.

### Dry-run

`RemediationPlan::summary()` returns a human-readable description of proposed
changes. Used by CLI `--dry-run` and GUI preview.

## CLI Integration

New subcommand:

```
gmail-filters remediate [--dry-run]
```

Flow:
1. Authenticate, fetch label map
2. `OverlapDetector::detect()` → get overlap groups
3. If no overlaps, print message and exit
4. For each group, interactive prompt (using `inquire`):
   - Show from pattern, affected labels, resolution type
   - `MechanicalFix`: show proposed replacements, confirm or skip
   - `PickWinner`: show labels as choices, user picks one (plus "rescan"
     and "skip" keybindings)
5. Build plan, show summary, confirm, execute

## Tauri Integration

New file `src-tauri/commands/remediation.rs`:

```rust
#[tauri::command]
async fn detect_overlaps(state: State<'_, AppState>) -> Result<Vec<OverlapGroup>>

#[tauri::command]
async fn submit_group_decision(
    group_id: String,
    decision: GroupDecision,
    state: State<'_, AppState>,
) -> Result<()>

#[tauri::command]
async fn execute_remediation(state: State<'_, AppState>) -> Result<RemediationResult>

#[tauri::command]
async fn remediation_summary(state: State<'_, AppState>) -> Result<String>
```

Decisions stored in `AppState` as user works through groups. Frontend gets a
new `RemediationView.tsx` (frontend implementation out of scope for this design).

### Rescan Path

When user picks "rescan" for a `PickWinner` group, call the existing scanner +
classifier pipeline scoped to that sender's emails, generate fresh exclusive
filters, and slot them into the plan as `ReplaceWithExclusive`. Reuses existing
infrastructure.

## Testing

### Unit Tests (in `filter_remediation.rs`)

- Grouping: filters with same `from:` domain land in same group
- Grouping: domain subsumes specific sender (e.g., `cba.com.au` groups with
  `noreply@cba.com.au`)
- Grouping: unrelated senders stay separate
- Classification: group with subject differences → `MechanicalFix`
- Classification: group with identical criteria → `PickWinner`
- MechanicalFix synthesis: remainder filter gets correct `-subject:` exclusions
- Plan execution: correct delete/create sequence with mock client

### Integration Test

- Round-trip: detect → decide → execute → re-detect shows zero overlaps

## What Does Not Change

- Existing filter creation pipeline
- Existing overlap analysis UI in Tauri (read-only analysis)
- Label hierarchy or naming
- Rollback infrastructure (reused as-is)
