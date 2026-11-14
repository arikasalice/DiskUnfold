//! Orders result version notifications independently of read admission.
use crate::ResultVersion;
use du_core_contracts::{
    Id, SizeAmount,
    result::ResultCompleteness,
    result_events::{
        PublishedCompleteness, ResultVersionChange, ResultVersionChanged, ResultVersionNotice,
        ResultVersionPublisher,
    },
};
use std::sync::{Arc, Mutex};

#[derive(Default)]
pub(super) struct PublicationEvents {
    pub order: Mutex<()>,
    publisher: Option<Arc<dyn ResultVersionPublisher>>,
}

impl PublicationEvents {
    pub(super) fn new(publisher: Arc<dyn ResultVersionPublisher>) -> Self {
        Self {
            order: Mutex::new(()),
            publisher: Some(publisher),
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
