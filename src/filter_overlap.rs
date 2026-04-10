//! Filter overlap detection algorithms
//!
//! This module provides AST-based analysis of Gmail filter relationships,
//! detecting overlaps, redundancies, and conflicts without requiring example emails.
//!
//! # Overview
//!
//! The analyzer examines pairs of filters and determines their relationship:
//! - **Disjoint**: Filters match completely different emails
//! - **Identical**: Filters match exactly the same emails
//! - **Subsumes**: Filter A catches all emails that B catches (and possibly more)
//! - **SubsumedBy**: Filter A is caught by filter B
//! - **Overlaps**: Filters share some matching emails but not all
//!
//! # Example
//!
//! ```
//! use gmail_automation::filter_ast::{FilterExpr, Filter, FilterActions};
//! use gmail_automation::filter_overlap::{FilterOverlapAnalyzer, ConflictSeverity};
//!
//! let analyzer = FilterOverlapAnalyzer::new();
//!
//! let filter_a = Filter::new(
//!     "1",
//!     "All GitHub",
//!     FilterExpr::from_domain("github.com"),
//!     FilterActions::with_label("GitHub"),
//! );
//!
//! let filter_b = Filter::new(
//!     "2",
//!     "GitHub Bot",
//!     FilterExpr::from_sender("bot@github.com"),
//!     FilterActions::with_label("GitHub/bots"),
//! );
//!
//! let result = analyzer.analyze_all(vec![filter_a, filter_b]);
//! // Result will show that filter_a subsumes filter_b (domain covers specific sender)
//! ```

use crate::filter_ast::{
    ActionConflict, DomainPattern, EmailPattern, Filter, FilterExpr, FromClause, SubjectClause,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Relationship between two filter patterns
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PatternRelation {
    /// Patterns match completely different emails
    Disjoint,
    /// Patterns match exactly the same emails
    Identical,
    /// First pattern catches all emails that second catches (and possibly more)
    Subsumes,
    /// First pattern is completely covered by second pattern
    SubsumedBy,
    /// Patterns share some matching emails but not all
    Overlaps {
        /// Human-readable description of the overlap
        description: String,
    },
}

impl PatternRelation {
    /// Returns true if there's any overlap between patterns
    pub fn has_overlap(&self) -> bool {
        !matches!(self, PatternRelation::Disjoint)
    }

    /// Returns a human-readable description
    pub fn describe(&self) -> String {
        match self {
            PatternRelation::Disjoint => "No overlap".to_string(),
            PatternRelation::Identical => "Identical patterns".to_string(),
            PatternRelation::Subsumes => "First pattern subsumes second".to_string(),
            PatternRelation::SubsumedBy => "First pattern is subsumed by second".to_string(),
            PatternRelation::Overlaps { description } => description.clone(),
        }
    }
}

/// Type of conflict between filters
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ConflictType {
    /// Filters match overlapping emails
    Overlap,
    /// Theoretical overlap: filters on orthogonal dimensions (FROM-only vs SUBJECT-only)
    /// These rarely conflict in practice since they target different email characteristics
    TheoreticalOverlap,
    /// One filter is completely redundant (covered by another)
    Redundancy,
    /// Filters apply different labels to same emails
    LabelConflict,
    /// Filters have different archive settings for same emails
    ArchiveConflict,
    /// An exclusion negates part of a filter's matches
    ExclusionConflict,
}

impl ConflictType {
    /// Returns a human-readable name
    pub fn name(&self) -> &str {
        match self {
            ConflictType::Overlap => "Overlap",
            ConflictType::TheoreticalOverlap => "Theoretical Overlap",
            ConflictType::Redundancy => "Redundancy",
            ConflictType::LabelConflict => "Label Conflict",
            ConflictType::ArchiveConflict => "Archive Conflict",
            ConflictType::ExclusionConflict => "Exclusion Conflict",
        }
    }

    /// Returns true if this is a theoretical overlap (orthogonal dimensions)
    pub fn is_theoretical(&self) -> bool {
        matches!(self, ConflictType::TheoreticalOverlap)
    }
}

/// Severity of a detected conflict
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ConflictSeverity {
    /// Informational - may be intentional
    Info,
    /// Warning - should review
    Warning,
    /// Error - likely unintended, needs fixing
    Error,
}

impl ConflictSeverity {
    /// Returns a human-readable name
    pub fn name(&self) -> &str {
        match self {
            ConflictSeverity::Info => "Info",
            ConflictSeverity::Warning => "Warning",
            ConflictSeverity::Error => "Error",
        }
    }
}

/// A detected conflict between two filters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterConflict {
    /// ID of first filter
    pub filter_a_id: String,
    /// ID of second filter
    pub filter_b_id: String,
    /// Name of first filter (for display)
    pub filter_a_name: String,
    /// Name of second filter (for display)
    pub filter_b_name: String,
    /// Type of conflict
    pub conflict_type: ConflictType,
    /// Severity level
    pub severity: ConflictSeverity,
    /// Human-readable description of the conflict
    pub description: String,
    /// Suggested resolutions
    pub resolution_suggestions: Vec<String>,
}

impl FilterConflict {
    /// Creates a new conflict
    pub fn new(
        filter_a: &Filter,
        filter_b: &Filter,
        conflict_type: ConflictType,
        severity: ConflictSeverity,
        description: String,
    ) -> Self {
        Self {
            filter_a_id: filter_a.id.clone(),
            filter_b_id: filter_b.id.clone(),
            filter_a_name: filter_a.name.clone(),
            filter_b_name: filter_b.name.clone(),
            conflict_type,
            severity,
            description,
            resolution_suggestions: Vec::new(),
        }
    }

    /// Adds resolution suggestions
    pub fn with_suggestions(mut self, suggestions: Vec<String>) -> Self {
        self.resolution_suggestions = suggestions;
        self
    }

