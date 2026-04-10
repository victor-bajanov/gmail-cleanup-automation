//! AST data structures for filter expressions
//!
//! This module provides Abstract Syntax Tree (AST) representations of Gmail filter
//! criteria, enabling static analysis of filter relationships without requiring
//! example emails.
//!
//! # Overview
//!
//! Gmail filters consist of criteria (FROM, SUBJECT, etc.) and actions (LABEL, ARCHIVE, etc.).
//! This module models the criteria portion as an AST that can be analyzed for:
//! - Overlap detection (two filters matching the same emails)
//! - Redundancy detection (one filter completely covers another)
//! - Conflict detection (same emails routed to different labels)
//!
//! # Example
//!
//! ```
//! use gmail_automation::filter_ast::{FilterExpr, FromClause, DomainPattern};
//!
//! // Create a filter that matches all emails from github.com
//! let github_filter = FilterExpr {
//!     from_clause: Some(FromClause::Domain(DomainPattern {
//!         domain: "github.com".to_string(),
//!         include_subdomains: false,
//!     })),
//!     subject_clause: None,
//!     exclusions: vec![],
//! };
//! ```

use serde::{Deserialize, Serialize};

/// Root AST node representing a complete filter expression
///
/// A filter expression combines multiple clauses (FROM, SUBJECT, etc.) with
/// implicit AND semantics. An email must match ALL non-None clauses to be
/// captured by the filter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FilterExpr {
    /// FROM clause - matches sender patterns
    pub from_clause: Option<FromClause>,
    /// SUBJECT clause - matches subject line patterns
    pub subject_clause: Option<SubjectClause>,
    /// Exclusion clauses - patterns to explicitly exclude
    pub exclusions: Vec<ExclusionClause>,
}

impl FilterExpr {
    /// Creates a new empty filter expression
    pub fn new() -> Self {
        Self {
            from_clause: None,
            subject_clause: None,
            exclusions: vec![],
        }
    }

    /// Creates a filter expression matching a domain
    pub fn from_domain(domain: impl Into<String>) -> Self {
        Self {
            from_clause: Some(FromClause::Domain(DomainPattern {
                domain: domain.into(),
                include_subdomains: false,
            })),
            subject_clause: None,
            exclusions: vec![],
        }
    }

    /// Creates a filter expression matching a specific sender
    pub fn from_sender(email: impl Into<String>) -> Self {
        let email_str = email.into();
        let parts: Vec<&str> = email_str.split('@').collect();
        let (local_part, domain) = if parts.len() == 2 {
            (parts[0].to_string(), parts[1].to_string())
        } else {
            (email_str.clone(), String::new())
        };

        Self {
            from_clause: Some(FromClause::SpecificSender(EmailPattern {
                local_part,
                domain,
            })),
            subject_clause: None,
            exclusions: vec![],
        }
    }

    /// Adds an exclusion for a specific sender
    pub fn with_exclusion(mut self, email: impl Into<String>) -> Self {
        let email_str = email.into();
        let parts: Vec<&str> = email_str.split('@').collect();
        let (local_part, domain) = if parts.len() == 2 {
            (parts[0].to_string(), parts[1].to_string())
        } else {
            (email_str.clone(), String::new())
        };

        self.exclusions.push(ExclusionClause {
            pattern: FromClause::SpecificSender(EmailPattern { local_part, domain }),
        });
        self
    }

    /// Adds a subject keyword requirement
    pub fn with_subject_keywords(mut self, keywords: Vec<String>, match_mode: SubjectMatchMode) -> Self {
        self.subject_clause = Some(SubjectClause {
            keywords,
            match_mode,
        });
        self
    }

    /// Checks if this filter has any criteria
    pub fn is_empty(&self) -> bool {
        self.from_clause.is_none() && self.subject_clause.is_none()
    }

    /// Returns a human-readable description of the filter
    pub fn describe(&self) -> String {
        let mut parts = Vec::new();

        if let Some(ref from) = self.from_clause {
            parts.push(format!("FROM: {}", from.describe()));
        }

        if let Some(ref subject) = self.subject_clause {
            parts.push(format!("SUBJECT: {}", subject.describe()));
        }

        for exclusion in &self.exclusions {
            parts.push(format!("EXCLUDE: {}", exclusion.pattern.describe()));
        }

        if parts.is_empty() {
            "Empty filter".to_string()
        } else {
            parts.join(" AND ")
        }
    }
}

