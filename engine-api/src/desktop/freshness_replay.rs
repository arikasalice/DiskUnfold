//! Retains the latest freshness event of each scope version for windows that missed it.
//!
//! The desktop stream is lossy by design: a window that opens late, or a queue that
//! overflows, misses events. The store keeps one event per scope version in session
//! memory and hands out copies that carry `requery`, because the retained change list
//! only describes the step before it. A copy keeps the sequence and the summary.
//!
//! The envelope `seq` is not used to detect gaps on this topic; gaps and order are judged
//! only by the body `sequence`. A retained copy holds envelope `seq` 0. The queue that
//! hands the copy out assigns its own transport sequence, as it does for every event, so
//! the copy leaves the desktop with a nonzero, rising envelope `seq` and the original body
//! `sequence`.
use du_core_contracts::{
    EventEnvelope,
    result_freshness::{FreshnessChanged, RESULT_FRESHNESS_CHANGED, ResultFreshnessChanged},
};
use std::collections::BTreeMap;

/// The newest requery copy per scope version, bounded by the live ledgers.
///
/// An event without a current version and with all counts zero says the ledger is gone,
/// and removes the copy, so the map never outgrows the set of scope versions that exist.
#[derive(Default)]
pub(super) struct LatestFreshness {
    by_scope: BTreeMap<String, (u64, EventEnvelope)>,
}

impl LatestFreshness {
    /// Notes a queued event; events of other topics and malformed bodies are ignored.
    pub(super) fn record(&mut self, event: &EventEnvelope) {
        if event.topic != RESULT_FRESHNESS_CHANGED {
            return;
        }
        let Ok(mut body) = serde_json::from_value::<ResultFreshnessChanged>(event.body.clone())
        else {
            return;
        };
        let key = body.scope_version_id.as_str().to_owned();
        if event.result_version.is_none() && body.summary.counts.total() == 0 {
            self.by_scope.remove(&key);
            return;
        }
        if self
            .by_scope
            .get(&key)
            .is_some_and(|(sequence, _)| *sequence > body.sequence)
        {
            return;
        }
        let sequence = body.sequence;
        body.changed = FreshnessChanged::Requery;
        let Ok(value) = serde_json::to_value(body) else {
            return;
        };
        let mut copy = event.clone();
        // Placeholder only: `Pending::take` overwrites it with the transport sequence.
        copy.seq = 0;
        copy.body = value;
        self.by_scope.insert(key, (sequence, copy));
    }

    /// Returns the copies to send, in scope version order.
    pub(super) fn replays(&self) -> Vec<EventEnvelope> {
        self.by_scope
            .values()
            .map(|(_, event)| event.clone())
            .collect()
    }
}
