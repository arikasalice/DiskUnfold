use crate::search::{SearchEntry, SearchRequest};
use crate::{
    api::{CallerCtx, Channel, Engine, EngineBuilder},
    desktop::DesktopEvents,
    workers::Workers,
};
use du_analysis_index::{
    IngestSink, Observation, query,
    query::{Condition, SortSpec},
};
use du_core_contracts::{
    CallEnvelope, ENVELOPE_VERSION,
    job::{JobHistory, JobSummary},
    ports::{PortError, Random},
    query_region_ipc::QueryRegionKind,
    result::ResultCompleteness,
};
use du_job_runtime::{JobClock, JobIdentity, JobRuntime};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

static CALL_SEQUENCE: AtomicUsize = AtomicUsize::new(1);
static HARNESS_SEQUENCE: AtomicUsize = AtomicUsize::new(120);

pub(super) struct TestClock;
impl JobClock for TestClock {
    fn monotonic_now(&self) -> Option<Duration> {
        Some(Duration::ZERO)
    }
    fn now_ms(&self) -> i64 {
        1
    }
}

pub(super) struct TestHistory;
impl JobHistory for TestHistory {
    fn append(&self, _: &JobSummary) -> Result<du_core_contracts::job::HistoryAppend, String> {
        Ok(du_core_contracts::job::HistoryAppend::Added)
    }
}

pub(super) struct TestEntropy;
impl Random for TestEntropy {
    fn fill(&self, bytes: &mut [u8]) -> Result<(), PortError> {
        bytes.fill(9);
        Ok(())
    }
}

pub(super) fn id(value: u8) -> du_core_contracts::Id {
    let mut bytes = [0; 16];
    bytes[6] = 0x70;
    bytes[8] = 0x80;
    bytes[15] = value;
    du_core_contracts::Id::from_v7_bytes(bytes)
}

struct HarnessIds(AtomicUsize);
impl JobIdentity for HarnessIds {
    fn next_id(&self) -> du_core_contracts::Id {
        id(u8::try_from(self.0.fetch_add(1, Ordering::SeqCst)).unwrap_or(250))
    }
}

struct HarnessEntropy(u8);
impl Random for HarnessEntropy {
    fn fill(&self, bytes: &mut [u8]) -> Result<(), PortError> {
        bytes.fill(self.0);
        Ok(())
    }
}

pub(super) struct Harness {
    pub(super) engine: Arc<Engine>,
    pub(super) runtime: Arc<JobRuntime>,
    pub(super) events: Arc<DesktopEvents>,
    pub(super) entry: Arc<SearchEntry>,
    pub(super) version: du_core_contracts::Id,
    pub(super) search_region: du_core_contracts::Id,
    pub(super) browse_region: du_core_contracts::Id,
}

pub(super) struct ClearTestGates;
impl Drop for ClearTestGates {
    fn drop(&mut self) {
        let _ = crate::search::test_gate::set(None);
        let _ = crate::search::test_gate::set_cancel_gate(None);
    }
}

