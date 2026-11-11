use super::*;
use crate::{api::EngineBuilder, desktop::DesktopEvents, query_region::commands::Regions};
use du_analysis_index::{IngestSink, Observation, query};
use du_core_contracts::{
    job::{HistoryAppend, JobHistory, JobSummary},
    ports::{PortError, Random},
    result::ResultCompleteness,
};
use du_job_runtime::{JobClock, JobRuntime};
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

struct Identity {
    next: AtomicUsize,
    cancel_on: AtomicUsize,
    replace_on: AtomicUsize,
    caller: Mutex<Option<CallerCtx>>,
    store: Mutex<Option<IngestSink>>,
}
impl JobIdentity for Identity {
    fn next_id(&self) -> Id {
        let sequence = self.next.fetch_add(1, Ordering::SeqCst);
        if sequence == self.replace_on.load(Ordering::SeqCst)
            && let Ok(store) = self.store.lock()
            && let Some(store) = store.as_ref()
            && let Ok(mut scan) = store.open(id(81))
            && scan.add_directory(vec![]).is_ok()
            && scan
                .add_file(Observation {
                    path: vec![b"replacement".to_vec()],
                    size: None,
                })
                .is_ok()
        {
            let _ = scan.seal(id(82), ResultCompleteness::Complete);
        }
        if sequence == self.cancel_on.load(Ordering::SeqCst)
            && let Ok(caller) = self.caller.lock()
            && let Some(caller) = caller.as_ref()
        {
            caller.cancel();
        }
        let mut bytes = [0; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[15] = u8::try_from(sequence).unwrap_or(0);
        Id::from_v7_bytes(bytes)
    }
}
struct Clock;
impl JobClock for Clock {
    fn monotonic_now(&self) -> Option<Duration> {
        Some(Duration::ZERO)
    }
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
struct Entropy;
impl Random for Entropy {
    fn fill(&self, bytes: &mut [u8]) -> Result<(), PortError> {
        bytes.fill(9);
        Ok(())
    }
}
fn id(n: u8) -> Id {
    let mut bytes = [0; 16];
    bytes[6] = 0x70;
    bytes[8] = 0x80;
    bytes[15] = n;
    Id::from_v7_bytes(bytes)
}

#[test]
fn invalid_cancelled_and_replaced_searches_preserve_published_results() -> Result<(), String> {
    let store = IngestSink::default();
    let mut scan = store.open(id(1)).map_err(|error| format!("{error:?}"))?;
    scan.add_directory(vec![])
        .map_err(|error| format!("{error:?}"))?;
    scan.add_file(Observation {
        path: vec![b"one".to_vec()],
        size: None,
    })
    .map_err(|error| format!("{error:?}"))?;
    scan.seal(id(2), ResultCompleteness::Complete)
        .map_err(|error| format!("{error:?}"))?;
    let sets = Arc::new(ResultSets::new(store));
    let old = query::run(
        &Registry::baseline(),
        sets.source()
            .pin()
            .map_err(|error| format!("{error:?}"))?
            .ok_or("no source")?,
        &Condition::All(vec![]),
        query::Sort {
            id: "sort.path",
            revision: 1,
            basis: query::Basis::Logical,
            descending: false,
        },
        || false,
    )
    .map_err(|error| format!("{error:?}"))?;
    sets.register(id(80), old)
        .map_err(|error| format!("{error:?}"))?;

    let ids = Arc::new(Identity {
        next: AtomicUsize::new(0),
        cancel_on: AtomicUsize::new(usize::MAX),
        replace_on: AtomicUsize::new(usize::MAX),
        caller: Mutex::new(None),
        store: Mutex::new(Some(sets.source())),
    });
    let runtime = Arc::new(
        JobRuntime::new(
            ids.clone(),
            Arc::new(Clock),
            Arc::new(History),
            Arc::new(DesktopEvents::default()),
        )
        .map_err(|error| format!("{error:?}"))?,
    );
    let mut builder = EngineBuilder::default();
    let regions = Regions::register(&mut builder, ids.clone(), &Entropy, runtime)
        .map_err(|error| format!("{error:?}"))?;
    let region_ref = regions
        .registry
        .open(&regions.owner, QueryRegionKind::Search)
        .map_err(|error| format!("{error:?}"))?;
    let caller = CallerCtx::new(Channel::Gui, id(4), None);
    let entry = SearchEntry::new(sets.clone(), regions, ids.clone());
    let make_request =
        |request_seq, sort_id: &str, parent_search_ref, result_version, condition| SearchRequest {
            region_ref: region_ref.clone(),
            request_seq,
            result_version,
            parent_search_ref,
            condition,
            sorts: vec![SortSpec {
                id: sort_id.into(),
                revision: 1,
                basis: query::Basis::Logical,
                descending: false,
            }],
            count: None,
        };

    let first = entry
        .execute(
            make_request(1, "sort.path", None, Some(id(2)), Condition::All(vec![])),
            &caller,
        )
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(first.items().len(), 2);

    assert!(matches!(
        entry.execute(
            make_request(
                2,
                "sort.path",
                None,
                Some(id(2)),
                Condition::Predicate(query::Predicate::Registered {
                    id: "pred.missing".into(),
                    revision: 1,
                    operand: Vec::new(),
                }),
            ),
            &caller
        ),
        Err(SearchError::InvalidRequest)
    ));
    let old_page = sets
        .open_page(first.query_ref(), &id(2), 0, None)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(old_page.items().len(), 2);

    assert!(matches!(
        entry.execute(
            make_request(
                2,
                "sort.path",
                Some(first.query_ref().clone()),
                Some(id(82)),
                Condition::All(vec![])
            ),
            &caller
        ),
        Err(SearchError::StaleVersion)
    ));
    let refined = entry
        .execute(
            make_request(
                2,
                "sort.path",
                Some(first.query_ref().clone()),
                Some(id(2)),
                Condition::All(vec![]),
            ),
            &caller,
        )
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(refined.items().len(), 2);

    ids.cancel_on.store(5, Ordering::SeqCst);
    *ids.caller.lock().map_err(|_| "identity lock poisoned")? = Some(caller.clone());
    let outcome = entry.execute(
        make_request(3, "sort.path", None, Some(id(2)), Condition::All(vec![])),
        &caller,
    );
    assert!(matches!(outcome, Err(SearchError::Cancelled)));
    let old_page = sets
        .open_page(refined.query_ref(), &id(2), 0, None)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(old_page.items().len(), 2);

    ids.cancel_on.store(usize::MAX, Ordering::SeqCst);
    ids.replace_on.store(6, Ordering::SeqCst);
    let replacement_caller = CallerCtx::new(Channel::Gui, id(5), None);
    let replaced = entry
        .execute(
            make_request(4, "sort.path", None, Some(id(2)), Condition::All(vec![])),
            &replacement_caller,
        )
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(replaced.metadata().version, &id(2));
    assert_eq!(replaced.items().len(), 2);
    assert!(matches!(
        sets.open_page(replaced.query_ref(), &id(2), 0, None),
        Err(PageError::StaleVersion)
    ));
    assert_eq!(first.items().len(), 2);
    Ok(())
}
