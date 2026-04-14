# Remediation UI TODOs

## Bugs

- ~~Label names still not shown in remediation view~~ — Fixed: label cache preloaded async on client init, with defensive warming in detect_overlaps
- ~~Overlap detection falsely flags filters with `-subject:` exclusions~~ — Fixed: added `subject_exclusions_resolve_overlap` to analyzer, handles both SubjectMatchMode::Any and ::All
- ~~False overlap grouping (domain-only clustering)~~ — Fixed: union-find grouping with pairwise `analyze_expr_relation`
- ~~Filters without from_clause bridging all groups into one~~ — Fixed: skip non-analyzable filters in pairwise comparison
- ~~Duplicate group_ids causing wrong decision applied to wrong group~~ — Fixed: numeric suffix disambiguation
- ~~Decision counter stuck at 0/30~~ — Fixed: replaced `createMemo` with plain functions in SolidJS store
- Gmail API returns 500 "Internal error encountered" on filter deletes — needs investigation. All filter IDs confirmed to exist via API. `with_retry` already retries 3x with backoff. Could be malformed request, invalid scope, or genuine transient errors. Check: request payload, auth scope used (`gmail.settings.basic`), whether `should_retry` handles 500s correctly.
- 0 Created despite 17 Deleted — replacement filter creation is skipped when any delete in the group fails (`"Skipping replacement creation due to delete failures"`). Needs investigation alongside the 500 errors above — the skip logic may be correct if deletes are genuinely failing.
