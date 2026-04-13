# Remediation UI TODOs

## Bugs

- ~~Label names still not shown in remediation view~~ — Fixed: label cache preloaded async on client init, with defensive warming in detect_overlaps
- ~~Overlap detection falsely flags filters with `-subject:` exclusions~~ — Fixed: added `subject_exclusions_resolve_overlap` to analyzer, handles both SubjectMatchMode::Any and ::All
