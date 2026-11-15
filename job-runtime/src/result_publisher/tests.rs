use crate::{JobClock, JobEvent, JobEvents, JobIdentity, JobRuntime};
use du_core_contracts::{
    Id,
    job::{HistoryAppend, JobHistory, JobSummary},
    result_events::{
        PublishedCompleteness, ResultVersionChange, ResultVersionChanged, ResultVersionNotice,
    },
    result_freshness::{
        FreshnessChanged, FreshnessCounts, FreshnessState, FreshnessSummary,
        ResultFreshnessChanged, ResultFreshnessNotice,
    },
};
use std::sync::{Arc, Mutex};

type Check = Result<(), Box<dyn std::error::Error>>;

struct Identity(Id);
impl JobIdentity for Identity {
    fn next_id(&self) -> Id {
        self.0.clone()
    }
}
struct Clock;
impl JobClock for Clock {
    fn now_ms(&self) -> i64 {
        1
    }
}
struct History;
impl JobHistory for History {
    fn append(&self, _: &JobSummary) -> Result<HistoryAppend, String> {
        Ok(HistoryAppend::Added)
    }
}
#[derive(Default)]
struct Events(Mutex<Vec<JobEvent>>);
impl JobEvents for Events {
    fn publish(&self, event: JobEvent) {
        if let Ok(mut events) = self.0.lock() {
            events.push(event);
        }
    }
}

fn id(last: u8) -> Result<Id, Box<dyn std::error::Error>> {
    Ok(Id::parse(&format!(
        "00000000-0000-7000-8000-0000000000{last:02}"
    ))?)
}

fn freshness(
    sequence: u64,
    current: bool,
) -> Result<ResultFreshnessNotice, Box<dyn std::error::Error>> {
    let counts = FreshnessCounts {
        unwatched: u64::from(current),
        ..FreshnessCounts::default()
    };
    Ok(ResultFreshnessNotice {
        result_version: current.then(|| id(3)).transpose()?,
        body: ResultFreshnessChanged {
            lineage_id: id(1)?,
            scope_version_id: id(2)?,
            sequence,
            summary: FreshnessSummary {
                representative: FreshnessState::new(if current {
                    FreshnessState::UNWATCHED
                } else {
                    FreshnessState::UNVERIFIABLE
                }),
                counts,
            },
            changed: FreshnessChanged::Requery,
            observed_at: None,
        },
    })
}

#[test]
fn freshness_notices_keep_their_order_and_sequence_beside_version_notices() -> Check {
    let events = Arc::new(Events::default());
    let runtime = JobRuntime::new(
        Arc::new(Identity(id(9)?)),
        Arc::new(Clock),
        Arc::new(History),
        events.clone(),
    )?;
    let versions = runtime.result_version_publisher();
    let publisher = runtime.result_freshness_publisher();
    versions.publish_result_version(ResultVersionNotice {
        result_version: Some(id(3)?),
        body: ResultVersionChanged {
            lineage_id: id(1)?,
            source_ref: id(1)?,
            origin: "scan".into(),
            data_source: None,
            sequence: 1,
            change: ResultVersionChange::Published,
            previous_result_version: None,
            completeness: Some(PublishedCompleteness::Complete),
            observed_at: None,
        },
    });
    // An invalid notice is dropped; valid ones are neither merged nor reordered.
    publisher.publish_result_freshness(freshness(0, true)?);
    for sequence in [1, 2, 3] {
        publisher.publish_result_freshness(freshness(sequence, true)?);
    }
    publisher.publish_result_freshness(freshness(4, false)?);
    runtime.flush()?;
    let events = events.0.lock().map_err(|_| "events poisoned")?;
    assert!(matches!(events[0], JobEvent::ResultVersionChanged(_)));
    let sequences: Vec<(u64, bool)> = events[1..]
        .iter()
        .filter_map(|event| match event {
            JobEvent::ResultFreshnessChanged(notice) => {
                Some((notice.body.sequence, notice.result_version.is_some()))
            }
            _ => None,
        })
        .collect();
    assert_eq!(sequences, vec![(1, true), (2, true), (3, true), (4, false)]);
    Ok(())
}