impl Default for FilterExpr {
    fn default() -> Self {
        Self::new()
    }
}

/// FROM clause patterns - what sender patterns the filter matches
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FromClause {
    /// Matches all emails from a domain (e.g., *@github.com)
    Domain(DomainPattern),
    /// Matches emails from a specific sender (e.g., noreply@github.com)
    SpecificSender(EmailPattern),
    /// Matches emails from multiple senders (OR clause)
    MultipleSenders(Vec<FromClause>),
}

impl FromClause {
    /// Returns a human-readable description
    pub fn describe(&self) -> String {
        match self {
            FromClause::Domain(d) => d.describe(),
            FromClause::SpecificSender(e) => e.full_address(),
            FromClause::MultipleSenders(senders) => {
                senders.iter().map(|s| s.describe()).collect::<Vec<_>>().join(" OR ")
            }
        }
    }

    /// Extracts the domain from this clause
    pub fn domain(&self) -> &str {
        match self {
            FromClause::Domain(d) => &d.domain,
            FromClause::SpecificSender(e) => &e.domain,
            FromClause::MultipleSenders(senders) => {
                senders.first().map(|s| s.domain()).unwrap_or("")
            }
        }
    }
}

/// Domain pattern for matching entire domains
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DomainPattern {
    /// The domain to match (e.g., "github.com")
    pub domain: String,
    /// Whether to include subdomains (e.g., mail.github.com)
    pub include_subdomains: bool,
}

impl DomainPattern {
    /// Creates a new domain pattern
    pub fn new(domain: impl Into<String>) -> Self {
        Self {
            domain: domain.into(),
            include_subdomains: false,
        }
    }

    /// Creates a domain pattern that includes subdomains
    pub fn with_subdomains(domain: impl Into<String>) -> Self {
        Self {
            domain: domain.into(),
            include_subdomains: true,
        }
    }

    /// Returns a human-readable description
    pub fn describe(&self) -> String {
        if self.include_subdomains {
            format!("*@*.{}", self.domain)
        } else {
            format!("*@{}", self.domain)
        }
    }

    /// Checks if this domain pattern matches a given domain
    pub fn matches_domain(&self, other_domain: &str) -> bool {
        let self_lower = self.domain.to_lowercase();
        let other_lower = other_domain.to_lowercase();

        if self_lower == other_lower {
            return true;
        }

        if self.include_subdomains {
            // Check if other_domain is a subdomain of self.domain
            other_lower.ends_with(&format!(".{}", self_lower))
        } else {
            false
        }
    }
}

/// Email pattern for matching specific senders
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EmailPattern {
    /// The local part before @ (e.g., "noreply")
    pub local_part: String,
    /// The domain after @ (e.g., "github.com")
    pub domain: String,
}

impl EmailPattern {
    /// Creates a new email pattern
    pub fn new(local_part: impl Into<String>, domain: impl Into<String>) -> Self {
        Self {
            local_part: local_part.into(),
            domain: domain.into(),
        }
    }

    /// Parses an email pattern from a full email address
    pub fn parse(email: &str) -> Option<Self> {
        let parts: Vec<&str> = email.split('@').collect();
        if parts.len() == 2 {
            Some(Self {
                local_part: parts[0].to_string(),
                domain: parts[1].to_string(),
            })
        } else {
            None
        }
    }

    /// Returns the full email address
    pub fn full_address(&self) -> String {
        format!("{}@{}", self.local_part, self.domain)
    }

    /// Checks if this email pattern matches another email (case-insensitive)
    pub fn matches(&self, other: &EmailPattern) -> bool {
        self.local_part.eq_ignore_ascii_case(&other.local_part)
            && self.domain.eq_ignore_ascii_case(&other.domain)
    }
}

/// Subject clause for matching subject line patterns
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubjectClause {
    /// Keywords to match in the subject
    pub keywords: Vec<String>,
    /// How to combine keywords (ANY = OR, ALL = AND)
    pub match_mode: SubjectMatchMode,
}

impl SubjectClause {
    /// Creates a new subject clause matching any of the keywords
    pub fn any_of(keywords: Vec<String>) -> Self {
        Self {
            keywords,
            match_mode: SubjectMatchMode::Any,
        }
    }

