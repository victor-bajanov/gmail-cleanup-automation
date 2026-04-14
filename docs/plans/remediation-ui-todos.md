# Remediation UI TODOs

## Bugs

- ~~Label names still not shown in remediation view~~ — Fixed: label cache preloaded async on client init, with defensive warming in detect_overlaps
- ~~Overlap detection falsely flags filters with `-subject:` exclusions~~ — Fixed: added `subject_exclusions_resolve_overlap` to analyzer, handles both SubjectMatchMode::Any and ::All
- ~~False overlap grouping (domain-only clustering)~~ — Fixed: union-find grouping with pairwise `analyze_expr_relation`
- ~~Filters without from_clause bridging all groups into one~~ — Fixed: skip non-analyzable filters in pairwise comparison
- ~~Duplicate group_ids causing wrong decision applied to wrong group~~ — Fixed: numeric suffix disambiguation
- ~~Decision counter stuck at 0/30~~ — Fixed: replaced `createMemo` with plain functions in SolidJS store
- Gmail API returns 500 "Internal error encountered" on filter deletes — transient Google backend errors. Need retry with exponential backoff on 500s during remediation execution.
- 0 Created despite 17 Deleted — replacement filter creation is skipped when any delete in the group fails (`"Skipping replacement creation due to delete failures"`). This is overly conservative; should create replacements for successfully deleted filters, or retry failed deletes before giving up.
