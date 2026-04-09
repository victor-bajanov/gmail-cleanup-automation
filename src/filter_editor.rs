use serde::{Deserialize, Serialize};

use crate::client::{ExistingFilterInfo, GmailClient};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FilterAction {
    UpdateArchive { filter_id: String, new_value: bool },
    UpdateLabels { filter_id: String, add: Vec<String>, remove: Vec<String> },
    Delete { filter_id: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionDiff {
    pub filter_id: String,
    pub description: String,
    pub action_type: String,
    pub changes: Vec<FieldChange>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldChange {
    pub field: String,
    pub before: String,
    pub after: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditorApplyResult {
    pub succeeded: usize,
    pub failed: Vec<(String, String)>,
}

const INBOX_LABEL: &str = "INBOX";

pub fn dry_run(actions: &[FilterAction], filters: &[ExistingFilterInfo]) -> Vec<ActionDiff> {
    actions
        .iter()
        .filter_map(|action| match action {
            FilterAction::UpdateArchive {
                filter_id,
                new_value,
            } => {
                let filter = find_filter(filters, filter_id)?;
                let currently_archived = has_archive(filter);
                if currently_archived == *new_value {
                    return None; // no-op
                }
                Some(ActionDiff {
                    filter_id: filter_id.clone(),
                    description: describe_filter(filter),
                    action_type: "update".into(),
                    changes: vec![FieldChange {
                        field: "archive".into(),
                        before: if currently_archived { "ON" } else { "OFF" }.into(),
                        after: if *new_value { "ON" } else { "OFF" }.into(),
                    }],
                })
            }
            FilterAction::UpdateLabels {
                filter_id,
                add,
                remove,
            } => {
                let filter = find_filter(filters, filter_id)?;
                let before_labels = filter.add_label_ids.clone();
                let mut after_labels: Vec<String> = before_labels
                    .iter()
                    .filter(|l| !remove.contains(l))
                    .cloned()
                    .collect();
                for label in add {
                    if !after_labels.contains(label) {
                        after_labels.push(label.clone());
                    }
                }
                if before_labels == after_labels {
                    return None; // no-op
                }
                Some(ActionDiff {
                    filter_id: filter_id.clone(),
                    description: describe_filter(filter),
                    action_type: "update".into(),
                    changes: vec![FieldChange {
                        field: "labels".into(),
                        before: before_labels.join(", "),
                        after: after_labels.join(", "),
                    }],
                })
            }
            FilterAction::Delete { filter_id } => {
                let filter = find_filter(filters, filter_id)?;
                Some(ActionDiff {
                    filter_id: filter_id.clone(),
                    description: describe_filter(filter),
                    action_type: "delete".into(),
                    changes: vec![],
                })
            }
        })
        .collect()
}

pub async fn apply(
    client: &dyn GmailClient,
    actions: &[FilterAction],
    filters: &[ExistingFilterInfo],
) -> EditorApplyResult {
    let mut succeeded = 0;
    let mut failed: Vec<(String, String)> = Vec::new();

    for action in actions {
        let result = apply_single(client, action, filters).await;
        match result {
            Ok(()) => succeeded += 1,
            Err(e) => {
                let filter_id = match action {
                    FilterAction::UpdateArchive { filter_id, .. } => filter_id,
                    FilterAction::UpdateLabels { filter_id, .. } => filter_id,
                    FilterAction::Delete { filter_id } => filter_id,
                };
                failed.push((filter_id.clone(), e));
            }
        }
    }

    EditorApplyResult { succeeded, failed }
}

async fn apply_single(
    client: &dyn GmailClient,
    action: &FilterAction,
    filters: &[ExistingFilterInfo],
) -> std::result::Result<(), String> {
    match action {
        FilterAction::UpdateArchive {
            filter_id,
            new_value,
        } => {
            let filter = find_filter(filters, filter_id)
                .ok_or_else(|| format!("Filter {} not found", filter_id))?;
            let mut updated = filter.clone();
            if *new_value {
                if !updated.remove_label_ids.contains(&INBOX_LABEL.to_string()) {
                    updated.remove_label_ids.push(INBOX_LABEL.to_string());
                }
            } else {
                updated.remove_label_ids.retain(|l| l != INBOX_LABEL);
            }
            client
                .delete_filter(filter_id)
                .await
                .map_err(|e| e.to_string())?;
            client
                .create_filter_from_info(&updated)
                .await
                .map_err(|e| e.to_string())?;
            Ok(())
        }
        FilterAction::UpdateLabels {
            filter_id,
            add,
            remove,
        } => {
            let filter = find_filter(filters, filter_id)
                .ok_or_else(|| format!("Filter {} not found", filter_id))?;
            let mut updated = filter.clone();
            updated.add_label_ids.retain(|l| !remove.contains(l));
            for label in add {
                if !updated.add_label_ids.contains(label) {
                    updated.add_label_ids.push(label.clone());
                }
            }
            client
                .delete_filter(filter_id)
                .await
                .map_err(|e| e.to_string())?;
            client
                .create_filter_from_info(&updated)
                .await
                .map_err(|e| e.to_string())?;
            Ok(())
        }
        FilterAction::Delete { filter_id } => {
            client
                .delete_filter(filter_id)
                .await
                .map_err(|e| e.to_string())?;
            Ok(())
        }
    }
}

fn find_filter<'a>(filters: &'a [ExistingFilterInfo], id: &str) -> Option<&'a ExistingFilterInfo> {
    filters.iter().find(|f| f.id == id)
}

fn describe_filter(filter: &ExistingFilterInfo) -> String {
    filter
        .query
        .clone()
        .or_else(|| filter.from.clone())
        .unwrap_or_else(|| filter.id.clone())
}

fn has_archive(filter: &ExistingFilterInfo) -> bool {
    filter.remove_label_ids.contains(&INBOX_LABEL.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_filter(id: &str, query: &str, labels: Vec<&str>, archive: bool) -> ExistingFilterInfo {
        ExistingFilterInfo {
            id: id.to_string(),
            query: Some(query.to_string()),
            from: None,
            to: None,
            subject: None,
            add_label_ids: labels.into_iter().map(|s| s.to_string()).collect(),
            remove_label_ids: if archive { vec!["INBOX".to_string()] } else { vec![] },
        }
    }

    #[test]
    fn test_dry_run_archive_toggle_on() {
        let filters = vec![make_filter("f1", "from:(*@github.com)", vec!["lbl_gh"], false)];
        let actions = vec![FilterAction::UpdateArchive { filter_id: "f1".into(), new_value: true }];
        let diffs = dry_run(&actions, &filters);
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].filter_id, "f1");
        assert_eq!(diffs[0].action_type, "update");
        assert_eq!(diffs[0].changes[0].field, "archive");
        assert_eq!(diffs[0].changes[0].before, "OFF");
        assert_eq!(diffs[0].changes[0].after, "ON");
    }

    #[test]
    fn test_dry_run_archive_toggle_off() {
        let filters = vec![make_filter("f1", "from:(*@github.com)", vec!["lbl_gh"], true)];
        let actions = vec![FilterAction::UpdateArchive { filter_id: "f1".into(), new_value: false }];
        let diffs = dry_run(&actions, &filters);
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].changes[0].before, "ON");
        assert_eq!(diffs[0].changes[0].after, "OFF");
    }

    #[test]
    fn test_dry_run_label_update() {
        let filters = vec![make_filter("f1", "from:(*@github.com)", vec!["lbl_gh", "lbl_old"], false)];
        let actions = vec![FilterAction::UpdateLabels {
            filter_id: "f1".into(),
            add: vec!["lbl_new".into()],
            remove: vec!["lbl_old".into()],
        }];
        let diffs = dry_run(&actions, &filters);
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].changes[0].field, "labels");
        assert_eq!(diffs[0].changes[0].before, "lbl_gh, lbl_old");
        assert_eq!(diffs[0].changes[0].after, "lbl_gh, lbl_new");
    }

    #[test]
    fn test_dry_run_delete() {
        let filters = vec![make_filter("f1", "from:(*@github.com)", vec!["lbl_gh"], false)];
        let actions = vec![FilterAction::Delete { filter_id: "f1".into() }];
        let diffs = dry_run(&actions, &filters);
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].action_type, "delete");
        assert!(diffs[0].changes.is_empty());
    }

    #[test]
    fn test_dry_run_nonexistent_filter() {
        let filters = vec![make_filter("f1", "from:(*@github.com)", vec!["lbl_gh"], false)];
        let actions = vec![FilterAction::Delete { filter_id: "f_nonexistent".into() }];
        let diffs = dry_run(&actions, &filters);
        assert_eq!(diffs.len(), 0);
    }

    #[test]
    fn test_dry_run_empty_queue() {
        let filters = vec![make_filter("f1", "from:(*@github.com)", vec!["lbl_gh"], false)];
        let diffs = dry_run(&[], &filters);
        assert!(diffs.is_empty());
    }

    #[test]
    fn test_dry_run_noop_archive_already_on() {
        let filters = vec![make_filter("f1", "from:(*@github.com)", vec!["lbl_gh"], true)];
        let actions = vec![FilterAction::UpdateArchive { filter_id: "f1".into(), new_value: true }];
        let diffs = dry_run(&actions, &filters);
        assert_eq!(diffs.len(), 0);
    }
}