    /// Creates a new subject clause matching all keywords
    pub fn all_of(keywords: Vec<String>) -> Self {
        Self {
            keywords,
            match_mode: SubjectMatchMode::All,
        }
    }

    /// Returns a human-readable description
    pub fn describe(&self) -> String {
        let separator = match self.match_mode {
            SubjectMatchMode::Any => " OR ",
            SubjectMatchMode::All => " AND ",
        };
        self.keywords.join(separator)
    }

    /// Checks if two subject clauses have any overlap
    pub fn overlaps_with(&self, other: &SubjectClause) -> bool {
        // If either uses ANY mode and shares keywords, there's potential overlap
        for kw in &self.keywords {
            let kw_lower = kw.to_lowercase();
            for other_kw in &other.keywords {
                if other_kw.to_lowercase() == kw_lower {
                    return true;
                }
            }
        }
        false
    }
}

/// How to combine multiple subject keywords
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SubjectMatchMode {
    /// Match if ANY keyword is present (OR)
    Any,
    /// Match only if ALL keywords are present (AND)
    All,
}

/// Exclusion clause - patterns to explicitly exclude from the filter
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExclusionClause {
    /// The pattern to exclude
    pub pattern: FromClause,
}

impl ExclusionClause {
    /// Creates an exclusion for a specific email address
    pub fn sender(email: &str) -> Option<Self> {
        EmailPattern::parse(email).map(|pattern| Self {
            pattern: FromClause::SpecificSender(pattern),
        })
    }

    /// Creates an exclusion for an entire domain
    pub fn domain(domain: impl Into<String>) -> Self {
        Self {
            pattern: FromClause::Domain(DomainPattern::new(domain)),
        }
    }
}

/// Filter actions - what to do when a filter matches
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FilterActions {
    /// Label to apply
    pub label: Option<String>,
    /// Whether to archive (skip inbox)
    pub archive: bool,
    /// Whether to mark as read
    pub mark_read: bool,
    /// Whether to star the message
    pub star: bool,
    /// Whether to delete (move to trash)
    pub delete: bool,
    /// Whether to mark as important
    pub important: Option<bool>,
    /// Whether to categorize (Primary, Social, Updates, Forums, Promotions)
    pub category: Option<String>,
}

impl FilterActions {
    /// Creates default actions (no-op)
    pub fn new() -> Self {
        Self {
            label: None,
            archive: false,
            mark_read: false,
            star: false,
            delete: false,
            important: None,
            category: None,
        }
    }

    /// Creates actions that apply a label
    pub fn with_label(label: impl Into<String>) -> Self {
        Self {
            label: Some(label.into()),
            ..Self::new()
        }
    }

    /// Creates actions that apply a label and archive
    pub fn label_and_archive(label: impl Into<String>) -> Self {
        Self {
            label: Some(label.into()),
            archive: true,
            ..Self::new()
        }
    }

    /// Checks if two action sets conflict
    pub fn conflicts_with(&self, other: &FilterActions) -> Option<ActionConflict> {
        // Different labels = conflict
        if let (Some(ref a), Some(ref b)) = (&self.label, &other.label) {
            if a != b {
                return Some(ActionConflict::DifferentLabels {
                    label_a: a.clone(),
                    label_b: b.clone(),
                });
            }
        }

        // Different archive settings = conflict
        if self.archive != other.archive {
            return Some(ActionConflict::DifferentArchive {
                archive_a: self.archive,
                archive_b: other.archive,
            });
        }

        // One deletes, other doesn't = conflict
        if self.delete != other.delete {
            return Some(ActionConflict::DifferentDelete {
                delete_a: self.delete,
                delete_b: other.delete,
            });
        }

        None
    }
}

impl Default for FilterActions {
    fn default() -> Self {
        Self::new()
    }
}

/// Types of action conflicts between filters
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ActionConflict {
    /// Filters apply different labels
    DifferentLabels { label_a: String, label_b: String },
    /// Filters have different archive settings
    DifferentArchive { archive_a: bool, archive_b: bool },
    /// Filters have different delete settings
    DifferentDelete { delete_a: bool, delete_b: bool },
}

