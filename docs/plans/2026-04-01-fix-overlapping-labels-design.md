# Fix Overlapping Automanaged Labels

## Problem

Emails from a single sender (e.g., cba.com.au) receive multiple overlapping
Automanaged labels (Financial, Other, Personal, Receipts) instead of being
routed to one label per email.

The clustering algorithm creates separate clusters for different subject
patterns from the same sender. Each cluster produces a filter with a different
target label. The remainder cluster (no subject pattern) generates a broad
`from:` filter that matches all emails from that sender, including ones already
captured by subject-specific clusters.

The deduplication logic does not catch this because its key includes
`target_label_id`, so two filters with the same `from_pattern` but different
labels are treated as distinct.

## Desired Behavior

Subject-based routing with mutual exclusivity: subject-pattern clusters get
their own labels, and the remainder cluster explicitly excludes those subject
patterns so filters never overlap.

## Design

### Part 1: Clustering Fix (interactive.rs)

Track which subject patterns were extracted for each sender. When building the
remainder cluster, store those patterns as `excluded_subject_patterns` on the
EmailCluster. This flows through to filter generation, producing queries like:

```
from:sender@cba.com.au -subject:(statement) -subject:(receipt)
```

### Part 2: Deduplication Safety Net (filter_manager.rs)

Add a check in `is_redundant_filter`: when a new filter has the same
`from_pattern` as an existing filter, and the new filter has no subject
keywords or exclusions, but the existing filter does have subject keywords,
mark the new filter as redundant.

### Changes by File (all in src/, the library)

1. **models.rs** -- Add `excluded_subject_patterns: Vec<String>` to
   EmailCluster and FilterRule
2. **interactive.rs** -- In `create_clusters`, collect extracted subject
   patterns per sender and pass them to the remainder cluster
3. **filter_manager.rs** -- Two changes:
   - `build_gmail_query`: emit `-subject:(pattern)` terms from
     excluded_subject_patterns
   - `is_redundant_filter`: detect broad-vs-specific overlap on same
     from_pattern

### What Does Not Change

- Subject-pattern cluster creation logic (already correct)
- Interactive review UI (clusters appear the same to the user)
- Label hierarchy or naming
- CLI or Tauri code (they consume FilterRule from the library)

### Testing

- Unit test: `build_gmail_query` emits `-subject:` exclusions
- Unit test: `is_redundant_filter` catches broad overlaps
- Unit test: remainder clusters carry `excluded_subject_patterns`