    /// Returns a summary of the conflict
    pub fn summary(&self) -> String {
        format!(
            "[{}] {} between '{}' and '{}': {}",
            self.severity.name(),
            self.conflict_type.name(),
            self.filter_a_name,
            self.filter_b_name,
            self.description
        )
    }
}

/// Result of analyzing all filters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisResult {
    /// Total number of filters analyzed
    pub total_filters: usize,
    /// Detected conflicts
    pub conflicts: Vec<FilterConflict>,
    /// Pairwise relations between filters (for visualization)
    pub relations: HashMap<(String, String), PatternRelation>,
}

impl AnalysisResult {
    /// Returns conflicts filtered by severity
    pub fn conflicts_by_severity(&self, min_severity: ConflictSeverity) -> Vec<&FilterConflict> {
        self.conflicts
            .iter()
            .filter(|c| c.severity >= min_severity)
            .collect()
    }

    /// Returns conflicts filtered by type
    pub fn conflicts_by_type(&self, conflict_type: ConflictType) -> Vec<&FilterConflict> {
        self.conflicts
            .iter()
            .filter(|c| c.conflict_type == conflict_type)
            .collect()
    }

    /// Returns the number of errors
    pub fn error_count(&self) -> usize {
        self.conflicts
            .iter()
            .filter(|c| c.severity == ConflictSeverity::Error)
            .count()
    }

    /// Returns the number of warnings
    pub fn warning_count(&self) -> usize {
        self.conflicts
            .iter()
            .filter(|c| c.severity == ConflictSeverity::Warning)
            .count()
    }

    /// Returns a summary of the analysis
    pub fn summary(&self) -> String {
        format!(
            "Analyzed {} filters: {} errors, {} warnings, {} info",
            self.total_filters,
            self.error_count(),
            self.warning_count(),
            self.conflicts.len() - self.error_count() - self.warning_count()
        )
    }
}

/// Filter overlap analyzer
///
/// Analyzes pairs of filters to detect overlaps, redundancies, and conflicts.
pub struct FilterOverlapAnalyzer {
    /// Whether to include informational conflicts
    include_info: bool,
}

impl FilterOverlapAnalyzer {
    /// Creates a new analyzer
    pub fn new() -> Self {
        Self { include_info: true }
    }

    /// Creates an analyzer that excludes informational conflicts
    pub fn warnings_and_errors_only() -> Self {
        Self { include_info: false }
    }

    /// Analyzes all filters and returns detected conflicts
    pub fn analyze_all(&self, filters: Vec<Filter>) -> AnalysisResult {
        let mut conflicts = Vec::new();
        let mut relations = HashMap::new();

        // Compare each pair of filters
        for i in 0..filters.len() {
            for j in (i + 1)..filters.len() {
                let filter_a = &filters[i];
                let filter_b = &filters[j];

                // Analyze the relationship
                let relation = self.analyze_filter_relation(filter_a, filter_b);
                relations.insert(
                    (filter_a.id.clone(), filter_b.id.clone()),
                    relation.clone(),
                );

                // Generate conflicts based on relationship
                if let Some(conflict) = self.generate_conflict(filter_a, filter_b, &relation) {
                    if self.include_info || conflict.severity > ConflictSeverity::Info {
                        conflicts.push(conflict);
                    }
                }
            }
        }

        AnalysisResult {
            total_filters: filters.len(),
            conflicts,
            relations,
        }
    }

    /// Analyzes the relationship between two filters
    pub fn analyze_filter_relation(&self, filter_a: &Filter, filter_b: &Filter) -> PatternRelation {
        self.analyze_expr_relation(&filter_a.expr, &filter_b.expr)
    }

    /// Analyzes the relationship between two filter expressions
    pub fn analyze_expr_relation(&self, expr_a: &FilterExpr, expr_b: &FilterExpr) -> PatternRelation {
        // First, analyze FROM clause relationship
        let from_relation = match (&expr_a.from_clause, &expr_b.from_clause) {
            (Some(from_a), Some(from_b)) => self.analyze_from_relation(from_a, from_b),
            (None, Some(_)) => PatternRelation::Subsumes, // No from = matches all
            (Some(_), None) => PatternRelation::SubsumedBy, // Other matches all
            (None, None) => PatternRelation::Identical, // Both match all senders
        };

        // If FROM clauses are disjoint, filters are disjoint
        if matches!(from_relation, PatternRelation::Disjoint) {
            return PatternRelation::Disjoint;
        }

        // Analyze SUBJECT clause relationship
        let subject_relation = match (&expr_a.subject_clause, &expr_b.subject_clause) {
            (Some(subj_a), Some(subj_b)) => self.analyze_subject_relation(subj_a, subj_b),
            (None, Some(_)) => PatternRelation::Subsumes, // No subject = matches all
            (Some(_), None) => PatternRelation::SubsumedBy,
            (None, None) => PatternRelation::Identical,
        };

        // If SUBJECT clauses are disjoint, filters are disjoint
        if matches!(subject_relation, PatternRelation::Disjoint) {
            return PatternRelation::Disjoint;
        }

        // Check if exclusions resolve overlap
        if self.exclusions_resolve_overlap(expr_a, expr_b) {
            return PatternRelation::Disjoint;
        }

        // Combine relations
        self.combine_relations(from_relation, subject_relation)
    }

