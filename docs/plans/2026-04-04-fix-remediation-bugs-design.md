# Fix Remediation Filter Bugs

## Problem

Running the remediation apply phase causes most emails to get tagged with dozens
of unrelated labels and archived. Root cause: 4 bugs in
`src/filter_remediation.rs`.

### Bug 1: `from_pattern` lost on replacement filters

`classify_groups` line 548 reads `filter.from.clone()` to build replacement
`FilterRule`s. But Gmail stores `from:` criteria in either the `from` field or
the `query` field depending on how the filter was created. `parse_from_key`
already handles this duality for grouping, but `classify_groups` only reads
`filter.from`. When `from` is `None`, the replacement gets `from_pattern: None`
— a subject-only filter with no sender restriction.

### Bug 2: Apply query is domain-only

`apply()` line 323 builds `format!("from:{}", swap.from_pattern)`. This matches
every email from the entire domain, regardless of subject or other criteria. A
swap intended for emails matching `from:X subject:Y` hits all emails from X.

### Bug 3: `LabelSwap` drops filter specificity

The `LabelSwap` struct only carries `from_pattern: String`. Even if detection
correctly identified which emails each filter covers, that information is
discarded before `apply` runs.

### Bug 4: `PickWinner` fallthrough

If `parse_gmail_query` fails to extract subject keywords from a filter's `query`
field, `all_subject_keywords` stays empty and the group falls to `PickWinner`
even when filters have distinct subjects. Semantically different filters get
treated as identical.

## Design

### Fix 1: `extract_from_pattern` helper

Add a helper that mirrors `parse_from_key`'s fallback logic but returns a
`FilterRule`-compatible from-pattern string:

```rust
fn extract_from_pattern(filter: &ExistingFilterInfo) -> (Option<String>, bool) {
    if let Some(ref from) = filter.from {
        let is_specific = from.contains('@');
        return (Some(from.clone()), is_specific);
    }
    if let Some(ref query) = filter.query {
        let expr = parse_gmail_query(query);
        if let Some(ref from_clause) = expr.from_clause {
            return match from_clause {
                FromClause::Domain(dp) => (Some(format!("*@{}", dp.domain)), false),
                FromClause::SpecificSender(ep) => (Some(ep.full_address()), true),
            };
        }
    }
    (None, false)
}
```

Replace lines 548-552 in `classify_groups` with a call to this helper.

### Fix 2: Scope apply query via `reconstruct_filter_query`

Add a helper that builds a full Gmail search query from an `ExistingFilterInfo`:

```rust
fn reconstruct_filter_query(filter: &ExistingFilterInfo) -> String {
    // Prefer the raw query field — it's exactly what Gmail uses
    if let Some(ref query) = filter.query {
        if !query.is_empty() {
            return query.clone();
        }
    }
    // Fallback: build from individual fields
    let mut parts = Vec::new();
    if let Some(ref from) = filter.from {
        parts.push(format!("from:({})", from));
    }
    if let Some(ref subject) = filter.subject {
        if !subject.is_empty() {
            parts.push(format!("subject:({})", subject));
        }
    }
    parts.join(" ")
}
```

### Fix 3: Expand `LabelSwap` to carry full query

Replace `from_pattern: String` with `query: String` on `LabelSwap`. Change
`collect_swaps` to produce one `LabelSwap` per loser filter (not per group),
each scoped to the loser's full query via `reconstruct_filter_query`. This way
only emails that actually matched the loser filter get relabeled.

```rust
pub struct LabelSwap {
    pub query: String,
    pub add_label_id: String,
    pub remove_label_ids: Vec<String>,
}
```

Update `apply()` to use `swap.query` directly instead of building
`format!("from:{}", swap.from_pattern)`.

### Fix 4: Defensive subject extraction

In the subject-extraction loop (lines 503-527), add a raw regex fallback when
`parse_gmail_query` returns no subject clause: scan the `query` string for
`subject:` or `subject:(...)` patterns. This catches edge cases the parser
misses and prevents incorrect `PickWinner` fallthrough.

## Testing: proptest Property-Based Verification

`proptest` is already a dev-dependency. Use it to verify invariants across
hundreds of random filter configurations with automatic shrinking.

### Strategies

**Filter configuration axes:**

| Axis            | Values                                               |
|-----------------|------------------------------------------------------|
| from field      | `Some(specific_email)`, `Some(domain)`, `None`       |
| query field     | with from+subject, from only, subject only, `None`   |
| subject field   | `Some(keywords)`, `None`                             |
| label           | varies per filter                                    |
| archive         | `remove_label_ids` contains INBOX or not             |

**From location** (where from-criteria lives): `FromField`, `QueryField`, `Both`

Compose these into a `filter_config_strategy()` that produces
`ExistingFilterInfo` values, and an `overlap_group_strategy()` that produces
`Vec<ExistingFilterInfo>` of 2-5 filters sharing a domain.

### Properties

**P1: from_pattern never lost** — For every `ExistingFilterInfo` where
`parse_from_key` returns `Some(...)`, the corresponding `FilterRule` replacement
produced by `classify_groups` MUST have `from_pattern = Some(_)`.

**P2: apply query always includes from** — For every `LabelSwap` produced by
`collect_swaps`, the `query` field MUST contain a `from:` clause.

**P3: apply query includes subject when present** — If a loser filter has
subject criteria in any field, the `LabelSwap.query` MUST include that subject
restriction.

**P4: no cross-subject contamination** — Applying swaps for `from:X subject:Y`
must not affect emails matching `from:X subject:Z`.

**P5: MechanicalFix when subjects differ** — If a group has 2+ filters for the
same domain with at least one having subject criteria (in any field) and
different labels, `classify_groups` MUST produce `MechanicalFix`, not
`PickWinner`.

**P6: replacement from_pattern matches group** — Every `FilterRule` in a
`MechanicalFix.proposed_replacements` must have a `from_pattern` whose domain
matches `group.from_pattern`.

**P7: replacement mutual exclusivity** — For any two replacements in the same
`MechanicalFix` group: subject-specific filters have different keywords, and
the bare remainder filter excludes all subject-specific keywords.

**P8: PickWinner only when truly no subjects** — `PickWinner` is only reached
when NO filter in the group has subject criteria in ANY field.

**P9: archive isolation** — `LabelSwap.remove_label_ids` must never contain
`INBOX`. Archival is a filter-level action, not a label swap concern.

### What Does Not Change

- `OverlapDetector::detect` (the entry point that calls `list_filters`)
- `RemediationPlan::execute` / `execute_with_progress` (filter CRUD operations)
- `group_filters` / `parse_from_key` (grouping logic is correct)
- `FilterOverlapAnalyzer` in `filter_overlap.rs` (AST analysis layer)
- CLI and Tauri command surfaces
- The `Consolidate` resolution path