pub(super) fn make_harness() -> Result<Harness, String> {
    let store = IngestSink::default();
    let mut scan = store.open(id(1)).map_err(|error| format!("{error:?}"))?;
    scan.add_directory(Vec::new())
        .map_err(|error| format!("{error:?}"))?;
    scan.add_file(Observation {
        path: vec![b"item".to_vec()],
        size: None,
    })
    .map_err(|error| format!("{error:?}"))?;
    let version = id(82);
    scan.seal(version.clone(), ResultCompleteness::Complete)
        .map_err(|error| format!("{error:?}"))?;

    let identity_start = HARNESS_SEQUENCE.fetch_add(10, Ordering::SeqCst);
    let ids = Arc::new(HarnessIds(AtomicUsize::new(identity_start)));
    let entropy = HarnessEntropy(identity_start as u8);
    let events = Arc::new(DesktopEvents::default());
    let runtime = Arc::new(
        JobRuntime::new(
            ids.clone(),
            Arc::new(TestClock),
            Arc::new(TestHistory),
            events.clone(),
        )
        .map_err(|error| format!("{error:?}"))?,
    );
    let workers = Arc::new(Workers::start(runtime.clone()).map_err(|error| format!("{error:?}"))?);
    let mut builder = EngineBuilder::default();
    crate::jobs::register(&mut builder, runtime.clone());
    let entry = crate::browse::folder::commands::register(
        &mut builder,
        Arc::new(du_analysis_index::query::page::ResultSets::new(store)),
        ids,
        workers,
        &entropy,
    )
    .map_err(|error| format!("{error:?}"))?
    .ok_or("failed to register browse/search session")?;
    let engine = Arc::new(builder.build().map_err(|error| error.to_string())?);
    let search_region = entry
        .regions
        .registry
        .open(&entry.regions.owner, QueryRegionKind::Search)
        .map_err(|error| format!("{error:?}"))?;
    let browse_region = entry
        .regions
        .registry
        .open(&entry.regions.owner, QueryRegionKind::BrowseList)
        .map_err(|error| format!("{error:?}"))?;
    Ok(Harness {
        engine,
        runtime,
        events,
        entry,
        version,
        search_region,
        browse_region,
    })
}

pub(super) fn call(
    engine: &Engine,
    name: &str,
    body: Value,
    correlation: u8,
) -> du_core_contracts::ResultEnvelope {
    let correlation_id =
        id(correlation.wrapping_add(CALL_SEQUENCE.fetch_add(1, Ordering::SeqCst) as u8));
    engine.call(
        CallEnvelope {
            name: name.into(),
            contract_version: ENVELOPE_VERSION,
            correlation_id: correlation_id.clone(),
            body,
        },
        &CallerCtx::new(Channel::Gui, correlation_id, None),
    )
}

pub(super) fn request_cancel(
    engine: &Engine,
    region: &du_core_contracts::Id,
    seq: u32,
) -> du_core_contracts::ResultEnvelope {
    call(
        engine,
        "query.cancel",
        json!({"regionRef":region,"target":{"kind":"request","requestSeq":seq}}),
        211,
    )
}

pub(super) fn browse_query(harness: &Harness) -> Result<du_core_contracts::Id, String> {
    let open = call(
        &harness.engine,
        "browse.open",
        json!({
            "regionRef":harness.browse_region,
            "viewKind":"list",
            "requestSeq":1,
            "resultVersion":harness.version,
            "pageSize":10,
        }),
        212,
    );
    if let Some(error) = open.error {
        return Err(format!("browse.open: {}", error.reason_key));
    }
    if open.body["kind"] != "opened" {
        return Err(format!("expected sync browse open, got {}", open.body));
    }
    let query_ref = serde_json::from_value(open.body["page"]["queryRef"].clone())
        .map_err(|error| error.to_string())?;
    let page = call(
        &harness.engine,
        "query.page",
        json!({"queryRef":query_ref,"resultVersion":harness.version,"start":0,"count":10}),
        213,
    );
    if let Some(error) = page.error {
        return Err(format!("Browse result unreadable: {}", error.reason_key));
    }
    if page.body["rowKind"] != "folder" {
        return Err(format!("expected Browse result, got {}", page.body));
    }
    Ok(query_ref)
}

pub(super) fn search_request(
    region_ref: du_core_contracts::Id,
    version: du_core_contracts::Id,
) -> SearchRequest {
    SearchRequest {
        region_ref,
        request_seq: 1,
        result_version: Some(version),
        parent_search_ref: None,
        condition: Condition::All(Vec::new()),
        sorts: vec![SortSpec {
            id: "sort.path".into(),
            revision: 1,
            basis: query::Basis::Logical,
            descending: false,
        }],
        count: None,
    }
}