    /// Analyzes relationship between two FROM clauses
    pub fn analyze_from_relation(&self, from_a: &FromClause, from_b: &FromClause) -> PatternRelation {
        match (from_a, from_b) {
            // Domain vs Domain
            (FromClause::Domain(da), FromClause::Domain(db)) => {
                self.analyze_domain_relation(da, db)
            }

            // Specific sender vs Domain
            (FromClause::SpecificSender(email), FromClause::Domain(domain)) => {
                if domain.matches_domain(&email.domain) {
                    PatternRelation::SubsumedBy // Domain catches this specific sender
                } else {
                    PatternRelation::Disjoint
                }
            }

            // Domain vs Specific sender
            (FromClause::Domain(domain), FromClause::SpecificSender(email)) => {
                if domain.matches_domain(&email.domain) {
                    PatternRelation::Subsumes // Domain catches this specific sender
                } else {
                    PatternRelation::Disjoint
                }
            }

            // Specific sender vs Specific sender
            (FromClause::SpecificSender(a), FromClause::SpecificSender(b)) => {
                if a.matches(b) {
                    PatternRelation::Identical
                } else if a.domain.eq_ignore_ascii_case(&b.domain) {
                    // Same domain, different senders - could be related
                    PatternRelation::Disjoint // Actually disjoint, but same domain
                } else {
                    PatternRelation::Disjoint
                }
            }

            // MultipleSenders vs single (Domain or SpecificSender)
            (FromClause::MultipleSenders(senders), other) => {
                self.analyze_multi_vs_single(senders, other)
            }

            // Single vs MultipleSenders
            (other, FromClause::MultipleSenders(senders)) => {
                // Reverse the result
                match self.analyze_multi_vs_single(senders, other) {
                    PatternRelation::Subsumes => PatternRelation::SubsumedBy,
                    PatternRelation::SubsumedBy => PatternRelation::Subsumes,
                    other => other,
                }
            }
        }
    }

    /// Analyzes the relationship between MultipleSenders and a single FromClause
    fn analyze_multi_vs_single(&self, senders: &[FromClause], other: &FromClause) -> PatternRelation {
        let mut all_subsumed = true;
        let mut any_subsumes_or_identical = false;
        let mut any_overlaps = false;

        for sender in senders {
            let relation = self.analyze_from_relation(sender, other);
            match relation {
                PatternRelation::Identical | PatternRelation::Subsumes => {
                    // This sender subsumes or equals other
                    any_subsumes_or_identical = true;
                    all_subsumed = false; // this sender is not subsumed by other
                }
                PatternRelation::SubsumedBy => {
                    // This sender is subsumed by other — keep all_subsumed true
                }
                PatternRelation::Overlaps { .. } => {
                    any_overlaps = true;
                    all_subsumed = false;
                }
                PatternRelation::Disjoint => {
                    all_subsumed = false;
                }
            }
        }

        if all_subsumed {
            // All senders are subsumed by other
            PatternRelation::SubsumedBy
        } else if any_subsumes_or_identical {
            // Multi contains other (and possibly more)
            PatternRelation::Subsumes
        } else if any_overlaps {
            PatternRelation::Overlaps {
                description: "Some senders overlap".to_string(),
            }
        } else {
            PatternRelation::Disjoint
        }
    }

    /// Analyzes relationship between two domain patterns
    fn analyze_domain_relation(&self, da: &DomainPattern, db: &DomainPattern) -> PatternRelation {
        let a_lower = da.domain.to_lowercase();
        let b_lower = db.domain.to_lowercase();

        // Exact same domain
        if a_lower == b_lower {
            return match (da.include_subdomains, db.include_subdomains) {
                (true, true) => PatternRelation::Identical,
                (false, false) => PatternRelation::Identical,
                (true, false) => PatternRelation::Subsumes, // A includes subdomains, B doesn't
                (false, true) => PatternRelation::SubsumedBy,
            };
        }

        // Check subdomain relationships
        if da.include_subdomains && is_subdomain(&b_lower, &a_lower) {
            // B is a subdomain of A, and A includes subdomains
            return PatternRelation::Subsumes;
        }

        if db.include_subdomains && is_subdomain(&a_lower, &b_lower) {
            // A is a subdomain of B, and B includes subdomains
            return PatternRelation::SubsumedBy;
        }

        PatternRelation::Disjoint
    }

