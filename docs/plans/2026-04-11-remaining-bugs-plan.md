# Remaining Bugs Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Fix 3 bugs: apply_filters missing label creation, parse_gmail_query false positives, Exclude Forever not persisting.

**Architecture:** Extract shared label-creation pipeline into `label_manager.rs`. Add `QueryParser` trait + regex-based implementation in `filter_overlap.rs`. Wire `ExclusionManager` into Tauri `AppState`.

**Tech Stack:** Rust, Tauri, Gmail API, regex crate, parking_lot::RwLock

---

## Task 1: Shared Label Creation Pipeline — Tests

**Files:**
- Modify: `src/label_manager.rs` (add tests at bottom)

**Step 1: Write failing tests for `create_labels_and_resolve_ids`**

Add these tests at the bottom of `src/label_manager.rs` inside the existing `#[cfg(test)] mod tests` block:

```rust
#[tokio::test]
async fn test_create_labels_and_resolve_ids_creates_missing() {
    use crate::filter_manager::FilterRule;
    
    let mut mock = MockGmailClient::new();
    // list_labels returns empty (no existing labels)
    mock.labels = vec![];
    // create_label will return sequential IDs
    mock.next_label_id = std::sync::atomic::AtomicUsize::new(1);

    let mut label_manager = LabelManager::new(Box::new(mock), "AutoManaged".to_string());
    
    let filters = vec![
        FilterRule {
            id: None,
            name: "test".to_string(),
            from_pattern: Some("*@example.com".to_string()),
            is_specific_sender: false,
            excluded_senders: vec![],
            subject_keywords: vec![],
            excluded_subject_patterns: vec![],
            target_label_id: "AutoManaged/Other/example.com".to_string(),
            should_archive: false,
            estimated_matches: 10,
        },
    ];

    let (name_to_id, stats) = create_labels_and_resolve_ids(
        &mut label_manager, &filters, false
    ).await.unwrap();

    assert_eq!(stats.created, 1);
    assert!(name_to_id.contains_key("automanaged/other/example.com"));
}

#[tokio::test]
async fn test_create_labels_and_resolve_ids_skips_existing() {
    use crate::filter_manager::FilterRule;
    
    let mut mock = MockGmailClient::new();
    mock.labels = vec![
        crate::client::LabelInfo {
            id: "Label_existing".to_string(),
            name: "AutoManaged/Other/example.com".to_string(),
        },
    ];

    let mut label_manager = LabelManager::new(Box::new(mock), "AutoManaged".to_string());
    
    let filters = vec![
        FilterRule {
            id: None,
            name: "test".to_string(),
            from_pattern: Some("*@example.com".to_string()),
            is_specific_sender: false,
            excluded_senders: vec![],
            subject_keywords: vec![],
            excluded_subject_patterns: vec![],
            target_label_id: "AutoManaged/Other/example.com".to_string(),
            should_archive: false,
            estimated_matches: 10,
        },
    ];

    let (name_to_id, stats) = create_labels_and_resolve_ids(
        &mut label_manager, &filters, false
    ).await.unwrap();

    assert_eq!(stats.created, 0);
    assert_eq!(stats.skipped, 1);
    assert_eq!(name_to_id.get("automanaged/other/example.com").unwrap(), "Label_existing");
}

#[tokio::test]
async fn test_create_labels_and_resolve_ids_dry_run() {
    use crate::filter_manager::FilterRule;
    
    let mut mock = MockGmailClient::new();
    mock.labels = vec![];

    let mut label_manager = LabelManager::new(Box::new(mock), "AutoManaged".to_string());
    
    let filters = vec![
        FilterRule {
            id: None,
            name: "test".to_string(),
            from_pattern: Some("*@example.com".to_string()),
            is_specific_sender: false,
            excluded_senders: vec![],
            subject_keywords: vec![],
            excluded_subject_patterns: vec![],
            target_label_id: "AutoManaged/Other/example.com".to_string(),
            should_archive: false,
            estimated_matches: 10,
        },
    ];

    let (name_to_id, stats) = create_labels_and_resolve_ids(
        &mut label_manager, &filters, true
    ).await.unwrap();

    // Dry run: nothing actually created, but stats reflect what would happen
    assert_eq!(stats.created, 1);
    assert!(name_to_id.is_empty()); // No real IDs in dry run
}

#[tokio::test]
async fn test_create_labels_deduplicates() {
    use crate::filter_manager::FilterRule;
    
    let mut mock = MockGmailClient::new();
    mock.labels = vec![];
    mock.next_label_id = std::sync::atomic::AtomicUsize::new(1);

    let mut label_manager = LabelManager::new(Box::new(mock), "AutoManaged".to_string());
    
    // Two filters targeting the same label
    let filters = vec![
        FilterRule {
            id: None,
            name: "filter1".to_string(),
            from_pattern: Some("*@a.com".to_string()),
            is_specific_sender: false,
            excluded_senders: vec![],
            subject_keywords: vec![],
            excluded_subject_patterns: vec![],
            target_label_id: "AutoManaged/Other/example.com".to_string(),
            should_archive: false,
            estimated_matches: 5,
        },
        FilterRule {
            id: None,
            name: "filter2".to_string(),
            from_pattern: Some("*@b.com".to_string()),
            is_specific_sender: false,
            excluded_senders: vec![],
            subject_keywords: vec![],
            excluded_subject_patterns: vec![],
            target_label_id: "AutoManaged/Other/example.com".to_string(),
            should_archive: false,
            estimated_matches: 3,
        },
    ];

    let (_name_to_id, stats) = create_labels_and_resolve_ids(
        &mut label_manager, &filters, false
    ).await.unwrap();

    // Should only create one label despite two filters
    assert_eq!(stats.created, 1);
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test create_labels_and_resolve_ids -- --nocapture 2>&1 | head -30`
Expected: compilation error — `create_labels_and_resolve_ids` doesn't exist yet. Also check if `MockGmailClient` exists and has the needed fields; if not, adapt the tests to use whatever mock exists.

