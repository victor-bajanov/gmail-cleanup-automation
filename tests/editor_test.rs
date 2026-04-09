use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use gmail_automation::client::{ExistingFilterInfo, GmailClient, LabelInfo, ProgressCallback};
use gmail_automation::error::Result;
use gmail_automation::filter_editor::{apply, FilterAction};
use gmail_automation::models::FilterRule;
use gmail_automation::rate_limiter::QuotaStats;

struct MockEditorClient {
    deleted: Arc<Mutex<Vec<String>>>,
    created: Arc<Mutex<Vec<ExistingFilterInfo>>>,
    fail_ids: Vec<String>,
}

impl MockEditorClient {
    fn new() -> Self {
        Self {
            deleted: Arc::new(Mutex::new(vec![])),
            created: Arc::new(Mutex::new(vec![])),
            fail_ids: vec![],
        }
    }

    fn with_fail_ids(fail_ids: Vec<String>) -> Self {
        Self {
            deleted: Arc::new(Mutex::new(vec![])),
            created: Arc::new(Mutex::new(vec![])),
            fail_ids,
        }
    }
}

#[async_trait]
impl GmailClient for MockEditorClient {
    async fn delete_filter(&self, filter_id: &str) -> Result<()> {
        if self.fail_ids.contains(&filter_id.to_string()) {
            return Err(gmail_automation::error::GmailError::ApiError(format!(
                "Simulated failure for {}",
                filter_id
            )));
        }
        self.deleted.lock().unwrap().push(filter_id.to_string());
        Ok(())
    }

    async fn create_filter_from_info(&self, info: &ExistingFilterInfo) -> Result<String> {
        self.created.lock().unwrap().push(info.clone());
        Ok(format!("f_new_{}", info.id))
    }

    async fn list_message_ids(&self, _query: &str) -> Result<Vec<String>> {
        unimplemented!()
    }
    async fn get_message(
        &self,
        _id: &str,
    ) -> Result<gmail_automation::models::MessageMetadata> {
        unimplemented!()
    }
    async fn list_labels(&self) -> Result<Vec<LabelInfo>> {
        unimplemented!()
    }
    async fn create_label(&self, _name: &str) -> Result<String> {
        unimplemented!()
    }
    async fn delete_label(&self, _label_id: &str) -> Result<()> {
        unimplemented!()
    }
    async fn list_filters(&self) -> Result<Vec<ExistingFilterInfo>> {
        unimplemented!()
    }
    async fn create_filter(&self, _filter: &FilterRule) -> Result<String> {
        unimplemented!()
    }
    async fn update_filter(&self, _filter_id: &str, _filter: &FilterRule) -> Result<String> {
        unimplemented!()
    }
    async fn apply_label(&self, _message_id: &str, _label_id: &str) -> Result<()> {
        unimplemented!()
    }
    async fn remove_label(&self, _message_id: &str, _label_id: &str) -> Result<()> {
        unimplemented!()
    }
    async fn batch_remove_label(
        &self,
        _message_ids: &[String],
        _label_id: &str,
    ) -> Result<usize> {
        unimplemented!()
    }
    async fn batch_add_label(
        &self,
        _message_ids: &[String],
        _label_id: &str,
    ) -> Result<usize> {
        unimplemented!()
    }
    async fn batch_modify_labels(
        &self,
        _message_ids: &[String],
        _add_label_ids: &[String],
        _remove_label_ids: &[String],
    ) -> Result<usize> {
        unimplemented!()
    }
    async fn fetch_messages_batch(
        &self,
        _message_ids: Vec<String>,
    ) -> Result<Vec<gmail_automation::models::MessageMetadata>> {
        unimplemented!()
    }
    async fn fetch_messages_with_progress(
        &self,
        _message_ids: Vec<String>,
        _on_progress: ProgressCallback,
    ) -> Result<Vec<gmail_automation::models::MessageMetadata>> {
        unimplemented!()
    }
    async fn quota_stats(&self) -> QuotaStats {
        unimplemented!()
    }
}