    /// Analyzes relationship between two subject clauses
    fn analyze_subject_relation(
        &self,
        subj_a: &SubjectClause,
        subj_b: &SubjectClause,
    ) -> PatternRelation {
        // Check if keywords overlap
        let common_keywords: Vec<_> = subj_a
            .keywords
            .iter()
            .filter(|kw| {
                let kw_lower = kw.to_lowercase();
                subj_b
                    .keywords
                    .iter()
                    .any(|other| other.to_lowercase() == kw_lower)
            })
            .collect();

        if common_keywords.is_empty() {
            // No common keywords
            return PatternRelation::Disjoint;
        }

        // Some common keywords exist
        let a_keywords_count = subj_a.keywords.len();
        let b_keywords_count = subj_b.keywords.len();
        let common_count = common_keywords.len();

        // All keywords are the same
        if common_count == a_keywords_count && common_count == b_keywords_count {
            return PatternRelation::Identical;
        }

        // A is a subset of B (all of A's keywords are in B)
        if common_count == a_keywords_count {
            return PatternRelation::SubsumedBy;
        }

        // B is a subset of A
        if common_count == b_keywords_count {
            return PatternRelation::Subsumes;
        }

        // Partial overlap
        PatternRelation::Overlaps {
            description: format!(
                "Shared keywords: {}",
                common_keywords
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }

    /// Checks if exclusions resolve an overlap between two filters
    pub fn exclusions_resolve_overlap(&self, expr_a: &FilterExpr, expr_b: &FilterExpr) -> bool {
        // Check if A's exclusions exclude B's matches
        if let Some(ref from_b) = expr_b.from_clause {
            for exclusion in &expr_a.exclusions {
                let relation = self.analyze_from_relation(&exclusion.pattern, from_b);
                if matches!(
                    relation,
                    PatternRelation::Subsumes | PatternRelation::Identical
                ) {
                    return true; // A excludes everything B matches
                }
            }
        }

        // Check if B's exclusions exclude A's matches
        if let Some(ref from_a) = expr_a.from_clause {
            for exclusion in &expr_b.exclusions {
                let relation = self.analyze_from_relation(&exclusion.pattern, from_a);
                if matches!(
                    relation,
                    PatternRelation::Subsumes | PatternRelation::Identical
                ) {
                    return true; // B excludes everything A matches
                }
            }
        }

        false
    }

    /// Combines FROM and SUBJECT relations into overall relation
    fn combine_relations(
        &self,
        from_relation: PatternRelation,
        subject_relation: PatternRelation,
    ) -> PatternRelation {
        match (&from_relation, &subject_relation) {
            // Both identical = identical
            (PatternRelation::Identical, PatternRelation::Identical) => PatternRelation::Identical,

            // One subsumes, other identical = subsumes
            (PatternRelation::Subsumes, PatternRelation::Identical)
            | (PatternRelation::Identical, PatternRelation::Subsumes) => PatternRelation::Subsumes,

            // One subsumed, other identical = subsumed
            (PatternRelation::SubsumedBy, PatternRelation::Identical)
            | (PatternRelation::Identical, PatternRelation::SubsumedBy) => {
                PatternRelation::SubsumedBy
            }

            // Both subsume = subsumes
            (PatternRelation::Subsumes, PatternRelation::Subsumes) => PatternRelation::Subsumes,

            // Both subsumed = subsumed
            (PatternRelation::SubsumedBy, PatternRelation::SubsumedBy) => {
                PatternRelation::SubsumedBy
            }

            // Mixed directions = overlap
            (PatternRelation::Subsumes, PatternRelation::SubsumedBy)
            | (PatternRelation::SubsumedBy, PatternRelation::Subsumes) => PatternRelation::Overlaps {
                description: "Filters have partially overlapping criteria".to_string(),
            },

            // Any overlap = overlap
            (PatternRelation::Overlaps { .. }, _) | (_, PatternRelation::Overlaps { .. }) => {
                PatternRelation::Overlaps {
                    description: "Filters have overlapping criteria".to_string(),
                }
            }

            // Disjoint should have been caught earlier, but handle anyway
            (PatternRelation::Disjoint, _) | (_, PatternRelation::Disjoint) => {
                PatternRelation::Disjoint
            }
        }
    }

    /// Generates a conflict from a relationship (if applicable)
    fn generate_conflict(
        &self,
        filter_a: &Filter,
        filter_b: &Filter,
        relation: &PatternRelation,
    ) -> Option<FilterConflict> {
        match relation {
            PatternRelation::Disjoint => None,

            PatternRelation::Identical => {
                // Check for action conflicts
                if let Some(action_conflict) =
                    filter_a.actions.conflicts_with(&filter_b.actions)
                {
                    Some(
                        FilterConflict::new(
                            filter_a,
                            filter_b,
                            match &action_conflict {
                                ActionConflict::DifferentLabels { .. } => ConflictType::LabelConflict,
                                ActionConflict::DifferentArchive { .. } => {
                                    ConflictType::ArchiveConflict
                                }
                                ActionConflict::DifferentDelete { .. } => ConflictType::LabelConflict,
                            },
                            ConflictSeverity::Error,
                            format!(
                                "Identical filters with conflicting actions: {}",
                                action_conflict.describe()
                            ),
                        )
                        .with_suggestions(vec![
                            "Merge filters into one".to_string(),
                            "Remove duplicate filter".to_string(),
                        ]),
                    )
                } else {
                    // Same filter, same actions = redundancy
                    Some(
                        FilterConflict::new(
                            filter_a,
                            filter_b,
                            ConflictType::Redundancy,
                            ConflictSeverity::Warning,
                            "Filters are identical - one is redundant".to_string(),
                        )
                        .with_suggestions(vec!["Remove duplicate filter".to_string()]),
                    )
                }
            }

            PatternRelation::Subsumes => {
                // Check for action conflicts
                if let Some(action_conflict) =
                    filter_a.actions.conflicts_with(&filter_b.actions)
                {
                    Some(
                        FilterConflict::new(
                            filter_a,
                            filter_b,
                            ConflictType::LabelConflict,
                            ConflictSeverity::Warning,
                            format!(
                                "'{}' catches all emails that '{}' catches, but with {}: {}",
                                filter_a.name,
                                filter_b.name,
                                match &action_conflict {
                                    ActionConflict::DifferentLabels { .. } => "different labels",
                                    ActionConflict::DifferentArchive { .. } => "different archive settings",
                                    ActionConflict::DifferentDelete { .. } => "different delete settings",
                                },
                                action_conflict.describe()
                            ),
                        )
                        .with_suggestions(vec![
                            format!(
                                "Add exclusion in '{}' for '{}'",
                                filter_a.name, filter_b.name
                            ),
                            format!(
                                "Remove '{}' if '{}' should handle these emails",
                                filter_b.name, filter_a.name
                            ),
                        ]),
                    )
                } else {
                    // One filter is more specific but same actions - may be intentional
                    Some(
                        FilterConflict::new(
                            filter_a,
                            filter_b,
                            ConflictType::Redundancy,
                            ConflictSeverity::Info,
                            format!(
                                "'{}' is broader than '{}' - the more specific filter may be redundant",
                                filter_a.name, filter_b.name
                            ),
                        )
                        .with_suggestions(vec![
                            format!(
                                "Keep both if '{}' needs special handling",
                                filter_b.name
                            ),
                            format!(
                                "Remove '{}' if it's not needed",
                                filter_b.name
                            ),
                        ]),
                    )
                }
            }

            PatternRelation::SubsumedBy => {
                // Reverse of Subsumes - filter_b subsumes filter_a
                if let Some(action_conflict) =
                    filter_a.actions.conflicts_with(&filter_b.actions)
                {
                    Some(
                        FilterConflict::new(
                            filter_a,
                            filter_b,
                            ConflictType::LabelConflict,
                            ConflictSeverity::Warning,
                            format!(
                                "'{}' catches all emails that '{}' catches: {}",
                                filter_b.name,
                                filter_a.name,
                                action_conflict.describe()
                            ),
                        )
                        .with_suggestions(vec![
                            format!(
                                "Add exclusion in '{}' for '{}'",
                                filter_b.name, filter_a.name
                            ),
                        ]),
                    )
                } else {
                    Some(
                        FilterConflict::new(
                            filter_a,
                            filter_b,
                            ConflictType::Redundancy,
                            ConflictSeverity::Info,
                            format!(
                                "'{}' is broader than '{}' - the more specific filter may be redundant",
                                filter_b.name, filter_a.name
                            ),
                        )
                        .with_suggestions(vec![
                            format!(
                                "Remove '{}' if not needed",
                                filter_a.name
                            ),
                        ]),
                    )
                }
            }

            PatternRelation::Overlaps { description } => {
                // Check if this is a theoretical overlap (orthogonal dimensions)
                // One filter is FROM-only, the other is SUBJECT-only
                let is_orthogonal = self.is_orthogonal_dimensions(&filter_a.expr, &filter_b.expr);

                // Partial overlap
                if let Some(action_conflict) =
                    filter_a.actions.conflicts_with(&filter_b.actions)
                {
                    // Even with action conflicts, orthogonal dimensions are less severe
                    let (conflict_type, severity) = if is_orthogonal {
                        (ConflictType::TheoreticalOverlap, ConflictSeverity::Info)
                    } else {
                        (ConflictType::LabelConflict, ConflictSeverity::Error)
                    };

                    Some(
                        FilterConflict::new(
                            filter_a,
                            filter_b,
                            conflict_type,
                            severity,
                            format!(
                                "{}overlapping filters with conflicting actions ({}): {}",
                                if is_orthogonal { "Theoretically " } else { "" },
                                description,
                                action_conflict.describe()
                            ),
                        )
                        .with_suggestions(if is_orthogonal {
                            vec![
                                "Filters target different email characteristics (FROM vs SUBJECT) - unlikely to conflict in practice".to_string(),
                            ]
                        } else {
                            vec![
                                "Add exclusions to separate the filters".to_string(),
                                "Merge filters if they should have same behavior".to_string(),
                            ]
                        }),
                    )
                } else {
                    let (conflict_type, description_text) = if is_orthogonal {
                        (
                            ConflictType::TheoreticalOverlap,
                            "Filters on orthogonal dimensions (FROM vs SUBJECT) - theoretical overlap only".to_string(),
                        )
                    } else {
                        (
                            ConflictType::Overlap,
                            format!("Filters overlap ({})", description),
                        )
                    };

                    Some(
                        FilterConflict::new(
                            filter_a,
                            filter_b,
                            conflict_type,
                            ConflictSeverity::Info,
                            description_text,
                        )
                        .with_suggestions(if is_orthogonal {
                            vec![
                                "These filters target different email characteristics and rarely conflict in practice".to_string(),
                            ]
                        } else {
                            vec![
                                "Consider if overlap is intentional".to_string(),
                            ]
                        }),
                    )
                }
            }
        }
    }

    /// Checks if two filters are on orthogonal dimensions
    /// (one is FROM-only, the other is SUBJECT-only)
    fn is_orthogonal_dimensions(&self, expr_a: &FilterExpr, expr_b: &FilterExpr) -> bool {
        let a_has_from = expr_a.from_clause.is_some();
        let a_has_subject = expr_a.subject_clause.is_some();
        let b_has_from = expr_b.from_clause.is_some();
        let b_has_subject = expr_b.subject_clause.is_some();

        // Orthogonal: A has FROM but no SUBJECT, B has SUBJECT but no FROM (or vice versa)
        (a_has_from && !a_has_subject && !b_has_from && b_has_subject)
            || (!a_has_from && a_has_subject && b_has_from && !b_has_subject)
    }
}

impl Default for FilterOverlapAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

/// Checks if `child` is a subdomain of `parent`
///
/// For example: "mail.github.com" is a subdomain of "github.com"
fn is_subdomain(child: &str, parent: &str) -> bool {
    child.ends_with(&format!(".{}", parent))
}

/// Parses a Gmail filter query into a FilterExpr
///
/// This is useful for converting existing filters into AST form for analysis.
pub fn parse_gmail_query(query: &str) -> FilterExpr {
    let mut expr = FilterExpr::new();

    // Parse from: clause
    if let Some(from_start) = query.find("from:(") {
        let from_end = query[from_start + 6..].find(')').map(|i| from_start + 6 + i);
        if let Some(end) = from_end {
            let from_pattern = &query[from_start + 6..end];

            if from_pattern.contains(" OR ") {
                // Multiple senders with OR
                let parts: Vec<FromClause> = from_pattern
                    .split(" OR ")
                    .filter_map(|part| {
                        let part = part.trim();
                        if part.starts_with("*@") {
                            Some(FromClause::Domain(DomainPattern::new(part.trim_start_matches("*@"))))
                        } else if part.contains('@') {
                            EmailPattern::parse(part).map(FromClause::SpecificSender)
                        } else {
                            None
                        }
                    })
                    .collect();

                if parts.len() == 1 {
                    expr.from_clause = Some(parts.into_iter().next().unwrap());
                } else if parts.len() >= 2 {
                    expr.from_clause = Some(FromClause::MultipleSenders(parts));
                }
            } else if from_pattern.starts_with("*@") {
                // Domain pattern
                let domain = from_pattern.trim_start_matches("*@");
                expr.from_clause = Some(FromClause::Domain(DomainPattern::new(domain)));
            } else if from_pattern.contains('@') {
                // Specific sender
                if let Some(email) = EmailPattern::parse(from_pattern) {
                    expr.from_clause = Some(FromClause::SpecificSender(email));
                }
            }
        }
    }

    // Parse -from: exclusions
    let mut search_start = 0;
    while let Some(pos) = query[search_start..].find("-from:(") {
        let actual_pos = search_start + pos;
        let end = query[actual_pos + 7..].find(')').map(|i| actual_pos + 7 + i);
        if let Some(end_pos) = end {
            let excluded = &query[actual_pos + 7..end_pos];
            if let Some(email) = EmailPattern::parse(excluded) {
                expr.exclusions.push(crate::filter_ast::ExclusionClause {
                    pattern: FromClause::SpecificSender(email),
                });
            }
            search_start = end_pos;
        } else {
            break;
        }
    }

    // Parse subject: clause
    if let Some(subj_start) = query.find("subject:(") {
        let subj_end = query[subj_start + 9..].find(')').map(|i| subj_start + 9 + i);
        if let Some(end) = subj_end {
            let subject_content = &query[subj_start + 9..end];
            let keywords: Vec<String> = subject_content
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
    }

    expr
}

/// Converts a FilterExpr to Gmail query syntax
pub fn to_gmail_query(expr: &FilterExpr) -> String {
    let mut parts = Vec::new();

    if let Some(ref from) = expr.from_clause {
        match from {
            FromClause::Domain(d) => {
                if d.include_subdomains {
                    parts.push(format!("from:(*@*.{})", d.domain));
                } else {
                    parts.push(format!("from:(*@{})", d.domain));
                }
            }
            FromClause::SpecificSender(e) => {
                parts.push(format!("from:({})", e.full_address()));
            }
            FromClause::MultipleSenders(senders) => {
                let sender_strs: Vec<String> = senders.iter().map(|s| s.describe()).collect();
                parts.push(format!("from:({})", sender_strs.join(" OR ")));
            }
        }
    }

    for exclusion in &expr.exclusions {
        match &exclusion.pattern {
            FromClause::Domain(d) => {
                parts.push(format!("-from:(*@{})", d.domain));
            }
            FromClause::SpecificSender(e) => {
                parts.push(format!("-from:({})", e.full_address()));
            }
            FromClause::MultipleSenders(senders) => {
                let sender_strs: Vec<String> = senders.iter().map(|s| s.describe()).collect();
                parts.push(format!("-from:({})", sender_strs.join(" OR ")));
            }
        }
    }

    if let Some(ref subject) = expr.subject_clause {
        let keywords = subject.keywords.join(" OR ");
        parts.push(format!("subject:({})", keywords));
    }

    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter_ast::{FilterActions, SubjectMatchMode};

    fn make_filter(id: &str, name: &str, expr: FilterExpr, label: &str, archive: bool) -> Filter {
        Filter::new(
            id,
            name,
            expr,
            if archive {
                FilterActions::label_and_archive(label)
            } else {
                FilterActions::with_label(label)
            },
        )
    }

    #[test]
    fn test_domain_vs_domain_identical() {
        let analyzer = FilterOverlapAnalyzer::new();
        let relation = analyzer.analyze_domain_relation(
            &DomainPattern::new("github.com"),
            &DomainPattern::new("github.com"),
        );
        assert_eq!(relation, PatternRelation::Identical);
    }

    #[test]
    fn test_domain_vs_domain_disjoint() {
        let analyzer = FilterOverlapAnalyzer::new();
        let relation = analyzer.analyze_domain_relation(
            &DomainPattern::new("github.com"),
            &DomainPattern::new("gitlab.com"),
        );
        assert_eq!(relation, PatternRelation::Disjoint);
    }

    #[test]
    fn test_domain_vs_sender_subsumes() {
        let analyzer = FilterOverlapAnalyzer::new();
        let relation = analyzer.analyze_from_relation(
            &FromClause::Domain(DomainPattern::new("github.com")),
            &FromClause::SpecificSender(EmailPattern::new("noreply", "github.com")),
        );
        assert_eq!(relation, PatternRelation::Subsumes);
    }

    #[test]
    fn test_sender_vs_domain_subsumed() {
        let analyzer = FilterOverlapAnalyzer::new();
        let relation = analyzer.analyze_from_relation(
            &FromClause::SpecificSender(EmailPattern::new("noreply", "github.com")),
            &FromClause::Domain(DomainPattern::new("github.com")),
        );
        assert_eq!(relation, PatternRelation::SubsumedBy);
    }

    #[test]
    fn test_sender_vs_sender_identical() {
        let analyzer = FilterOverlapAnalyzer::new();
        let relation = analyzer.analyze_from_relation(
            &FromClause::SpecificSender(EmailPattern::new("noreply", "github.com")),
            &FromClause::SpecificSender(EmailPattern::new("noreply", "github.com")),
        );
        assert_eq!(relation, PatternRelation::Identical);
    }

    #[test]
    fn test_sender_vs_sender_disjoint() {
        let analyzer = FilterOverlapAnalyzer::new();
        let relation = analyzer.analyze_from_relation(
            &FromClause::SpecificSender(EmailPattern::new("noreply", "github.com")),
            &FromClause::SpecificSender(EmailPattern::new("bot", "github.com")),
        );
        assert_eq!(relation, PatternRelation::Disjoint);
    }

    #[test]
    fn test_exclusions_resolve_overlap() {
        let analyzer = FilterOverlapAnalyzer::new();

        // Filter A: *@github.com, excluding bot@github.com
        let expr_a = FilterExpr::from_domain("github.com").with_exclusion("bot@github.com");

        // Filter B: bot@github.com
        let expr_b = FilterExpr::from_sender("bot@github.com");

        // A's exclusion should resolve the overlap
        assert!(analyzer.exclusions_resolve_overlap(&expr_a, &expr_b));
    }

    #[test]
    fn test_analyze_all_finds_conflicts() {
        let analyzer = FilterOverlapAnalyzer::new();

        let filters = vec![
            make_filter(
                "1",
                "All GitHub",
                FilterExpr::from_domain("github.com"),
                "GitHub",
                true,
            ),
            make_filter(
                "2",
                "GitHub Bot",
                FilterExpr::from_sender("bot@github.com"),
                "GitHub/bots",
                true,
            ),
        ];

        let result = analyzer.analyze_all(filters);
        assert_eq!(result.total_filters, 2);
        // Should find that filter 1 subsumes filter 2 with different labels
        assert!(!result.conflicts.is_empty());
    }

    #[test]
    fn test_analyze_all_no_conflict_when_disjoint() {
        let analyzer = FilterOverlapAnalyzer::warnings_and_errors_only();

        let filters = vec![
            make_filter(
                "1",
                "GitHub",
                FilterExpr::from_domain("github.com"),
                "GitHub",
                true,
            ),
            make_filter(
                "2",
                "GitLab",
                FilterExpr::from_domain("gitlab.com"),
                "GitLab",
                true,
            ),
        ];

        let result = analyzer.analyze_all(filters);
        assert_eq!(result.total_filters, 2);
        assert!(result.conflicts.is_empty());
    }

    #[test]
    fn test_parse_gmail_query() {
        let expr = parse_gmail_query("from:(*@github.com) -from:(bot@github.com) subject:(notification)");

        assert!(expr.from_clause.is_some());
        if let Some(FromClause::Domain(d)) = &expr.from_clause {
            assert_eq!(d.domain, "github.com");
        } else {
            panic!("Expected domain pattern");
        }

        assert_eq!(expr.exclusions.len(), 1);
        assert!(expr.subject_clause.is_some());
    }

    #[test]
    fn test_to_gmail_query() {
        let expr = FilterExpr::from_domain("github.com")
            .with_exclusion("bot@github.com")
            .with_subject_keywords(vec!["notification".to_string()], SubjectMatchMode::Any);

        let query = to_gmail_query(&expr);
        assert!(query.contains("from:(*@github.com)"));
        assert!(query.contains("-from:(bot@github.com)"));
        assert!(query.contains("subject:(notification)"));
    }

    #[test]
    fn test_subject_overlap() {
        let analyzer = FilterOverlapAnalyzer::new();

        let subj_a = SubjectClause::any_of(vec!["newsletter".to_string(), "digest".to_string()]);
        let subj_b = SubjectClause::any_of(vec!["newsletter".to_string(), "update".to_string()]);

        let relation = analyzer.analyze_subject_relation(&subj_a, &subj_b);
        assert!(matches!(relation, PatternRelation::Overlaps { .. }));
    }

    #[test]
    fn test_conflict_severity() {
        // Test ordering
        assert!(ConflictSeverity::Error > ConflictSeverity::Warning);
        assert!(ConflictSeverity::Warning > ConflictSeverity::Info);
    }

    #[test]
    fn test_pattern_relation_has_overlap() {
        assert!(!PatternRelation::Disjoint.has_overlap());
        assert!(PatternRelation::Identical.has_overlap());
        assert!(PatternRelation::Subsumes.has_overlap());
        assert!(PatternRelation::SubsumedBy.has_overlap());
        assert!(PatternRelation::Overlaps { description: "test".to_string() }.has_overlap());
    }

    #[test]
    fn test_pattern_relation_describe() {
        assert_eq!(PatternRelation::Disjoint.describe(), "No overlap");
        assert_eq!(PatternRelation::Identical.describe(), "Identical patterns");
        assert!(PatternRelation::Subsumes.describe().contains("subsumes"));
    }

    #[test]
    fn test_conflict_type_name() {
        assert_eq!(ConflictType::Overlap.name(), "Overlap");
        assert_eq!(ConflictType::Redundancy.name(), "Redundancy");
        assert_eq!(ConflictType::LabelConflict.name(), "Label Conflict");
    }

    #[test]
    fn test_analysis_result_methods() {
        let analyzer = FilterOverlapAnalyzer::new();

        let filters = vec![
            make_filter(
                "1",
                "Filter A",
                FilterExpr::from_domain("test.com"),
                "LabelA",
                true,
            ),
            make_filter(
                "2",
                "Filter B",
                FilterExpr::from_domain("test.com"),
                "LabelB",
                true,
            ),
        ];

        let result = analyzer.analyze_all(filters);

        // Should have at least one conflict (different labels, same domain)
        assert!(!result.conflicts.is_empty());

        // Test filtering methods
        let _errors = result.conflicts_by_severity(ConflictSeverity::Error);
        let label_conflicts = result.conflicts_by_type(ConflictType::LabelConflict);

        assert!(!label_conflicts.is_empty());

        // Test counts
        let error_count = result.error_count();
        let warning_count = result.warning_count();
        assert!(error_count > 0 || warning_count > 0);

        // Test summary
        let summary = result.summary();
        assert!(summary.contains("Analyzed 2 filters"));
    }

    #[test]
    fn test_filter_conflict_summary() {
        let filter_a = Filter::new(
            "1",
            "Filter A",
            FilterExpr::from_domain("test.com"),
            FilterActions::with_label("A"),
        );
        let filter_b = Filter::new(
            "2",
            "Filter B",
            FilterExpr::from_domain("test.com"),
            FilterActions::with_label("B"),
        );

        let conflict = FilterConflict::new(
            &filter_a,
            &filter_b,
            ConflictType::LabelConflict,
            ConflictSeverity::Error,
            "Test description".to_string(),
        );

        let summary = conflict.summary();
        assert!(summary.contains("Error"));
        assert!(summary.contains("Filter A"));
        assert!(summary.contains("Filter B"));
    }

    #[test]
    fn test_subdomain_matching_in_overlap() {
        let analyzer = FilterOverlapAnalyzer::new();

        let parent = DomainPattern::with_subdomains("github.com");
        let child = DomainPattern::new("mail.github.com");

        let relation = analyzer.analyze_domain_relation(&parent, &child);
        assert_eq!(relation, PatternRelation::Subsumes);

        // Reverse should be subsumed
        let relation2 = analyzer.analyze_domain_relation(&child, &parent);
        assert_eq!(relation2, PatternRelation::SubsumedBy);
    }

    #[test]
    fn test_no_from_clause_handling() {
        let analyzer = FilterOverlapAnalyzer::new();

        let expr_no_from = FilterExpr::new();
        let expr_with_from = FilterExpr::from_domain("test.com");

        let relation = analyzer.analyze_expr_relation(&expr_no_from, &expr_with_from);
        assert_eq!(relation, PatternRelation::Subsumes);

        let relation2 = analyzer.analyze_expr_relation(&expr_with_from, &expr_no_from);
        assert_eq!(relation2, PatternRelation::SubsumedBy);
    }

    #[test]
    fn test_identical_filters_same_actions() {
        let analyzer = FilterOverlapAnalyzer::new();

        let filters = vec![
            make_filter(
                "1",
                "Filter A",
                FilterExpr::from_domain("test.com"),
                "SameLabel",
                true,
            ),
            make_filter(
                "2",
                "Filter B",
                FilterExpr::from_domain("test.com"),
                "SameLabel",
                true,
            ),
        ];

        let result = analyzer.analyze_all(filters);
        // Should detect redundancy, not conflict
        assert!(!result.conflicts.is_empty());
        let conflict = &result.conflicts[0];
        assert_eq!(conflict.conflict_type, ConflictType::Redundancy);
    }

    #[test]
    fn test_default_analyzer() {
        let analyzer = FilterOverlapAnalyzer::default();
        let filters = vec![
            make_filter("1", "A", FilterExpr::from_domain("a.com"), "A", false),
        ];
        let result = analyzer.analyze_all(filters);
        assert_eq!(result.total_filters, 1);
        assert!(result.conflicts.is_empty());
    }

    #[test]
    fn test_parse_gmail_query_specific_sender() {
        let expr = parse_gmail_query("from:(noreply@github.com)");

        assert!(expr.from_clause.is_some());
        if let Some(FromClause::SpecificSender(email)) = &expr.from_clause {
            assert_eq!(email.local_part, "noreply");
            assert_eq!(email.domain, "github.com");
        } else {
            panic!("Expected SpecificSender");
        }
    }

    #[test]
    fn test_to_gmail_query_with_subdomains() {
        let mut expr = FilterExpr::new();
        expr.from_clause = Some(FromClause::Domain(DomainPattern::with_subdomains("github.com")));

        let query = to_gmail_query(&expr);
        assert!(query.contains("*.github.com"));
    }

    #[test]
    fn test_combined_relation_mixed_directions() {
        let analyzer = FilterOverlapAnalyzer::new();

        // Create filters where FROM and SUBJECT have opposite relations
        let filter_a = Filter::new(
            "1",
            "Filter A",
            FilterExpr::from_domain("github.com")
                .with_subject_keywords(vec!["alert".to_string()], SubjectMatchMode::Any),
            FilterActions::with_label("A"),
        );

        let filter_b = Filter::new(
            "2",
            "Filter B",
            FilterExpr::from_sender("bot@github.com")
                .with_subject_keywords(
                    vec!["alert".to_string(), "notification".to_string()],
                    SubjectMatchMode::Any,
                ),
            FilterActions::with_label("B"),
        );

        let relation = analyzer.analyze_filter_relation(&filter_a, &filter_b);
        // FROM: domain subsumes sender (Subsumes)
        // SUBJECT: single keyword is subsumed by multiple (SubsumedBy)
        // Combined: Overlaps (mixed directions)
        assert!(matches!(relation, PatternRelation::Overlaps { .. }));
    }

    #[test]
    fn test_parse_gmail_query_or_senders() {
        let expr = parse_gmail_query("from:(allsales@powerbuys.com.au OR dailydeals@powerbuys.com.au)");
        assert!(expr.from_clause.is_some());
        if let Some(FromClause::MultipleSenders(senders)) = &expr.from_clause {
            assert_eq!(senders.len(), 2);
        } else {
            panic!("Expected MultipleSenders, got {:?}", expr.from_clause);
        }
    }

    #[test]
    fn test_multiple_senders_vs_single_sender_disjoint() {
        let analyzer = FilterOverlapAnalyzer::new();
        let multi = FromClause::MultipleSenders(vec![
            FromClause::SpecificSender(EmailPattern::new("a", "x.com")),
            FromClause::SpecificSender(EmailPattern::new("b", "x.com")),
        ]);
        let single = FromClause::SpecificSender(EmailPattern::new("c", "y.com"));
        let relation = analyzer.analyze_from_relation(&multi, &single);
        assert_eq!(relation, PatternRelation::Disjoint);
    }

    #[test]
    fn test_multiple_senders_vs_single_sender_subsumes() {
        let analyzer = FilterOverlapAnalyzer::new();
        let multi = FromClause::MultipleSenders(vec![
            FromClause::SpecificSender(EmailPattern::new("a", "x.com")),
            FromClause::SpecificSender(EmailPattern::new("b", "x.com")),
        ]);
        let single = FromClause::SpecificSender(EmailPattern::new("a", "x.com"));
        let relation = analyzer.analyze_from_relation(&multi, &single);
        assert_eq!(relation, PatternRelation::Subsumes);
    }

    #[test]
    fn test_domain_subsumes_multiple_senders_same_domain() {
        let analyzer = FilterOverlapAnalyzer::new();
        let domain = FromClause::Domain(DomainPattern::new("x.com"));
        let multi = FromClause::MultipleSenders(vec![
            FromClause::SpecificSender(EmailPattern::new("a", "x.com")),
            FromClause::SpecificSender(EmailPattern::new("b", "x.com")),
        ]);
        let relation = analyzer.analyze_from_relation(&domain, &multi);
        assert_eq!(relation, PatternRelation::Subsumes);
    }

    #[test]
    fn test_powerbuys_vs_github_disjoint() {
        let analyzer = FilterOverlapAnalyzer::new();
        let expr_a = parse_gmail_query("from:(allsales@powerbuys.com.au OR dailydeals@powerbuys.com.au)");
        let expr_b = parse_gmail_query("from:(noreply@github.com)");
        let relation = analyzer.analyze_expr_relation(&expr_a, &expr_b);
        assert_eq!(relation, PatternRelation::Disjoint);
    }
}