**Step 3: Commit**

```
git add src/label_manager.rs
git commit -m "test: add failing tests for create_labels_and_resolve_ids"
```

---

## Task 2: Shared Label Creation Pipeline — Implementation

**Files:**
- Modify: `src/label_manager.rs` (add function + struct near top, after LabelManager impl)

**Step 1: Add `LabelCreationStats` struct and `create_labels_and_resolve_ids` function**

Add after the `LabelManager` impl block but before tests:

```rust
/// Stats from label creation
#[derive(Debug, Default)]
pub struct LabelCreationStats {
    pub created: usize,
    pub skipped: usize,
}

/// Creates missing Gmail labels and builds a name→ID map for filter creation.
///
/// This is the shared pipeline used by both CLI and GUI:
/// 1. Collects unique label names from filters
/// 2. Loads existing labels into cache
/// 3. Creates missing labels (unless dry_run)
/// 4. Returns name→ID map (lowercase keys) for resolving filter target_label_id
pub async fn create_labels_and_resolve_ids(
    label_manager: &mut LabelManager,
    filters: &[crate::filter_manager::FilterRule],
    dry_run: bool,
) -> crate::error::Result<(std::collections::HashMap<String, String>, LabelCreationStats)> {
    use std::collections::{HashMap, HashSet};

    // Collect unique label names from filters
    let unique_labels: HashSet<String> = filters
        .iter()
        .map(|f| f.target_label_id.clone())
        .collect();

    // Load existing labels into cache
    label_manager.load_existing_labels().await?;

    let mut name_to_id: HashMap<String, String> = HashMap::new();
    let mut stats = LabelCreationStats::default();

    for label in &unique_labels {
        let sanitized = label_manager.sanitize_label_name(label).unwrap_or_default();

        // Check if label already exists in cache (case-insensitive)
        if let Some(existing_id) = label_manager.get_label_id(&sanitized) {
            stats.skipped += 1;
            name_to_id.insert(label.to_lowercase(), existing_id);
            continue;
        }

        if !dry_run {
            let label_id = label_manager.create_label_direct(&sanitized).await?;
            name_to_id.insert(label.to_lowercase(), label_id);
        }
        stats.created += 1;
    }

    Ok((name_to_id, stats))
}
```

