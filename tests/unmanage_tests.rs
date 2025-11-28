//! Tests for the unmanage command functionality
//!
//! These tests verify the case-insensitive prefix matching logic used to identify
//! auto-managed filters and labels for deletion.

mod common;

use std::collections::HashMap;

use common::{create_test_existing_filter, create_test_label_info};

/// Helper function that replicates the filter identification logic from main.rs
/// This is the core logic we want to test
fn find_filters_to_delete(
    existing_filters: &[gmail_automation::client::ExistingFilterInfo],
    label_id_to_name: &HashMap<String, String>,
    label_prefix: &str,
) -> Vec<(String, Option<String>, String)> {
    let prefix_lower = label_prefix.to_lowercase();
    let mut filters_to_delete = Vec::new();

    for filter in existing_filters {
        for label_id in &filter.add_label_ids {
            if let Some(label_name) = label_id_to_name.get(label_id) {
                if label_name.to_lowercase().starts_with(&prefix_lower) {
                    filters_to_delete.push((
                        filter.id.clone(),
                        filter.query.clone(),
                        label_name.clone(),
                    ));
                    break;
                }
            }
        }
    }

    filters_to_delete
}

/// Helper function that replicates the label identification logic from main.rs
fn find_labels_to_delete<'a>(
    existing_labels: &'a [gmail_automation::client::LabelInfo],
    label_prefix: &str,
) -> Vec<&'a gmail_automation::client::LabelInfo> {
    let prefix_lower = label_prefix.to_lowercase();
    existing_labels
        .iter()
        .filter(|l| l.name.to_lowercase().starts_with(&prefix_lower))
        .collect()
}

// ============================================================================
// Unit Tests for Case-Insensitive Prefix Matching
// ============================================================================

#[test]
fn test_prefix_matching_exact_case() {
    // Config prefix matches exactly with label name
    let labels = vec![
        create_test_label_info("label-1", "AutoManaged/Newsletters"),
        create_test_label_info("label-2", "Personal"),
    ];

    let result = find_labels_to_delete(&labels, "AutoManaged");

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].name, "AutoManaged/Newsletters");
}

#[test]
fn test_prefix_matching_title_case_mismatch() {
    // This is the bug case: config has "AutoManaged" but Gmail stores "Automanaged"
    let labels = vec![
        create_test_label_info("label-1", "Automanaged/Newsletters"),
        create_test_label_info("label-2", "Automanaged/Receipts/Amazon"),
        create_test_label_info("label-3", "Personal"),
    ];

    let result = find_labels_to_delete(&labels, "AutoManaged");

    assert_eq!(result.len(), 2);
    assert!(result.iter().any(|l| l.name == "Automanaged/Newsletters"));
    assert!(result.iter().any(|l| l.name == "Automanaged/Receipts/Amazon"));
}

#[test]
fn test_prefix_matching_all_lowercase() {
    let labels = vec![
        create_test_label_info("label-1", "automanaged/newsletters"),
        create_test_label_info("label-2", "Personal"),
    ];

    let result = find_labels_to_delete(&labels, "AutoManaged");

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].name, "automanaged/newsletters");
}

#[test]
fn test_prefix_matching_all_uppercase() {
    let labels = vec![
        create_test_label_info("label-1", "AUTOMANAGED/NEWSLETTERS"),
        create_test_label_info("label-2", "Personal"),
    ];

    let result = find_labels_to_delete(&labels, "AutoManaged");

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].name, "AUTOMANAGED/NEWSLETTERS");
}

#[test]
fn test_prefix_matching_mixed_case_config() {
    // Config has weird casing, should still match
    let labels = vec![
        create_test_label_info("label-1", "Automanaged/Newsletters"),
        create_test_label_info("label-2", "Personal"),
    ];

    let result = find_labels_to_delete(&labels, "aUtOmAnAgEd");

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].name, "Automanaged/Newsletters");
}

#[test]
fn test_prefix_matching_no_matches() {
    let labels = vec![
        create_test_label_info("label-1", "Personal"),
        create_test_label_info("label-2", "Work"),
        create_test_label_info("label-3", "OtherPrefix/Something"),
    ];

    let result = find_labels_to_delete(&labels, "AutoManaged");

    assert_eq!(result.len(), 0);
}

#[test]
fn test_prefix_matching_partial_match_rejected() {
    // "Auto" should not match "Automanaged" as a prefix
    // But "Automanaged" should not match "Auto" prefix
    let labels = vec![
        create_test_label_info("label-1", "Automanaged/Newsletters"),
        create_test_label_info("label-2", "Automatic/Tasks"),
    ];

    // "Auto" prefix should match both
    let result = find_labels_to_delete(&labels, "Auto");
    assert_eq!(result.len(), 2);

    // "Automanaged" prefix should only match the first
    let result2 = find_labels_to_delete(&labels, "Automanaged");
    assert_eq!(result2.len(), 1);
    assert_eq!(result2[0].name, "Automanaged/Newsletters");
}

#[test]
fn test_prefix_matching_empty_prefix() {
    let labels = vec![
        create_test_label_info("label-1", "Automanaged/Newsletters"),
        create_test_label_info("label-2", "Personal"),
    ];

    // Empty prefix matches everything
    let result = find_labels_to_delete(&labels, "");

    assert_eq!(result.len(), 2);
}

// ============================================================================
// Filter Identification Tests
// ============================================================================

