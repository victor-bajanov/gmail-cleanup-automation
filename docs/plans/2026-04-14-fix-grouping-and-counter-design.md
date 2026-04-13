# Fix Overlap Grouping and Decision Counter

Date: 2026-04-14

## Bug 1: False overlap grouping

### Problem

`group_filters()` in `filter_remediation.rs:676-715` groups filters purely by domain. A catch-all filter with `-from:` exclusions (e.g., `from:(amazon.com.au) -from:(order-update@...) -from:(auto-confirm@...)`) gets grouped with the specific sender filters it explicitly excludes, creating false overlap groups.

### Root cause

`parse_from_key()` extracts only the positive `from_clause` and ignores exclusions entirely. The pairwise `analyze_expr_relation()` (which correctly identifies disjoint filters via `exclusions_resolve_overlap`) is never invoked during grouping.

### Design

Replace domain-based grouping with union-find grouping based on actual overlap analysis:

1. Parse every filter's query into a `FilterExpr` using `parse_gmail_query()` (or extract from `from`/`subject` fields)
2. For each pair of filters, run `analyze_expr_relation` — if not `Disjoint`, union them
3. Build connected components — each component becomes an `OverlapGroup`
4. Discard singletons

This is O(n^2) pairwise comparisons across all filters. At 300 filters = ~45k pairs, sub-second in Rust.

The `group_id` and `from_pattern` for each group can be derived from the dominant domain in the component.

### Key files

- `src/filter_remediation.rs:676-715` — `group_filters()`, replace core algorithm
- `src/filter_overlap.rs:318-352` — `analyze_expr_relation()`, already works correctly
- `src/filter_overlap.rs:999` — `parse_gmail_query()`, used to build `FilterExpr` from queries

## Bug 2: Decision counter stuck at 0/30

### Problem

The "X/30 decided" counter and "Review Plan" button never update even though groups show "Accepted" badges after picking winners.

### Root cause

`decidedCount` and `allDecided` in `ui/src/stores/app.ts:208-213` use `createMemo()` at module top-level, outside any SolidJS reactive root (`render`/`createRoot`). The memos compute their initial values but don't properly track signal updates because they have no owning scope. The "Accepted" badge works because `getDecisionState()` is a plain function called inside the component's reactive scope.

### Design

Replace `createMemo(...)` with plain arrow functions:

```typescript
decidedCount: () => Object.keys(remediationDecisions()).length,
allDecided: () => {
  const groups = remediationGroups();
  const decisions = remediationDecisions();
  return groups.length > 0 && groups.every(g => g.group_id in decisions);
},
```

SolidJS tracks signal access reactively when these are called from JSX, so no memo needed.

### Key files

- `ui/src/stores/app.ts:208-213` — change `createMemo` to plain functions