**Step 2: Run tests to verify they pass**

Run: `cargo test create_labels_and_resolve_ids -- --nocapture 2>&1 | tail -20`
Expected: all 4 tests pass. If MockGmailClient needed adjustments in Task 1, those carry forward.

**Step 3: Commit**

```
git add src/label_manager.rs
git commit -m "feat: add shared create_labels_and_resolve_ids pipeline"
```

---

## Task 3: Wire Shared Pipeline into GUI apply_filters

**Files:**
- Modify: `src-tauri/src/commands/filters.rs` (update `apply_filters` function at line 315)

**Step 1: Update `apply_filters` to create labels and resolve IDs before creating filters**

Replace the body of `apply_filters` (lines 315-378) with:

```rust
#[tauri::command]
pub async fn apply_filters(
    app: AppHandle,
    dry_run: bool,
    state: State<'_, AppState>,
) -> Result<ApplyFiltersResult, String> {
    let client = state.get_client().ok_or("Gmail client not initialized")?;
    let proposed = state.get_proposed_filters();

    if proposed.is_empty() {
        return Ok(ApplyFiltersResult {
            success: true,
            created: 0,
            failed: 0,
            dry_run,
            errors: vec![],
        });
    }

    // Step 1: Create labels and resolve IDs
    let config = state.get_config().unwrap_or_default();
    let client_for_labels = std::sync::Arc::clone(&client);
    let mut label_manager = gmail_automation::LabelManager::new(
        Box::new(client_for_labels),
        config.classification.label_prefix.clone(),
    );

    let (name_to_id, label_stats) = gmail_automation::label_manager::create_labels_and_resolve_ids(
        &mut label_manager,
        &proposed,
        dry_run,
    ).await.map_err(|e| format!("Failed to create labels: {}", e))?;

    tracing::info!(
        "Labels: {} created, {} skipped",
        label_stats.created,
        label_stats.skipped
    );

    // Populate label cache in AppState
    {
        let mut cache = state.label_cache.write();
        cache.extend(name_to_id.clone());
    }

    // Step 2: Create filters with resolved label IDs
    let total = proposed.len();
    let mut created = 0;
    let mut errors = Vec::new();

    let client_clone = std::sync::Arc::clone(&client);
    let mut manager = gmail_automation::FilterManager::new(Box::new(client_clone));

    for (i, filter) in proposed.iter().enumerate() {
        app.events().emit_filter_progress(FilterProgress {
            operation: FilterOperation::Creating,
            current: i + 1,
            total,
            filter_name: Some(filter.name.clone()),
        });

        if dry_run {
            tracing::info!("Dry run: would create filter {}", filter.name);
            created += 1;
        } else {
            // Resolve label name to Gmail ID
            let label_id = match name_to_id.get(&filter.target_label_id.to_lowercase()) {
                Some(id) => id.clone(),
                None => {
                    errors.push(format!(
                        "Label ID not found for '{}' in filter '{}'",
                        filter.target_label_id, filter.name
                    ));
                    continue;
                }
            };

            let mut resolved_filter = filter.clone();
            resolved_filter.target_label_id = label_id;

            match manager.create_filter(&resolved_filter).await {
                Ok(_) => created += 1,
                Err(e) => errors.push(format!("Failed to create '{}': {}", filter.name, e)),
            }
        }
    }

    app.events().emit_filter_progress(FilterProgress {
        operation: FilterOperation::Complete,
        current: total,
        total,
        filter_name: None,
    });

    Ok(ApplyFiltersResult {
        success: errors.is_empty(),
        created,
        failed: errors.len(),
        dry_run,
        errors,
    })
}
```

**Step 2: Verify it compiles**

Run: `cargo check -p gmail-cleanup-gui 2>&1 | tail -20`
Expected: compiles. Fix any import issues (may need `use gmail_automation::label_manager::create_labels_and_resolve_ids;` and ensure `LabelManager`, `FilterManager`, `LabelCreationStats` are exported from the lib).

