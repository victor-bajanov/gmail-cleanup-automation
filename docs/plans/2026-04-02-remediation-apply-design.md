# Remediation Apply: Label Swap for PickWinner Groups

## Problem

The remediation feature currently only modifies Gmail filters. When a PickWinner group is resolved (user picks label A over label B), existing emails still carry the old labels. New mail routes correctly, but historical messages remain mislabeled.

## Approach

Separate apply phase (Approach B) — a `RemediationApplicator` that runs after filter execution. Clean separation between filter CRUD and email label changes.

Pipeline becomes: detect → decide → execute (filters) → apply (emails)

## Data Model

```rust
pub struct LabelSwap {
    pub from_pattern: String,          // Gmail from: query
    pub add_label_id: String,          // winner label
    pub remove_label_ids: Vec<String>, // loser label(s)
}

pub struct ApplyResult {
    pub messages_relabeled: usize,
    pub messages_failed: usize,
    pub errors: Vec<String>,
}
```

Only PickWinner decisions produce LabelSwaps. Consolidate and MechanicalFix need no email changes (same label).

## RemediationApplicator

```rust
pub struct RemediationApplicator;

impl RemediationApplicator {
    pub fn collect_swaps(plan: &RemediationPlan) -> Vec<LabelSwap>;
    pub async fn apply(
        client: &dyn GmailClient,
        swaps: &[LabelSwap],
    ) -> Result<ApplyResult>;
}
```

**collect_swaps**: Iterates plan groups. For KeepOne decisions, the kept filter's label is the winner; all other filters' labels (deduplicated, excluding winner) are losers. from_pattern comes from the group.

**apply**: For each LabelSwap:
1. Build query: `from:{from_pattern}` scoped to loser labels
2. `client.list_message_ids(&query)` to find affected messages
3. `client.batch_modify_labels(&ids, &[winner], &losers)` to swap
4. Track counts, continue on errors (non-fatal)

## CLI Integration

Flags:
- `--dry-run`: shows plan + swaps, executes neither
- `--no-apply`: executes filter changes, skips email swaps

After filter execution, shows swap plan with estimated email counts (from read-only query), then separate confirmation prompt before touching emails.

## Tauri Commands

- `collect_swaps` — returns `Vec<LabelSwap>` for frontend display
- `apply_swaps` — executes swaps, returns `ApplyResult`

## Error Handling

- Non-fatal: failed batch logs warning, continues to next swap
- Empty swap list: skip apply phase with message
- Idempotent: adding existing label or removing absent label is a no-op in Gmail API
- Rate limiting: reuses existing batch_modify_labels chunking (1000 msgs) and quota permits
- No rollback: matches main flow philosophy
