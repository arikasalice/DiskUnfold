//! Category counts and reason counts derived once from the stored decisions.

use std::collections::BTreeMap;

use super::{Category, Decision, REASON_RULE_UNEVALUABLE, REASON_VALUE_UNKNOWN};

/// Counts for one group, in rule display order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GroupCounts {
    /// Files that match the rule.
    pub members: u32,
    /// Members that also match another rule.
    pub overlap_members: u32,
    /// Files this rule could not decide, including files it did not evaluate.
    pub undecidable: u32,
}

/// Category counts of one classification.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Summary {
    /// Files evaluated in the scope.
    pub scope_files: u32,
    /// Files that match at least one rule.
    pub matched: u32,
    /// Files that match at least two rules.
    pub overlap: u32,
    /// Files that match no rule and have no undecided or unavailable rule.
    pub unclassified: u32,
    /// Files that match no rule and have an undecided or unavailable rule.
    pub undecidable: u32,
    /// Whether scope files = matched + unclassified + undecidable.
    pub reconciled: bool,
    /// One entry per rule, in display order.
    pub groups: Vec<GroupCounts>,
    /// Undecidable files per known reason key, each file counted once per reason.
    pub reason_counts: BTreeMap<&'static str, u32>,
}

/// How one file's rule decisions fall into the categories.
pub(super) struct Tally {
    /// Rules the file matches.
    pub(super) matches: usize,
    /// Some rule reported unknown for the file.
    pub(super) unknown: bool,
    /// Some rule is unavailable.
    pub(super) unavailable: bool,
}

impl Tally {
    pub(super) fn of(row: &[Decision]) -> Self {
        Self {
            matches: row.iter().filter(|d| **d == Decision::Match).count(),
            unknown: row.contains(&Decision::Unknown),
            unavailable: row.contains(&Decision::RuleUnavailable),
        }
    }

    /// Whether some rule could not decide for the file.
    pub(super) fn undecided(&self) -> bool {
        self.unknown || self.unavailable
    }
}

/// Returns the decisions of the file at `index`; every file has `width` entries.
pub(super) fn row_at(decisions: &[Decision], width: usize, index: usize) -> &[Decision] {
    decisions
        .get(index.saturating_mul(width)..index.saturating_add(1).saturating_mul(width))
        .unwrap_or_default()
}

/// Reports whether a file with these decisions belongs to `category`.
pub(super) fn belongs(category: Category, row: &[Decision]) -> bool {
    if let Category::Group(index) = category {
        return row.get(index) == Some(&Decision::Match);
    }
    let tally = Tally::of(row);
    match category {
        Category::Overlap => tally.matches >= 2,
        Category::Unclassified => tally.matches == 0 && !tally.undecided(),
        Category::Undecidable => tally.matches == 0 && tally.undecided(),
        Category::Group(_) => false,
    }
}

impl Summary {
    /// Counts every file once. `decisions` holds `rule_count` entries per file.
    pub(super) fn compute(file_count: usize, decisions: &[Decision], rule_count: usize) -> Self {
        let mut summary = Self {
            scope_files: saturate(file_count),
            matched: 0,
            overlap: 0,
            unclassified: 0,
            undecidable: 0,
            reconciled: false,
            groups: vec![GroupCounts::default(); rule_count],
            reason_counts: BTreeMap::new(),
        };
        for index in 0..file_count {
            summary.add(row_at(decisions, rule_count, index));
        }
        let partition = u64::from(summary.matched)
            + u64::from(summary.unclassified)
            + u64::from(summary.undecidable);
        summary.reconciled = u64::from(summary.scope_files) == partition;
        summary
    }

    fn add(&mut self, row: &[Decision]) {
        let tally = Tally::of(row);
        for (counts, decision) in self.groups.iter_mut().zip(row) {
            if *decision == Decision::Match {
                counts.members = counts.members.saturating_add(1);
                counts.overlap_members += u32::from(tally.matches >= 2);
            }
            counts.undecidable += u32::from(decision.is_undecided());
        }
        if tally.matches > 0 {
            self.matched = self.matched.saturating_add(1);
            self.overlap += u32::from(tally.matches >= 2);
        } else if tally.undecided() {
            self.undecidable = self.undecidable.saturating_add(1);
            for (present, key) in [
                (tally.unavailable, REASON_RULE_UNEVALUABLE),
                (tally.unknown, REASON_VALUE_UNKNOWN),
            ] {
                if present {
                    *self.reason_counts.entry(key).or_default() += 1;
                }
            }
        } else {
            self.unclassified = self.unclassified.saturating_add(1);
        }
    }
}

fn saturate(count: usize) -> u32 {
    u32::try_from(count).unwrap_or(u32::MAX)
}