**Step 3: Commit**

```
git add src-tauri/src/commands/filters.rs
git commit -m "fix: apply_filters now creates labels and resolves IDs before creating Gmail filters"
```

---

## Task 4: QueryParser Trait + Display Name Handling — Tests

**Files:**
- Modify: `src/filter_ast.rs` (add `FromClause::DisplayName` variant)
- Modify: `src/filter_overlap.rs` (add tests)

**Step 1: Add `FromClause::DisplayName` variant**

In `src/filter_ast.rs`, add to the `FromClause` enum:

```rust
pub enum FromClause {
    Domain(DomainPattern),
    SpecificSender(EmailPattern),
    MultipleSenders(Vec<FromClause>),
    DisplayName(String),  // Opaque display name — treated as Disjoint with everything
}
```

Update any `match` arms on `FromClause` throughout the codebase to handle `DisplayName`. In overlap analysis (`compare_from_clauses` or similar), `DisplayName` should return `PatternRelation::Disjoint` against everything.

**Step 2: Add `excluded_subjects` field to `FilterExpr`**

In `src/filter_ast.rs`, add to the `FilterExpr` struct:

```rust
pub struct FilterExpr {
    pub from_clause: Option<FromClause>,
    pub subject_clause: Option<SubjectClause>,
    pub exclusions: Vec<ExclusionClause>,
    pub subject_exclusions: Vec<String>,  // -subject:() keywords
}
```

Update `FilterExpr::new()` to initialize `subject_exclusions: vec![]`.

**Step 3: Write failing tests for the new parser behavior**

Add to the test module in `src/filter_overlap.rs`:

```rust
#[test]
fn test_parse_gmail_query_display_name() {
    // Display name without @ should produce DisplayName variant, not None
    let expr = parse_gmail_query("from:(iiNET Support)");
    assert!(expr.from_clause.is_some(), "Display name should not produce None from_clause");
    match expr.from_clause.as_ref().unwrap() {
        FromClause::DisplayName(name) => assert_eq!(name, "iiNET Support"),
        other => panic!("Expected DisplayName, got {:?}", other),
    }
}

#[test]
fn test_parse_gmail_query_negative_subject() {
    // -subject:() should be parsed as subject exclusion, not positive subject
    let expr = parse_gmail_query("from:(*@example.com) -subject:(Unsubscribe)");
    assert!(expr.subject_clause.is_none(), "-subject should not be parsed as positive subject");
    assert_eq!(expr.subject_exclusions, vec!["Unsubscribe"]);
}

#[test]
fn test_parse_gmail_query_negative_subject_not_confused_with_positive() {
    // A query with ONLY -subject:() should not have a positive subject clause
    let expr = parse_gmail_query("from:(*@example.com) -subject:(Promo OR Sale)");
    assert!(expr.subject_clause.is_none());
    assert_eq!(expr.subject_exclusions, vec!["Promo", "Sale"]);
}

#[test]
fn test_parse_gmail_query_both_positive_and_negative_subject() {
    let expr = parse_gmail_query("from:(*@example.com) subject:(Invoice) -subject:(Draft)");
    assert!(expr.subject_clause.is_some());
    assert_eq!(expr.subject_clause.unwrap().keywords, vec!["Invoice"]);
    assert_eq!(expr.subject_exclusions, vec!["Draft"]);
}

#[test]
fn test_display_name_overlap_is_disjoint() {
    // Two display-name filters should be disjoint with domain filters
    let mut a = FilterExpr::new();
    a.from_clause = Some(FromClause::DisplayName("iiNET Support".to_string()));
    let mut b = FilterExpr::new();
    b.from_clause = Some(FromClause::Domain(DomainPattern::new("iinet.com")));
    
    let relation = compare_from_clauses(a.from_clause.as_ref(), b.from_clause.as_ref());
    assert_eq!(relation, PatternRelation::Disjoint);
}

#[test]
fn test_parse_gmail_query_or_with_display_names() {
    // OR list mixing emails and display names
    let expr = parse_gmail_query("from:(user@example.com OR iiNET Support)");
    match expr.from_clause.as_ref().unwrap() {
        FromClause::MultipleSenders(senders) => {
            assert_eq!(senders.len(), 2);
            assert!(matches!(&senders[0], FromClause::SpecificSender(_)));
            assert!(matches!(&senders[1], FromClause::DisplayName(_)));
        }
        other => panic!("Expected MultipleSenders, got {:?}", other),
    }
}
```