impl ActionConflict {
    /// Returns a human-readable description of the conflict
    pub fn describe(&self) -> String {
        match self {
            ActionConflict::DifferentLabels { label_a, label_b } => {
                format!("Label conflict: '{}' vs '{}'", label_a, label_b)
            }
            ActionConflict::DifferentArchive { archive_a, archive_b } => {
                format!(
                    "Archive conflict: {} vs {}",
                    if *archive_a { "archive" } else { "keep in inbox" },
                    if *archive_b { "archive" } else { "keep in inbox" }
                )
            }
            ActionConflict::DifferentDelete { delete_a, delete_b } => {
                format!(
                    "Delete conflict: {} vs {}",
                    if *delete_a { "delete" } else { "keep" },
                    if *delete_b { "delete" } else { "keep" }
                )
            }
        }
    }
}

/// A complete filter with both expression (criteria) and actions
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Filter {
    /// Unique identifier
    pub id: String,
    /// Human-readable name
    pub name: String,
    /// Filter criteria (what emails to match)
    pub expr: FilterExpr,
    /// Filter actions (what to do with matches)
    pub actions: FilterActions,
}

impl Filter {
    /// Creates a new filter
    pub fn new(id: impl Into<String>, name: impl Into<String>, expr: FilterExpr, actions: FilterActions) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            expr,
            actions,
        }
    }

    /// Returns a human-readable description
    pub fn describe(&self) -> String {
        let mut desc = format!("[{}] {}\n", self.id, self.name);
        desc.push_str(&format!("  Criteria: {}\n", self.expr.describe()));
        if let Some(ref label) = self.actions.label {
            desc.push_str(&format!("  Label: {}\n", label));
        }
        if self.actions.archive {
            desc.push_str("  Archive: yes\n");
        }
        desc
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_domain_pattern_matches() {
        let pattern = DomainPattern::new("github.com");
        assert!(pattern.matches_domain("github.com"));
        assert!(pattern.matches_domain("GITHUB.COM"));
        assert!(!pattern.matches_domain("mail.github.com"));
        assert!(!pattern.matches_domain("notgithub.com"));

        let pattern_with_sub = DomainPattern::with_subdomains("github.com");
        assert!(pattern_with_sub.matches_domain("github.com"));
        assert!(pattern_with_sub.matches_domain("mail.github.com"));
        assert!(pattern_with_sub.matches_domain("notifications.mail.github.com"));
        assert!(!pattern_with_sub.matches_domain("notgithub.com"));
    }

    #[test]
    fn test_email_pattern_parse() {
        let pattern = EmailPattern::parse("noreply@github.com").unwrap();
        assert_eq!(pattern.local_part, "noreply");
        assert_eq!(pattern.domain, "github.com");
        assert_eq!(pattern.full_address(), "noreply@github.com");

        assert!(EmailPattern::parse("invalid").is_none());
    }

    #[test]
    fn test_filter_expr_builder() {
        let filter = FilterExpr::from_domain("github.com")
            .with_exclusion("bot@github.com")
            .with_subject_keywords(
                vec!["notification".to_string()],
                SubjectMatchMode::Any,
            );

        assert!(filter.from_clause.is_some());
        assert_eq!(filter.exclusions.len(), 1);
        assert!(filter.subject_clause.is_some());
    }

    #[test]
    fn test_action_conflicts() {
        let actions1 = FilterActions::with_label("Label1");
        let actions2 = FilterActions::with_label("Label2");

        let conflict = actions1.conflicts_with(&actions2);
        assert!(conflict.is_some());
        assert!(matches!(conflict.unwrap(), ActionConflict::DifferentLabels { .. }));

        let actions3 = FilterActions::label_and_archive("Label1");
        let actions4 = FilterActions::with_label("Label1");

        let conflict = actions3.conflicts_with(&actions4);
        assert!(conflict.is_some());
        assert!(matches!(conflict.unwrap(), ActionConflict::DifferentArchive { .. }));
    }

    #[test]
    fn test_subject_clause_overlap() {
        let clause1 = SubjectClause::any_of(vec!["newsletter".to_string(), "digest".to_string()]);
        let clause2 = SubjectClause::any_of(vec!["newsletter".to_string(), "update".to_string()]);
        let clause3 = SubjectClause::any_of(vec!["alert".to_string(), "warning".to_string()]);

        assert!(clause1.overlaps_with(&clause2)); // share "newsletter"
        assert!(!clause1.overlaps_with(&clause3)); // no common keywords
    }

    #[test]
    fn test_filter_describe() {
        let filter = Filter::new(
            "filter-123",
            "GitHub Notifications",
            FilterExpr::from_domain("github.com"),
            FilterActions::label_and_archive("AutoManaged/github"),
        );

        let description = filter.describe();
        assert!(description.contains("filter-123"));
        assert!(description.contains("GitHub Notifications"));
        assert!(description.contains("github.com"));
        assert!(description.contains("AutoManaged/github"));
    }

    #[test]
    fn test_empty_filter_expr() {
        let filter = FilterExpr::new();
        assert!(filter.is_empty());
        assert_eq!(filter.describe(), "Empty filter");
    }

    #[test]
    fn test_from_clause_domain_extraction() {
        let domain_clause = FromClause::Domain(DomainPattern::new("example.com"));
        assert_eq!(domain_clause.domain(), "example.com");

        let sender_clause = FromClause::SpecificSender(EmailPattern::new("test", "example.com"));
        assert_eq!(sender_clause.domain(), "example.com");
    }

    #[test]
    fn test_email_pattern_case_insensitive_match() {
        let pattern1 = EmailPattern::new("NoReply", "GitHub.COM");
        let pattern2 = EmailPattern::new("noreply", "github.com");
        assert!(pattern1.matches(&pattern2));
    }

    #[test]
    fn test_exclusion_clause_builders() {
        let sender_exclusion = ExclusionClause::sender("bot@github.com").unwrap();
        if let FromClause::SpecificSender(ref email) = sender_exclusion.pattern {
            assert_eq!(email.local_part, "bot");
            assert_eq!(email.domain, "github.com");
        } else {
            panic!("Expected SpecificSender");
        }

        let domain_exclusion = ExclusionClause::domain("spam.com");
        if let FromClause::Domain(ref domain) = domain_exclusion.pattern {
            assert_eq!(domain.domain, "spam.com");
        } else {
            panic!("Expected Domain");
        }

        assert!(ExclusionClause::sender("invalid").is_none());
    }

    #[test]
    fn test_filter_actions_delete_conflict() {
        let mut actions1 = FilterActions::new();
        actions1.delete = true;
        let actions2 = FilterActions::new();

        let conflict = actions1.conflicts_with(&actions2);
        assert!(conflict.is_some());
        assert!(matches!(conflict.unwrap(), ActionConflict::DifferentDelete { .. }));
    }

    #[test]
    fn test_filter_actions_no_conflict() {
        let actions1 = FilterActions::with_label("Label");
        let actions2 = FilterActions::with_label("Label");

        assert!(actions1.conflicts_with(&actions2).is_none());
    }

    #[test]
    fn test_subject_clause_describe() {
        let any_clause = SubjectClause::any_of(vec!["a".to_string(), "b".to_string()]);
        assert_eq!(any_clause.describe(), "a OR b");

        let all_clause = SubjectClause::all_of(vec!["x".to_string(), "y".to_string()]);
        assert_eq!(all_clause.describe(), "x AND y");
    }

    #[test]
    fn test_domain_pattern_describe() {
        let simple = DomainPattern::new("example.com");
        assert_eq!(simple.describe(), "*@example.com");

        let with_sub = DomainPattern::with_subdomains("example.com");
        assert_eq!(with_sub.describe(), "*@*.example.com");
    }

    #[test]
    fn test_action_conflict_describe() {
        let label_conflict = ActionConflict::DifferentLabels {
            label_a: "Work".to_string(),
            label_b: "Personal".to_string(),
        };
        let desc = label_conflict.describe();
        assert!(desc.contains("Work"));
        assert!(desc.contains("Personal"));

        let archive_conflict = ActionConflict::DifferentArchive {
            archive_a: true,
            archive_b: false,
        };
        let desc = archive_conflict.describe();
        assert!(desc.contains("archive"));
        assert!(desc.contains("inbox"));

        let delete_conflict = ActionConflict::DifferentDelete {
            delete_a: true,
            delete_b: false,
        };
        let desc = delete_conflict.describe();
        assert!(desc.contains("delete"));
        assert!(desc.contains("keep"));
    }

    #[test]
    fn test_from_sender_without_domain() {
        // Edge case: sender without @ symbol
        let filter = FilterExpr::from_sender("localsender");
        if let Some(FromClause::SpecificSender(ref email)) = filter.from_clause {
            assert_eq!(email.local_part, "localsender");
            assert_eq!(email.domain, "");
        } else {
            panic!("Expected SpecificSender");
        }
    }
}