fn make_filter(id: &str, query: &str, labels: Vec<&str>, archive: bool) -> ExistingFilterInfo {
    ExistingFilterInfo {
        id: id.to_string(),
        query: Some(query.to_string()),
        from: None,
        to: None,
        subject: None,
        add_label_ids: labels.into_iter().map(|s| s.to_string()).collect(),
        remove_label_ids: if archive {
            vec!["INBOX".to_string()]
        } else {
            vec![]
        },
    }
}

#[tokio::test]
async fn test_apply_archive_toggle() {
    let client = MockEditorClient::new();
    let filters = vec![make_filter(
        "f1",
        "from:(*@github.com)",
        vec!["lbl_gh"],
        false,
    )];
    let actions = vec![FilterAction::UpdateArchive {
        filter_id: "f1".into(),
        new_value: true,
    }];

    let result = apply(&client, &actions, &filters).await;

    assert_eq!(result.succeeded, 1);
    assert!(result.failed.is_empty());

    let deleted = client.deleted.lock().unwrap();
    assert_eq!(deleted.len(), 1);
    assert_eq!(deleted[0], "f1");

    let created = client.created.lock().unwrap();
    assert_eq!(created.len(), 1);
    assert!(
        created[0].remove_label_ids.contains(&"INBOX".to_string()),
        "New filter should have INBOX in remove_label_ids"
    );
}

#[tokio::test]
async fn test_apply_label_update() {
    let client = MockEditorClient::new();
    let filters = vec![make_filter(
        "f1",
        "from:(*@github.com)",
        vec!["lbl_gh", "lbl_old"],
        false,
    )];
    let actions = vec![FilterAction::UpdateLabels {
        filter_id: "f1".into(),
        add: vec!["lbl_new".into()],
        remove: vec!["lbl_old".into()],
    }];

    let result = apply(&client, &actions, &filters).await;

    assert_eq!(result.succeeded, 1);
    assert!(result.failed.is_empty());

    let deleted = client.deleted.lock().unwrap();
    assert_eq!(deleted.len(), 1);
    assert_eq!(deleted[0], "f1");

    let created = client.created.lock().unwrap();
    assert_eq!(created.len(), 1);
    assert_eq!(created[0].add_label_ids, vec!["lbl_gh", "lbl_new"]);
}

#[tokio::test]
async fn test_apply_delete() {
    let client = MockEditorClient::new();
    let filters = vec![make_filter(
        "f1",
        "from:(*@github.com)",
        vec!["lbl_gh"],
        false,
    )];
    let actions = vec![FilterAction::Delete {
        filter_id: "f1".into(),
    }];

    let result = apply(&client, &actions, &filters).await;

    assert_eq!(result.succeeded, 1);
    assert!(result.failed.is_empty());

    let deleted = client.deleted.lock().unwrap();
    assert_eq!(deleted.len(), 1);
    assert_eq!(deleted[0], "f1");

    let created = client.created.lock().unwrap();
    assert!(created.is_empty(), "Delete should not create a new filter");
}

#[tokio::test]
async fn test_apply_partial_failure() {
    let client = MockEditorClient::with_fail_ids(vec!["f2".into()]);
    let filters = vec![
        make_filter("f1", "from:(*@a.com)", vec!["lbl_a"], false),
        make_filter("f2", "from:(*@b.com)", vec!["lbl_b"], false),
    ];
    let actions = vec![
        FilterAction::Delete {
            filter_id: "f1".into(),
        },
        FilterAction::Delete {
            filter_id: "f2".into(),
        },
    ];

    let result = apply(&client, &actions, &filters).await;

    assert_eq!(result.succeeded, 1);
    assert_eq!(result.failed.len(), 1);
    assert_eq!(result.failed[0].0, "f2");
}