**Step 4: Run tests to verify they fail**

Run: `cargo test -p gmail-automation test_parse_gmail_query_display_name test_parse_gmail_query_negative_subject test_display_name_overlap -- --nocapture 2>&1 | tail -30`
Expected: compilation errors or test failures.

**Step 5: Commit**

```
git add src/filter_ast.rs src/filter_overlap.rs
git commit -m "test: add failing tests for display names, -subject parsing, QueryParser trait"
```

---

## Task 5: QueryParser Trait + Implementation

**Files:**
- Modify: `src/filter_overlap.rs` (add trait, refactor `parse_gmail_query`)

**Step 1: Add `QueryParser` trait and `RegexQueryParser`**

Add before the existing `parse_gmail_query` function:

```rust
/// Trait for parsing Gmail filter queries into FilterExpr ASTs.
/// Implementations can be swapped for testing or to upgrade the parser.
pub trait QueryParser: Send + Sync {
    fn parse(&self, query: &str) -> FilterExpr;
}

/// Regex-based query parser. Handles:
/// - from:() with domains, emails, display names, OR groups
/// - -from:() exclusions
/// - subject:() positive keywords
/// - -subject:() negative keywords
pub struct RegexQueryParser;

impl QueryParser for RegexQueryParser {
    fn parse(&self, query: &str) -> FilterExpr {
        parse_gmail_query_impl(query)
    }
}
```

**Step 2: Refactor `parse_gmail_query` to delegate to the new implementation**

Rename the current `parse_gmail_query` body to `parse_gmail_query_impl` and rewrite it with the regex approach:

```rust
/// Parses a Gmail filter query into a FilterExpr.
/// This is the public API — delegates to RegexQueryParser.
pub fn parse_gmail_query(query: &str) -> FilterExpr {
    parse_gmail_query_impl(query)
}

fn parse_gmail_query_impl(query: &str) -> FilterExpr {
    let mut expr = FilterExpr::new();

    // Use regex to find all field:(content) patterns with optional - prefix
    // We process them in order of appearance
    let field_re = regex::Regex::new(r"(-?)(from|subject|to):\(([^)]*)\)").unwrap();
    
    for caps in field_re.captures_iter(query) {
        let is_negative = &caps[1] == "-";
        let field = &caps[2];
        let content = caps[3].trim();
        
        match (field, is_negative) {
            ("from", false) => {
                expr.from_clause = parse_from_content(content);
            }
            ("from", true) => {
                // Parse exclusion - could be email or display name
                if let Some(clause) = parse_single_from(content) {
                    expr.exclusions.push(crate::filter_ast::ExclusionClause {
                        pattern: clause,
                    });
                }
            }
            ("subject", false) => {
                let keywords: Vec<String> = content
                    .split(" OR ")
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                if !keywords.is_empty() {
                    expr.subject_clause = Some(SubjectClause {
                        keywords,
                        match_mode: crate::filter_ast::SubjectMatchMode::Any,
                    });
                }
            }
            ("subject", true) => {
                let keywords: Vec<String> = content
                    .split(" OR ")
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                expr.subject_exclusions.extend(keywords);
            }
            _ => {} // to: or unknown — ignore for now
        }
    }

    expr
}

/// Parse the content inside from:(...) — handles OR groups, domains, emails, display names
fn parse_from_content(content: &str) -> Option<FromClause> {
    if content.contains(" OR ") {
        let parts: Vec<FromClause> = content
            .split(" OR ")
            .filter_map(|part| parse_single_from(part.trim()))
            .collect();
        match parts.len() {
            0 => None,
            1 => Some(parts.into_iter().next().unwrap()),
            _ => Some(FromClause::MultipleSenders(parts)),
        }
    } else {
        parse_single_from(content)
    }
}

/// Parse a single from pattern: domain (*@domain), email (user@domain), or display name
fn parse_single_from(s: &str) -> Option<FromClause> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if s.starts_with("*@") {
        Some(FromClause::Domain(DomainPattern::new(s.trim_start_matches("*@"))))
    } else if s.contains('@') {
        EmailPattern::parse(s).map(FromClause::SpecificSender)
    } else {
        // Display name (no @) — treat as opaque
        Some(FromClause::DisplayName(s.to_string()))
    }
}
```

