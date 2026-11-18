//! Orders result version notifications independently of read admission.
use crate::version_store::Entry;
use crate::{BuildHandle, IngestSink, ResultVersion};
use du_core_contracts::{
    Id, SizeAmount,
    result::ResultCompleteness,
    result_events::{
        PublishedCompleteness, ResultVersionChange, ResultVersionChanged, ResultVersionNotice,
        ResultVersionPublisher,
    },
    result_freshness::{ResultFreshnessNotice, ResultFreshnessPublisher},
};
use std::sync::{Arc, Mutex, Weak};

/// A holder of results derived from published versions that must drop them on retirement.
///
/// A version is retired once it can never become current again. Implementations run with
/// no result-store lock held and may take their own locks.
pub(crate) trait RetiredVersions: Send + Sync {
    /// Drops everything derived from `version`; readers that already pinned it keep reading.
    fn retire(&self, version: &Id);
}

#[derive(Default)]
pub(super) struct PublicationEvents {
    pub order: Mutex<()>,
    publisher: Option<Arc<dyn ResultVersionPublisher>>,
    freshness: Option<Arc<dyn ResultFreshnessPublisher>>,
    subscribers: Mutex<Vec<Weak<dyn RetiredVersions>>>,
}

impl PublicationEvents {
    pub(super) fn new(
        publisher: Arc<dyn ResultVersionPublisher>,
        freshness: Option<Arc<dyn ResultFreshnessPublisher>>,
    ) -> Self {
        Self {
            order: Mutex::new(()),
            publisher: Some(publisher),
            freshness,
            subscribers: Mutex::default(),
        }
    }

    /// Whether freshness notices are wanted; a store without a publisher keeps no ledger.
    pub(super) fn tracks_freshness(&self) -> bool {
        self.freshness.is_some()
    }

    /// Sends freshness notices in order, while holding the order guard after the version notice.
    pub(super) fn notify_freshness(&self, notices: Vec<ResultFreshnessNotice>) {
        if let Some(publisher) = &self.freshness {
            for notice in notices {
                publisher.publish_result_freshness(notice);
            }
        }
    }

    /// Registers a holder for retirement calls; it is forgotten once its last `Arc` is gone.
    pub(super) fn subscribe(&self, subscriber: Weak<dyn RetiredVersions>) {
        if let Ok(mut subscribers) = self.subscribers.lock() {
            subscribers.retain(|existing| existing.strong_count() != 0);
            subscribers.push(subscriber);
        }
    }

    /// Tells every live subscriber that `version` can never become current again.
    pub(super) fn retire(&self, version: &Id) {
        // Call outside the list lock so a subscriber may take its own locks freely.
        let live: Vec<Arc<dyn RetiredVersions>> = match self.subscribers.lock() {
            Ok(mut subscribers) => {
                subscribers.retain(|existing| existing.strong_count() != 0);
                subscribers.iter().filter_map(Weak::upgrade).collect()
            }
            Err(_) => return,
        };
        for subscriber in live {
            subscriber.retire(version);
        }
    }

    /// Sends the change after releasing the store lock, while holding the order guard.
    pub(super) fn notify(
        &self,
        lineage: Option<&Id>,
        sequence: Option<u64>,
        change: ResultVersionChange,
        previous: Option<&ResultVersion>,
        current: Option<&ResultVersion>,
    ) {
        let (Some(publisher), Some(lineage), Some(sequence), Some(relevant)) =
            (&self.publisher, lineage, sequence, current.or(previous))
        else {
            return;
        };
        publisher.publish_result_version(ResultVersionNotice {
            result_version: current.map(|result| result.version_id.clone()),
            body: ResultVersionChanged {
                lineage_id: lineage.clone(),
                source_ref: lineage.clone(),
                origin: relevant.origin().into(),
                data_source: Some(relevant.data_source),
                sequence,
                change,
                previous_result_version: previous.map(|result| result.version_id.clone()),
                completeness: current.map(completeness),
                observed_at: current.and_then(|result| result.observed_at.clone()),
            },
        });
    }
}

impl IngestSink {
    /// Creates the store with a freshness channel beside the version channel.
    ///
    /// Only a store with a freshness publisher keeps freshness ledgers.
    pub fn with_publishers(
        versions: Arc<dyn ResultVersionPublisher>,
        freshness: Arc<dyn ResultFreshnessPublisher>,
    ) -> Self {
        Self {
            state: Arc::default(),
            events: Arc::new(PublicationEvents::new(versions, Some(freshness))),
            folder_evidence_budget: Arc::default(),
        }
    }
}

impl BuildHandle {
    /// Retires the versions a publication made unreachable, after the store lock is released.
    ///
    /// `previous` is what the store replaced and `was_published` is whether this build had
    /// published before. A first, non-final publication replaces the baseline, which an
    /// abort restores, so it stays. Sealing is irreversible and retires the baseline too.
    /// Call after the order guard and the store lock are released, and before the replaced
    /// entry is dropped.
    pub(super) fn retire_replaced(
        &self,
        previous: Option<&Entry>,
        was_published: bool,
        finish: bool,
    ) {
        let previous = previous.map(|entry| &entry.result.version_id);
        if let Some(id) = previous.filter(|_| was_published || finish) {
            self.events.retire(id);
        }
        if finish && let Some(base) = &self.baseline {
            let id = &base.result.version_id;
            if previous != Some(id) {
                self.events.retire(id);
            }
        }
    }
}

fn completeness(result: &ResultVersion) -> PublishedCompleteness {
    if result.completeness == ResultCompleteness::Complete {
        return PublishedCompleteness::Complete;
    }
    let aggregate = result.scope_total();
    if [
        &aggregate.logical,
        &aggregate.allocated,
        &aggregate.unique_physical,
    ]
    .iter()
    .all(|value| value.amount == SizeAmount::Unknown)
    {
        PublishedCompleteness::Unknown
    } else {
        PublishedCompleteness::Partial
    }
}