#[test]
fn test_filter_identification_with_case_mismatch() {
    let filters = vec![
        create_test_existing_filter("filter-1", Some("from:(*@github.com)"), vec!["label-1"]),
        create_test_existing_filter("filter-2", Some("from:(*@linkedin.com)"), vec!["label-2"]),
        create_test_existing_filter("filter-3", Some("from:(*@personal.com)"), vec!["label-3"]),
    ];

    let mut label_id_to_name = HashMap::new();
    // Note: Gmail stores as "Automanaged" (title case), not "AutoManaged"
    label_id_to_name.insert("label-1".to_string(), "Automanaged/Notifications/Github".to_string());
    label_id_to_name.insert("label-2".to_string(), "Automanaged/Social/Linkedin".to_string());
    label_id_to_name.insert("label-3".to_string(), "Personal".to_string());

    // Config has "AutoManaged" but labels are "Automanaged"
    let result = find_filters_to_delete(&filters, &label_id_to_name, "AutoManaged");

    assert_eq!(result.len(), 2);
    assert!(result.iter().any(|(id, _, _)| id == "filter-1"));
    assert!(result.iter().any(|(id, _, _)| id == "filter-2"));
    // filter-3 should NOT be included (Personal label)
    assert!(!result.iter().any(|(id, _, _)| id == "filter-3"));
}

#[test]
fn test_filter_identification_multiple_labels() {
    // Filter with multiple labels, only one matches
    let filters = vec![create_test_existing_filter(
        "filter-1",
        Some("from:(*@example.com)"),
        vec!["label-1", "label-2"],
    )];

    let mut label_id_to_name = HashMap::new();
    label_id_to_name.insert("label-1".to_string(), "Personal".to_string());
    label_id_to_name.insert("label-2".to_string(), "Automanaged/Other".to_string());

    let result = find_filters_to_delete(&filters, &label_id_to_name, "AutoManaged");

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].0, "filter-1");
    // Should capture the matching label name
    assert_eq!(result[0].2, "Automanaged/Other");
}

#[test]
fn test_filter_identification_unknown_label() {
    // Filter references a label ID that's not in our mapping
    let filters = vec![create_test_existing_filter(
        "filter-1",
        Some("from:(*@example.com)"),
        vec!["unknown-label-id"],
    )];

    let label_id_to_name = HashMap::new(); // Empty mapping

    let result = find_filters_to_delete(&filters, &label_id_to_name, "AutoManaged");

    assert_eq!(result.len(), 0);
}

#[test]
fn test_filter_identification_no_labels() {
    // Filter with no add_label_ids
    let filters = vec![create_test_existing_filter(
        "filter-1",
        Some("from:(*@example.com)"),
        vec![],
    )];

    let label_id_to_name = HashMap::new();

    let result = find_filters_to_delete(&filters, &label_id_to_name, "AutoManaged");

    assert_eq!(result.len(), 0);
}

#[test]
fn test_filter_preserves_query_info() {
    let filters = vec![create_test_existing_filter(
        "filter-1",
        Some("from:(*@github.com) -from:(noreply@github.com)"),
        vec!["label-1"],
    )];

    let mut label_id_to_name = HashMap::new();
    label_id_to_name.insert("label-1".to_string(), "Automanaged/Github".to_string());

    let result = find_filters_to_delete(&filters, &label_id_to_name, "AutoManaged");

    assert_eq!(result.len(), 1);
    assert_eq!(
        result[0].1,
        Some("from:(*@github.com) -from:(noreply@github.com)".to_string())
    );
}

// ============================================================================
// Integration-style Tests
// ============================================================================

#[test]
fn test_real_world_scenario() {
    // Simulate a real-world scenario where:
    // - Config has prefix "AutoManaged"
    // - Gmail sanitizes to "Automanaged"
    // - Multiple filters and labels exist

    let labels = vec![
        create_test_label_info("label-1", "Automanaged"),
        create_test_label_info("label-2", "Automanaged/Newsletters"),
        create_test_label_info("label-3", "Automanaged/Newsletters/Tech"),
        create_test_label_info("label-4", "Automanaged/Receipts"),
        create_test_label_info("label-5", "INBOX"),
        create_test_label_info("label-6", "Personal"),
        create_test_label_info("label-7", "Work/Projects"),
    ];

    let filters = vec![
        create_test_existing_filter("f1", Some("from:(*@techcrunch.com)"), vec!["label-3"]),
        create_test_existing_filter("f2", Some("from:(*@amazon.com)"), vec!["label-4"]),
        create_test_existing_filter("f3", Some("from:(friend@personal.com)"), vec!["label-6"]),
    ];

    let label_id_to_name: HashMap<String, String> = labels
        .iter()
        .map(|l| (l.id.clone(), l.name.clone()))
        .collect();

    // Find labels to delete
    let labels_to_delete = find_labels_to_delete(&labels, "AutoManaged");
    assert_eq!(labels_to_delete.len(), 4); // All Automanaged/* labels

    // Find filters to delete
    let filters_to_delete = find_filters_to_delete(&filters, &label_id_to_name, "AutoManaged");
    assert_eq!(filters_to_delete.len(), 2); // f1 and f2, but not f3

    // Verify correct filters identified
    let filter_ids: Vec<&str> = filters_to_delete.iter().map(|(id, _, _)| id.as_str()).collect();
    assert!(filter_ids.contains(&"f1"));
    assert!(filter_ids.contains(&"f2"));
    assert!(!filter_ids.contains(&"f3"));
}

#[test]
fn test_custom_prefix() {
    // Test with a non-default prefix
    let labels = vec![
        create_test_label_info("label-1", "MyCustomPrefix/Category1"),
        create_test_label_info("label-2", "mycustomprefix/category2"),
        create_test_label_info("label-3", "MYCUSTOMPREFIX/CATEGORY3"),
        create_test_label_info("label-4", "OtherPrefix/Something"),
    ];

    let result = find_labels_to_delete(&labels, "MyCustomPrefix");

    assert_eq!(result.len(), 3);
    // Should not include OtherPrefix
    assert!(!result.iter().any(|l| l.name.contains("OtherPrefix")));
}
