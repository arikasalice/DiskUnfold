//! F36 virtual-group classification of the observed files in one pinned analysis result.
//!
//! A classification evaluates each rule (an F04 condition) for every file below a scope and
//! keeps the three-valued decision per file and rule. Only files are members; a folder is
//! never evaluated and an empty scope is a confirmed result with no files. A predicate that
//! reports unknown makes the rule undecided for that file, and a rule using a predicate that
//! the registry does not hold is unavailable for every file. Neither is ever read as a
//! mismatch. Categories are derived from the decisions:
//!
//! * group `G`: the files that match rule `G`;
//! * overlap: files that match at least two rules;
//! * unclassified: files that match no rule and have no undecided or unavailable rule;
//! * undecidable: files that match no rule and have at least one undecided or unavailable rule.
//!
//! The scope's file count must equal matched + unclassified + undecidable before a
//! classification can be registered in a [`ClassificationStore`].

pub mod ruleset;

mod classification;
mod classify;
mod explain;
mod page;
mod scope;
mod store;
mod summary;
mod totals;

pub use classification::{Classification, Reconciliation};
pub use classify::{Control, Parallelism, classify_scope};
pub use explain::{NodeDecision, RuleExplanation};
pub use page::{Category, Page, PageRow};
pub use scope::scope_file_count;
pub use store::ClassificationStore;
pub use summary::{GroupCounts, Summary};
pub use totals::Totals;

use crate::{QueryError, query};

/// Reason key for a rule that uses a predicate the registry does not hold.
pub const REASON_RULE_UNEVALUABLE: &str = "group.rule_unevaluable";
/// Reason key for a rule whose predicate reported unknown for a file.
pub const REASON_VALUE_UNKNOWN: &str = "group.value_unknown";

/// A rule's decision for one file, in display order of the supplied rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    /// The file satisfies the rule.
    Match,
    /// The file does not satisfy the rule.
    NoMatch,
    /// A predicate could not decide for this file; this is not a mismatch.
    Unknown,
    /// The rule uses a predicate that is not registered, so no file was evaluated.
    RuleUnavailable,
}

impl Decision {
    /// Returns the established reason for an undecided or unavailable decision.
    pub fn reason_key(self) -> Option<&'static str> {
        match self {
            Self::Unknown => Some(REASON_VALUE_UNKNOWN),
            Self::RuleUnavailable => Some(REASON_RULE_UNEVALUABLE),
            Self::Match | Self::NoMatch => None,
        }
    }

    /// Reports whether the rule could not decide for the file.
    pub fn is_undecided(self) -> bool {
        matches!(self, Self::Unknown | Self::RuleUnavailable)
    }
}

impl From<query::Truth> for Decision {
    fn from(truth: query::Truth) -> Self {
        match truth {
            query::Truth::Match => Self::Match,
            query::Truth::NoMatch => Self::NoMatch,
            query::Truth::Unknown => Self::Unknown,
        }
    }
}

/// Admission or evaluation failure; a cancelled operation returns no partial value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The pinned result could not resolve an item that the classification refers to.
    Source(QueryError),
    /// The scope root is not a folder of the classified result version.
    InvalidRoot,
    /// A rule exceeds the condition limits.
    InvalidRule(query::Error),
    /// A requested sort uses a strategy the registry does not hold.
    InvalidSort(query::Error),
    /// A group index or category refers to a rule that does not exist.
    InvalidSelection,
    /// The item is not a file in this classification's scope and result version.
    InvalidItem,
    /// The counts of the classification do not reconcile, so it cannot be registered.
    InconsistentMembership,
    /// The F03 set total could not be calculated.
    Total(query::SpaceTotalError),
    /// The caller cancelled the operation; nothing partial is returned.
    Cancelled,
    /// An internal lock was poisoned or a worker failed.
    Unavailable,
}