**Step 3: Update `compare_from_clauses` to handle `DisplayName`**

Find the function that compares `FromClause` variants (likely `compare_from_clauses` or within `analyze_pair`). Add a match arm:

```rust
// DisplayName is always Disjoint — we can't determine overlap with other types
(FromClause::DisplayName(_), _) | (_, FromClause::DisplayName(_)) => PatternRelation::Disjoint,
```

**Step 4: Update `to_gmail_query` to handle `DisplayName`**

In the `to_gmail_query` function, add a match arm for `DisplayName`:

```rust
FromClause::DisplayName(name) => {
    parts.push(format!("from:({})", name));
}
```

Also add serialization for `subject_exclusions`:

```rust
for kw in &expr.subject_exclusions {
    parts.push(format!("-subject:({})", kw));
}
```

**Step 5: Run all tests**

Run: `cargo test -p gmail-automation -- --nocapture 2>&1 | tail -40`
Expected: all existing tests pass + all new tests pass. If any existing test breaks, fix the issue (likely a missing match arm on `FromClause`).

**Step 6: Commit**

```
git add src/filter_overlap.rs src/filter_ast.rs
git commit -m "feat: regex-based query parser with QueryParser trait, display name + -subject support"
```

---

## Task 6: Exclude Forever Persistence — Tests

**Files:**
- Modify: `src-tauri/src/commands/clusters.rs` (add tests or manual verification steps)

**Step 1: Write test for exclusion integration**

Since Tauri commands are hard to unit test (they require `AppHandle` and `State`), this task focuses on verifying the integration points manually. However, add a unit test for the cluster key derivation logic:

Add a helper function and test in `src-tauri/src/commands/clusters.rs`:

```rust
/// Derives the exclusion key for a cluster (same format as CLI)
fn cluster_exclusion_key(cluster: &EmailCluster) -> String {
    let base = if cluster.is_specific_sender {
        cluster.sender_email.clone()
    } else {
        format!("*@{}", cluster.sender_domain)
    };
    if let Some(subject) = &cluster.subject_pattern {
        format!("{}|subject:{}", base, subject)
    } else {
        base
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gmail_automation::interactive::EmailCluster;
    use gmail_automation::EmailCategory;

    fn make_cluster(domain: &str, email: &str, is_specific: bool, subject: Option<&str>) -> EmailCluster {
        EmailCluster {
            sender_domain: domain.to_string(),
            sender_email: email.to_string(),
            is_specific_sender: is_specific,
            excluded_senders: vec![],
            subject_pattern: subject.map(|s| s.to_string()),
            excluded_subject_patterns: vec![],
            message_ids: vec![],
            suggested_category: EmailCategory::Other,
            suggested_label: String::new(),
            confidence: 0.0,
        }
    }

    #[test]
    fn test_cluster_exclusion_key_domain() {
        let cluster = make_cluster("example.com", "", false, None);
        assert_eq!(cluster_exclusion_key(&cluster), "*@example.com");
    }

    #[test]
    fn test_cluster_exclusion_key_specific_sender() {
        let cluster = make_cluster("example.com", "user@example.com", true, None);
        assert_eq!(cluster_exclusion_key(&cluster), "user@example.com");
    }

    #[test]
    fn test_cluster_exclusion_key_with_subject() {
        let cluster = make_cluster("example.com", "", false, Some("Newsletter"));
        assert_eq!(cluster_exclusion_key(&cluster), "*@example.com|subject:Newsletter");
    }
}
```

