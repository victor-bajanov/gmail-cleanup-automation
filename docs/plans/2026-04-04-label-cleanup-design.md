# Label Cleanup TUI

## Problem

After buggy remediation filters applied dozens of AutoManaged labels to emails,
the filters have been fixed but existing emails still carry 3+ AutoManaged
labels. Need a fast per-email triage TUI to pick the correct label (or remove
all) for each affected email.

## Design

### CLI subcommand

`gmail-cleanup label-cleanup` — new subcommand in `src/main.rs`.

### Scan phase

1. `list_labels()` -> filter to names starting with `AutoManaged/`
   (case-insensitive).
2. For each auto label, `list_message_ids("label:<id>")` -> build
   `HashMap<message_id, Vec<(label_id, label_name)>>`.
3. Filter to entries with 3+ auto labels.
4. Batch-fetch message metadata (subject, from, date) for surviving messages.
5. Sort by sender then date for natural grouping during review.

### TUI layout

Two-pane layout using `crossterm` raw mode:

```
┌─ Email 14/847 ───────────────────────────────────────┬─ Status ──────────┐
│                                                       │ ✓ 11 applied      │
│  From: noreply@cba.com.au                             │ ⏳ 2 in flight     │
│  Subject: Your statement is ready                     │ ✗ 0 failed        │
│  Date: 2026-03-15                                     │                   │
│                                                       │ Last: ✓ kept      │
│  Labels:                                              │   Financial       │
│   [1] Financial                                       │                   │
│   [2] Receipts                                        │                   │
│   [3] Receipts/amazon-com                             │                   │
│   [4] Other                                           │                   │
│   [5] Banking                                         │                   │
│   [6] Banking/statements                              │                   │
│   [7] Notifications                                   │                   │
│   [8] Alerts                                          │                   │
│   [9] Personal                                        │                   │
│   [a] Updates                                         │                   │
│   [b] Promotions                                      │                   │
│                                                       │                   │
│  [0] Remove all  [s] Skip  [u] Undo  [q] Quit        │                   │
└───────────────────────────────────────────────────────┴───────────────────┘
```

Label display: strip the `AutoManaged/` prefix, show everything below it
(e.g. `Receipts/amazon-com`).

### Key bindings

| Key   | Action                                            |
|-------|---------------------------------------------------|
| `1-9` | Keep that label (remove rest)                     |
| `a-z` | For labels 10-35 (a=10, b=11, ...)                |
| `0`   | Remove ALL auto labels from this email            |
| `s`   | Skip this email, move to next                     |
| `u`   | Undo last action (revert label change, go back)   |
| `q`   | Quit (in-flight ops finish, pending abandoned)     |

### Apply: immediate + async

Each keypress spawns an async task via `tokio::spawn` that calls
`batch_modify_labels` for that single email. The TUI immediately advances to
the next email. The status pane updates as tasks complete.

### Undo

Keep a stack of `(message_id, added_labels, removed_labels)`. On `u`, spawn the
inverse `batch_modify_labels` (re-add removed, remove added) and go back one
email in the list.

### Error handling

- Failed applies show in the status pane.
- On `q`, wait for in-flight ops to finish ("waiting for N ops...").
- Print summary of failures at exit so user can retry.

### What it doesn't do

- Touch filters (already fixed independently).
- Touch non-AutoManaged labels.
- Archive or unarchive.
- Group or batch emails — pure per-email triage.
