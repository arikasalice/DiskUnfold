//! The immutable result of one classification and its cheap accessors.

use std::{collections::BTreeMap, fmt, sync::Arc, sync::Mutex};

use du_core_contracts::{
    ItemRef,
    result::{ResultCompleteness, ResultVersionRead},
};

use super::{
    Decision, Error, REASON_RULE_UNEVALUABLE, REASON_VALUE_UNKNOWN,
    page::OrderCache,
    summary::{Summary, Tally, row_at},
};
use crate::{ResultVersion, query::Condition};

/// The inputs from which a classification is assembled after evaluation.
pub(super) struct Parts {
    pub(super) version: Arc<ResultVersion>,
    pub(super) root: Option<ItemRef>,
    pub(super) rules: Vec<Condition>,
    pub(super) rule_unavailable: Vec<bool>,
    pub(super) predicates: Vec<(String, u32)>,
    pub(super) files: Vec<u32>,
    pub(super) decisions: Vec<Decision>,
    pub(super) coverage_complete: bool,
}

/// Rule decisions for every file below one scope, with the exact source they came from.
///
/// A classification is immutable once built. Its summary is calculated once; member order
/// for a category and sort is calculated on the first request and kept for later pages.
pub struct Classification {
    pub(super) version: Arc<ResultVersion>,
    pub(super) root: Option<ItemRef>,
    pub(super) rules: Vec<Condition>,
    pub(super) rule_unavailable: Vec<bool>,
    pub(super) predicates: Vec<(String, u32)>,
    /// Item numbers of the member files in observed path order.
    pub(super) files: Vec<u32>,
    /// `rules.len()` decisions for each file, in file order.
    pub(super) decisions: Vec<Decision>,
    pub(super) coverage_complete: bool,
    pub(super) summary: Summary,
    pub(super) orders: Mutex<OrderCache>,
}

/// Recount of the observed files across all rules, independent of group selection.
#[derive(Debug, PartialEq, Eq)]
pub struct Reconciliation {
    /// Files evaluated in the scope.
    pub observed_files: usize,
    /// Files that match at least one rule.
    pub matched_files: usize,
    /// Files that match no rule and have no undecided rule.
    pub unclassified_files: usize,
    /// Files that match no rule and have an undecided or unavailable rule.
    pub undecidable_files: usize,
    /// Whether retained source evidence covers the entire requested scope.
    pub source_coverage_complete: bool,
    /// Known reason keys counted once per undecidable file.
    pub known_reason_counts: BTreeMap<&'static str, usize>,
    /// Undecidable files with an undecided value; they also count under the value reason.
    pub unattributed_files: usize,
}

impl Classification {
    pub(super) fn assemble(parts: Parts) -> Self {
        let summary = Summary::compute(parts.files.len(), &parts.decisions, parts.rules.len());
        Self {
            version: parts.version,
            root: parts.root,
            rules: parts.rules,
            rule_unavailable: parts.rule_unavailable,
            predicates: parts.predicates,
            files: parts.files,
            decisions: parts.decisions,
            coverage_complete: parts.coverage_complete,
            summary,
            orders: Mutex::default(),
        }
    }

    /// Returns the exact result version every decision and total was taken from.
    pub fn version(&self) -> &Arc<ResultVersion> {
        &self.version
    }

    /// Returns the scope folder, or `None` when the whole result was classified.
    pub fn root(&self) -> Option<&ItemRef> {
        self.root.as_ref()
    }

    /// Returns the rules in display order.
    pub fn rules(&self) -> &[Condition] {
        &self.rules
    }

    /// Reports, per rule, whether it uses a predicate that the registry did not hold.
    pub fn rule_unavailable(&self) -> &[bool] {
        &self.rule_unavailable
    }

    /// Lists the leaf predicates of all rules, sorted and without duplicates.
    pub fn predicates(&self) -> Vec<(String, u32)> {
        self.predicates.clone()
    }

    /// Returns the counts calculated when the classification was built.
    pub fn summary(&self) -> &Summary {
        &self.summary
    }

    /// Returns each member file with its decisions in rule order, in observed path order.
    pub fn rows(&self) -> impl Iterator<Item = (ItemRef, &[Decision])> + '_ {
        self.files.iter().enumerate().map(|(index, &item_no)| {
            (
                ItemRef {
                    result_version: self.version.version_id().clone(),
                    item_no,
                },
                self.row(index),
            )
        })
    }

    /// Returns the decisions of the file at `index` in path order, one per rule.
    pub(super) fn row(&self, index: usize) -> &[Decision] {
        row_at(&self.decisions, self.rules.len(), index)
    }

    /// Recounts the partition from the stored decisions, using all rules.
    ///
    /// Known reasons are counted once per undecidable file. Incomplete source coverage
    /// makes the observed count a non-exhaustive scope count.
    ///
    /// # Errors
    /// Returns [`Error::InconsistentMembership`] when the stored decisions do not have one
    /// entry per file and rule, and [`Error::Cancelled`].
    pub fn reconcile(&self, mut cancelled: impl FnMut() -> bool) -> Result<Reconciliation, Error> {
        if self.files.len().checked_mul(self.rules.len()) != Some(self.decisions.len()) {
            return Err(Error::InconsistentMembership);
        }
        let mut result = Reconciliation {
            observed_files: self.files.len(),
            matched_files: 0,
            unclassified_files: 0,
            undecidable_files: 0,
            source_coverage_complete: self.coverage_complete
                && self.version.completeness() == ResultCompleteness::Complete,
            known_reason_counts: BTreeMap::new(),
            unattributed_files: 0,
        };
        for index in 0..self.files.len() {
            if cancelled() {
                return Err(Error::Cancelled);
            }
            let tally = Tally::of(self.row(index));
            if tally.matches > 0 {
                result.matched_files += 1;
            } else if tally.undecided() {
                result.undecidable_files += 1;
                result.unattributed_files += usize::from(tally.unknown);
                for (present, key) in [
                    (tally.unavailable, REASON_RULE_UNEVALUABLE),
                    (tally.unknown, REASON_VALUE_UNKNOWN),
                ] {
                    if present {
                        *result.known_reason_counts.entry(key).or_default() += 1;
                    }
                }
            } else {
                result.unclassified_files += 1;
            }
        }
        if cancelled() {
            return Err(Error::Cancelled);
        }
        Ok(result)
    }

    /// Confirms that the cached summary and a fresh recount agree and partition the scope.
    pub(super) fn verify(&self) -> Result<(), Error> {
        let recount = self.reconcile(|| false)?;
        let summary = &self.summary;
        let partition = recount
            .matched_files
            .checked_add(recount.unclassified_files)
            .and_then(|sum| sum.checked_add(recount.undecidable_files));
        let agrees = summary.reconciled
            && partition == Some(recount.observed_files)
            && [
                (summary.scope_files, recount.observed_files),
                (summary.matched, recount.matched_files),
                (summary.unclassified, recount.unclassified_files),
                (summary.undecidable, recount.undecidable_files),
            ]
            .into_iter()
            .all(|(counted, recounted)| usize::try_from(counted) == Ok(recounted));
        if agrees {
            Ok(())
        } else {
            Err(Error::InconsistentMembership)
        }
    }
}

impl fmt::Debug for Classification {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Classification")
            .field("version", self.version.version_id())
            .field("root", &self.root)
            .field("rules", &self.rules.len())
            .field("summary", &self.summary)
            .finish()
    }
}
