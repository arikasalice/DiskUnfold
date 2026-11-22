//! Classified scan facts owned by each immutable publication.
use crate::{BuildError, BuildHandle, ResultVersion};
use du_core_contracts::{
    Timestamp,
    scan_failures::{ScanFailureKind, ScanIssue},
};

/// A pinned, root-partitioned index used to page classified issues without rescanning them.
#[derive(Debug)]
pub(crate) struct PageIndex {
    roots: Vec<RootIssues>,
}

#[derive(Debug)]
struct RootIssues {
    all: Vec<usize>,
    failures: Vec<usize>,
    by_kind: [Vec<usize>; 4],
    failures_by_kind: [Vec<usize>; 4],
}

impl RootIssues {
    fn new() -> Self {
        Self {
            all: Vec::new(),
            failures: Vec::new(),
            by_kind: std::array::from_fn(|_| Vec::new()),
            failures_by_kind: std::array::from_fn(|_| Vec::new()),
        }
    }

    fn rows(&self, kind: Option<ScanFailureKind>, failures_only: bool) -> &[usize] {
        match (kind, failures_only) {
            (None, false) => &self.all,
            (None, true) => &self.failures,
            (Some(kind), false) => &self.by_kind[kind_index(kind)],
            (Some(kind), true) => &self.failures_by_kind[kind_index(kind)],
        }
    }
}

impl PageIndex {
    fn new(issues: &[ScanIssue], root_count: usize) -> Self {
        let mut roots: Vec<_> = (0..root_count.max(1)).map(|_| RootIssues::new()).collect();
        for (issue_index, issue) in issues.iter().enumerate() {
            let Some(root) = roots.get_mut(issue.root_index as usize) else {
                continue;
            };
            let kind = kind_index(issue.kind);
            root.all.push(issue_index);
            root.by_kind[kind].push(issue_index);
            if issue
                .kind
                .counts_as_lane_failure(issue.stage, &issue.reason_key)
            {
                root.failures.push(issue_index);
                root.failures_by_kind[kind].push(issue_index);
            }
        }
        Self { roots }
    }
}

impl BuildHandle {
    /// Records a classified observation using its original root-relative bytes.
    pub fn record_scan_issue(&mut self, issue: ScanIssue) -> Result<(), BuildError> {
        if self.finished {
            return Err(BuildError::Unavailable);
        }
        if !self.valid_root(issue.root_index)
            || issue.relative.iter().any(Vec::is_empty)
            || issue.reason_key.is_empty()
        {
            return Err(BuildError::InvalidTree);
        }
        self.issues.push(issue);
        Ok(())
    }
}

impl ResultVersion {
    /// Returns the deterministically ordered collection facts pinned to this version.
    pub fn scan_issues(&self) -> &[ScanIssue] {
        &self.issues
    }

    /// Returns the matching row count and one page in deterministic issue order.
    ///
    /// The first call builds a pinned index in O(n) time and O(n) additional
    /// storage. Later pages visit at most the selected roots and returned rows.
    pub fn scan_issues_page(
        &self,
        root_indexes: Option<&[u32]>,
        kind: Option<ScanFailureKind>,
        failures_only: bool,
        offset: usize,
        limit: usize,
    ) -> (usize, Vec<&ScanIssue>) {
        let index = self
            .scan_issue_page_index
            .get_or_init(|| PageIndex::new(&self.issues, self.roots.len().max(1)));
        let roots = || -> Box<dyn Iterator<Item = u32> + '_> {
            match root_indexes {
                Some(indexes) => Box::new(indexes.iter().copied()),
                None => Box::new(0..u32::try_from(index.roots.len()).unwrap_or(u32::MAX)),
            }
        };
        let total: usize = roots()
            .filter_map(|root| index.roots.get(root as usize))
            .map(|root| root.rows(kind, failures_only).len())
            .sum();
        let mut rows = Vec::with_capacity(limit.min(total.saturating_sub(offset)));
        let mut skip = offset;
        for root_index in roots() {
            let Some(root) = index.roots.get(root_index as usize) else {
                continue;
            };
            let matching = root.rows(kind, failures_only);
            if skip >= matching.len() {
                skip -= matching.len();
                continue;
            }
            let start = skip;
            skip = 0;
            let end = matching.len().min(start.saturating_add(limit - rows.len()));
            rows.extend(
                matching[start..end]
                    .iter()
                    .filter_map(|issue_index| self.issues.get(*issue_index)),
            );
            if rows.len() == limit {
                break;
            }
        }
        (total, rows)
    }
}

fn kind_index(kind: ScanFailureKind) -> usize {
    match kind {
        ScanFailureKind::CollectionFailed => 0,
        ScanFailureKind::UnreadRange => 1,
        ScanFailureKind::Uncertain => 2,
        ScanFailureKind::UntraversedLink => 3,
    }
}

pub(crate) fn ordered(issues: &[ScanIssue]) -> Vec<ScanIssue> {
    let mut result = issues.to_vec();
    result.sort_by(|a, b| {
        (
            a.root_index,
            &a.relative,
            a.kind,
            a.stage,
            &a.reason_key,
            a.observed_at.as_ref().map(Timestamp::unix_millis),
        )
            .cmp(&(
                b.root_index,
                &b.relative,
                b.kind,
                b.stage,
                &b.reason_key,
                b.observed_at.as_ref().map(Timestamp::unix_millis),
            ))
    });
    result
}
