use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use gmail_automation::client::{ExistingFilterInfo, GmailClient, LabelInfo, ProgressCallback};
use gmail_automation::error::Result;
use gmail_automation::filter_remediation::{
    GroupDecision, OverlapDetector, RemediationPlan, ResolutionType,
};
use gmail_automation::models::FilterRule;
use gmail_automation::rate_limiter::QuotaStats;

struct MockRemediationClient {
    initial_filters: Vec<ExistingFilterInfo>,
    deleted: Arc<Mutex<Vec<String>>>,
    created: Arc<Mutex<Vec<FilterRule>>>,
    create_counter: Arc<Mutex<usize>>,
}

impl MockRemediationClient {
    fn new(filters: Vec<ExistingFilterInfo>) -> Self {
        Self {
            initial_filters: filters,
            deleted: Arc::new(Mutex::new(vec![])),
            created: Arc::new(Mutex::new(vec![])),
            create_counter: Arc::new(Mutex::new(0)),
        }
    }
}

#[async_trait]
impl GmailClient for MockRemediationClient {
    async fn list_filters(&self) -> Result<Vec<ExistingFilterInfo>> {
        Ok(self.initial_filters.clone())
    }

    async fn delete_filter(&self, filter_id: &str) -> Result<()> {
        self.deleted.lock().unwrap().push(filter_id.to_string());
        Ok(())
    }

    async fn create_filter(&self, filter: &FilterRule) -> Result<String> {
        let mut counter = self.create_counter.lock().unwrap();
        *counter += 1;
        let id = format!("f_new_{}", counter);
        self.created.lock().unwrap().push(filter.clone());
        Ok(id)
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
    async fn update_filter(
        &self,
        _filter_id: &str,
        _filter: &FilterRule,
    ) -> Result<String> {
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

#[tokio::test]
async fn test_full_remediation_roundtrip() {
    let filters = vec![
        ExistingFilterInfo {
            id: "f1".into(),
            query: None,
            from: Some("cba.com.au".into()),
            to: None,
            subject: Some("statement".into()),
            add_label_ids: vec!["lbl_fin".into()],
            remove_label_ids: vec![],
        },
        ExistingFilterInfo {
            id: "f2".into(),
            query: None,
            from: Some("cba.com.au".into()),
            to: None,
            subject: None,
            add_label_ids: vec!["lbl_oth".into()],
            remove_label_ids: vec![],
        },
    ];

    let client = MockRemediationClient::new(filters);
    let label_map: HashMap<String, String> = [
        ("lbl_fin".into(), "Financial".into()),
        ("lbl_oth".into(), "Other".into()),
    ]
    .into_iter()
    .collect();

    // Phase 1: Detect overlapping filter groups
    let groups = OverlapDetector::detect(&client, &label_map)
        .await
        .unwrap();
    assert_eq!(groups.len(), 1, "Should detect exactly one overlap group");
    assert_eq!(groups[0].group_id, "cba.com.au");

    // Phase 2: Decide — accept the mechanical fix for each group
    let mut plan = RemediationPlan::new();
    for group in groups {
        if let ResolutionType::MechanicalFix {
            ref proposed_replacements,
        } = group.resolution_type
        {
            plan.add(
                group.clone(),
                GroupDecision::ReplaceWithExclusive {
                    replacement_filters: proposed_replacements.clone(),
                },
            );
        } else {
            panic!("Expected MechanicalFix for cba.com.au group");
        }
    }

    // Phase 3: Execute the remediation plan
    let result = plan.execute(&client).await.unwrap();
    assert_eq!(result.deleted.len(), 2, "Both original filters should be deleted");
    assert!(
        result.created.len() >= 1,
        "At least one replacement filter should be created"
    );
    assert!(
        result.errors.is_empty(),
        "Execution should complete without errors: {:?}",
        result.errors
    );

    // Phase 4: Verify the replacement filters have correct exclusions
    let created_filters = client.created.lock().unwrap();
    let remainder = created_filters
        .iter()
        .find(|f| f.subject_keywords.is_empty())
        .expect("Should have a remainder filter (no subject keywords)");
    assert!(
        remainder
            .excluded_subject_patterns
            .contains(&"statement".to_string()),
        "Remainder filter should exclude 'statement', got: {:?}",
        remainder.excluded_subject_patterns
    );

    // Also verify the specific filter retained its subject keyword
    let specific = created_filters
        .iter()
        .find(|f| !f.subject_keywords.is_empty())
        .expect("Should have a specific subject filter");
    assert!(
        specific.subject_keywords.contains(&"statement".to_string()),
        "Specific filter should have 'statement' keyword"
    );
}