**Step 2: Run tests**

Run: `cargo test -p gmail-cleanup-gui cluster_exclusion_key -- --nocapture 2>&1 | tail -20`
Expected: compilation error — `cluster_exclusion_key` doesn't exist yet, but the test structure is ready. Note: check the actual Tauri package name — it might be different. Use `cargo test cluster_exclusion_key` if unsure.

**Step 3: Commit**

```
git add src-tauri/src/commands/clusters.rs
git commit -m "test: add tests for cluster exclusion key derivation"
```

---

## Task 7: Exclude Forever Persistence — Implementation

**Files:**
- Modify: `src-tauri/src/state.rs` (add `exclusion_manager` field)
- Modify: `src-tauri/src/commands/clusters.rs` (wire up exclusions)

**Step 1: Add `ExclusionManager` to `AppState`**

In `src-tauri/src/state.rs`:

Add to imports:
```rust
use gmail_automation::exclusions::ExclusionManager;
```

Add field to `AppState` struct (after `remediation_decisions`):
```rust
/// Persistent exclusion manager
pub exclusion_manager: RwLock<ExclusionManager>,
```

In `AppState::new()`, load from disk. Add after `hidden_filters_data` loading:
```rust
let exclusions_path = gmail_dir.join("exclusions.json");
let exclusion_manager = ExclusionManager::load_sync(&exclusions_path)
    .unwrap_or_else(|e| {
        tracing::warn!("Failed to load exclusions: {}, starting fresh", e);
        ExclusionManager::new()
    });
tracing::debug!("Loaded {} exclusions from disk", exclusion_manager.len());
```

Add to `Self { ... }` initializer:
```rust
exclusion_manager: RwLock::new(exclusion_manager),
```

**Step 2: Wire exclusions into `submit_cluster_decision`**

In `src-tauri/src/commands/clusters.rs`, in the `submit_cluster_decision` function, after the `GuiDecision` is added to state, add:

```rust
// Persist exclusion to disk if action is Exclude
if matches!(action, DecisionAction::Exclude) {
    let clusters = state.get_clusters();
    if let Some(cluster) = clusters.get(input.cluster_index) {
        let key = cluster_exclusion_key(cluster);
        let mut em = state.exclusion_manager.write();
        em.add(key, Some("Excluded via GUI".to_string()));
        
        // Save to disk — derive path from state
        let gmail_dir = state.credentials_path()
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| std::path::PathBuf::from(".gmail-automation"));
        let exclusions_path = gmail_dir.join("exclusions.json");
        if let Err(e) = em.save_sync(&exclusions_path) {
            tracing::error!("Failed to save exclusions: {}", e);
        }
    }
}
```

**Step 3: Filter excluded clusters in `get_clusters`**

In the `get_clusters` function, after getting clusters from state, filter out excluded ones:

```rust
let em = state.exclusion_manager.read();
let clusters: Vec<_> = clusters
    .into_iter()
    .filter(|c| !em.is_excluded(&cluster_exclusion_key(c)))
    .collect();
```

Note: `cluster_exclusion_key` needs to be accessible here — it was defined in Task 6 in the same file.

**Step 4: Run tests and verify compilation**

Run: `cargo test -p gmail-cleanup-gui -- --nocapture 2>&1 | tail -20` and `cargo check -p gmail-cleanup-gui 2>&1 | tail -20`
Expected: all tests pass, compiles cleanly.

**Step 5: Commit**

```
git add src-tauri/src/state.rs src-tauri/src/commands/clusters.rs
git commit -m "fix: Exclude Forever now persists via ExclusionManager"
```

---

## Task 8: Final Verification

**Step 1: Run full test suite**

Run: `cargo test 2>&1 | tail -30`
Expected: all tests pass.

**Step 2: Check compilation of both packages**

Run: `cargo check 2>&1 | tail -10`
Expected: clean build.

**Step 3: Commit any remaining fixes**

If any test failures or compilation issues were found and fixed, commit them.
