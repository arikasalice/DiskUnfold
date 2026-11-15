//! Freshness ledger changes made together with result-store state changes.
use super::{Lineage, State};
use crate::{ResultVersion, freshness_ledger::LedgerVersion};
use du_core_contracts::{Id, result_freshness::ResultFreshnessNotice};
use std::{collections::HashSet, sync::Arc};

impl State {
    // Everything below runs under the result-store lock, which query admissions wait on.
    // Only O(roots) work is allowed here; the ledger gets a `LedgerVersion` header that
    // cannot reach items or gaps, and per-root outcomes are computed before the lock.

    /// Returns the lineage whose results keep a freshness ledger: a real, bound lineage.
    fn freshness_lineage(&self, lineage: &Lineage) -> Option<Id> {
        lineage
            .as_ref()
            .filter(|id| !self.sample_lineages.contains_key(*id))
            .cloned()
    }

    /// Updates the ledgers for a publication that this state already accepted.
    ///
    /// `first` marks the first publication of the build, and `finish` its seal. Call under
    /// the same lock hold as the publication and send the notices after releasing it.
    pub(crate) fn freshness_published(
        &mut self,
        lineage: &Lineage,
        result: &Arc<ResultVersion>,
        first: bool,
        finish: bool,
    ) -> Vec<ResultFreshnessNotice> {
        let Some(id) = self.freshness_lineage(lineage) else {
            return Vec::new();
        };
        if !result.data_source.is_real() {
            return Vec::new();
        }
        let mut notices: Vec<_> = self
            .freshness
            .publish(&id, &result.ledger_version(), first, finish)
            .into_iter()
            .collect();
        notices.extend(self.prune_freshness());
        notices
    }

    /// Updates the ledgers for a withdrawn build after `abort` restored `baseline`.
    pub(crate) fn freshness_aborted(
        &mut self,
        lineage: &Lineage,
        build_scope: &Id,
        baseline: Option<&Arc<ResultVersion>>,
    ) -> Vec<ResultFreshnessNotice> {
        let Some(id) = self.freshness_lineage(lineage) else {
            return Vec::new();
        };
        let mut notices: Vec<_> = baseline
            .filter(|base| base.data_source.is_real())
            .and_then(|base| {
                self.freshness
                    .abort(&id, build_scope, &base.ledger_version())
            })
            .into_iter()
            .collect();
        notices.extend(self.prune_freshness());
        notices
    }

    /// Removes the ledgers of scope versions that no lineage's current version uses.
    fn prune_freshness(&mut self) -> Vec<ResultFreshnessNotice> {
        let live: HashSet<&Id> = self
            .current
            .values()
            .map(|entry| &entry.result.scope_version_id)
            .collect();
        self.freshness.prune(&live)
    }
}

impl ResultVersion {
    /// Borrows what the freshness ledger reads; O(1), and no access to items or gaps.
    fn ledger_version(&self) -> LedgerVersion<'_> {
        LedgerVersion {
            version_id: &self.version_id,
            scope_version_id: &self.scope_version_id,
            root_count: self.roots.len(),
            observed_at: self.observed_at.as_ref(),
            outcomes: self.freshness_outcomes.as_deref(),
        }
    }
}
