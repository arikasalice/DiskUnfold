use super::*;
use crate::{api::Engine, desktop::DesktopEvents};
use du_analysis_index::IngestSink;
use du_core_contracts::{
    CallEnvelope, ENVELOPE_VERSION, Id, ResultEnvelope,
    job::{HistoryAppend, JobHistory, JobState, JobSummary},
    ports::PortError,
    result::ResultCompleteness,
};
use du_job_runtime::{JobClock, JobRuntime};
use serde_json::{Value, json};

mod admission;
mod completion;
mod delivery_busy;
mod search;
static GATE_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct Ids;
impl JobIdentity for Ids {
    fn next_id(&self) -> Id {
        Id::from_v7_bytes(*uuid::Uuid::now_v7().as_bytes())
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
struct Entropy;
impl Random for Entropy {
    fn fill(&self, bytes: &mut [u8]) -> Result<(), PortError> {
        bytes.fill(1);
        Ok(())
    }
}

fn fixture(children: usize) -> Result<(Engine, Arc<JobRuntime>, Id), Box<dyn std::error::Error>> {
    let store = IngestSink::default();
    let mut scan = store
        .open(Ids.next_id())
        .map_err(|error| format!("{error:?}"))?;
    scan.add_directory(vec![])
        .map_err(|error| format!("{error:?}"))?;
    for index in 0..children {
        scan.add_directory(vec![format!("folder-{index:05}").into_bytes()])
            .map_err(|error| format!("{error:?}"))?;
    }
    let version = Ids.next_id();
    scan.seal(version.clone(), ResultCompleteness::Complete)
        .map_err(|error| format!("{error:?}"))?;
    let (engine, runtime) = register_fixture(Arc::new(ResultSets::new(store)))?;
    Ok((engine, runtime, version))
}

fn register_fixture(
    sets: Arc<ResultSets>,
) -> Result<(Engine, Arc<JobRuntime>), Box<dyn std::error::Error>> {
    let runtime = Arc::new(JobRuntime::new(
        Arc::new(Ids),
        Arc::new(Clock),
        Arc::new(History),
        Arc::new(DesktopEvents::default()),
    )?);
    let workers = Arc::new(Workers::start(runtime.clone())?);
    let mut builder = EngineBuilder::default();
    crate::space::register_total_with_queries(&mut builder, sets.clone());
    crate::jobs::register(&mut builder, runtime.clone());
    register(&mut builder, sets, Arc::new(Ids), workers, &Entropy)?;
    Ok((builder.build()?, runtime))
}

fn call(engine: &Engine, name: &str, body: Value, channel: Channel) -> ResultEnvelope {
    let correlation_id = Ids.next_id();
    let caller = CallerCtx::new(channel, correlation_id.clone(), None);
    engine.call(
        CallEnvelope {
            name: name.into(),
            contract_version: ENVELOPE_VERSION,
            correlation_id,
            body,
        },
        &caller,
    )
}

fn region(engine: &Engine) -> Value {
    let result = call(
        engine,
        "query.region.open",
        json!({"kind":"browse_list"}),
        Channel::Gui,
    );
    assert!(result.error.is_none(), "{:?}", result.error);
    result.body["regionRef"].clone()
}

#[test]
fn registered_gui_commands_open_and_continue_with_the_same_registry()
-> Result<(), Box<dyn std::error::Error>> {
    let (engine, _, version) = fixture(2)?;
    let region_ref = region(&engine);
    let request = json!({"regionRef":region_ref,"viewKind":"list","requestSeq":4,"resultVersion":version,"pageSize":1});
    let denied = call(&engine, "browse.open", request.clone(), Channel::Cli);
    assert_eq!(
        denied.error.ok_or("expected channel denial")?.reason_key,
        "command.not_provided"
    );
    let opened = call(&engine, "browse.open", request, Channel::Gui);
    assert!(opened.error.is_none(), "{:?}", opened.error);
    assert_eq!(opened.body["kind"], "opened");
    assert_eq!(opened.body["requestSeq"], 4);
    let first = &opened.body["page"];
    assert!(first.get("browseContext").is_none());
    let next = call(
        &engine,
        "query.page",
        json!({"nextCursor":first["nextCursor"],"count":1}),
        Channel::Gui,
    );
    assert!(next.error.is_none(), "{:?}", next.error);
    assert_eq!(next.body["queryRef"], first["queryRef"]);
    assert_eq!(next.body["resultVersion"], first["resultVersion"]);
    assert_ne!(next.body["rows"][0]["item"], first["rows"][0]["item"]);
    assert_eq!(next.body["nextCursor"], Value::Null);
    assert!(next.body.get("browseContext").is_none());
    Ok(())
}

fn await_query(runtime: &JobRuntime, job: &Id) -> Result<Id, Box<dyn std::error::Error>> {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(4) {
        let snapshot = runtime.snapshot(job)?;
        if snapshot.state.is_terminal() {
            assert_eq!(
                snapshot.state,
                JobState::Done,
                "{:?}",
                snapshot.failure_reason_key
            );
            return snapshot
                .result_version_id
                .ok_or_else(|| "missing query result reference".into());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    Err("folder job did not finish before the test deadline".into())
}

static PREPARE_GATE: std::sync::OnceLock<Arc<super::super::test_gate::Gate>> =
    std::sync::OnceLock::new();
static PAUSE_STAGE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
pub(super) fn pause_at(sequence: u32, stage: usize) {
    if sequence == 90001
        && PAUSE_STAGE.load(std::sync::atomic::Ordering::Acquire) == stage
        && let Some(gate) = PREPARE_GATE.get()
    {
        assert!(gate.pause().is_ok(), "test gate release timed out");
    }
}

pub(super) fn force_sync(sequence: u32) -> bool {
    sequence == 90001 && PAUSE_STAGE.load(std::sync::atomic::Ordering::Acquire) == 3
}

#[test]
fn late_preparation_cannot_supersede_newer_open() -> Result<(), Box<dyn std::error::Error>> {
    let _serial = GATE_TEST_LOCK.lock().map_err(|_| "gate lock poisoned")?;
    let gate = PREPARE_GATE.get_or_init(super::super::test_gate::Gate::new);
    for stage in 1..=4 {
        let (engine, runtime, version) = fixture(if stage == 4 { SYNC_WORK } else { 2 })?;
        let region_ref = region(&engine);
        let engine = Arc::new(engine);
        if stage == 2 {
            let prime = call(
                &engine,
                "browse.open",
                json!({"regionRef":region_ref,"viewKind":"list","requestSeq":1,"resultVersion":version}),
                Channel::Gui,
            );
            assert!(prime.error.is_none(), "cache setup: {:?}", prime.error);
        }
        PAUSE_STAGE.store(stage, std::sync::atomic::Ordering::Release);
        let prior_engine = engine.clone();
        let prior_version = version.clone();
        let prior_region = region_ref.clone();
        let prior = std::thread::spawn(move || {
            call(
                &prior_engine,
                "browse.open",
                json!({"regionRef":prior_region,"viewKind":"list","requestSeq":90001,"resultVersion":prior_version}),
                Channel::Gui,
            )
        });
        gate.entered()?;
        let newer = call(
            &engine,
            "browse.open",
            json!({"regionRef":region_ref,"viewKind":"list","requestSeq":90002,"resultVersion":version}),
            Channel::Gui,
        );
        gate.release()?;
        let old = prior.join().map_err(|_| "old thread panicked")?;
        assert!(newer.error.is_none(), "stage {stage}: {:?}", newer.error);
        assert!(
            old.error.is_some(),
            "stage {stage}: superseded request was accepted"
        );
        if stage == 4 {
            let job: Id = serde_json::from_value(newer.body["jobRef"].clone())?;
            await_query(&runtime, &job)?;
        }
        let retry = call(
            &engine,
            "browse.open",
            json!({"regionRef":region_ref,"viewKind":"list","requestSeq":90003,"resultVersion":version}),
            Channel::Gui,
        );
        assert!(
            retry.error.is_none(),
            "stage {stage}: retry failed: {:?}",
            retry.error
        );
    }
    // A later rejected reservation must not displace an earlier valid preparation.
    let (engine, _, version) = fixture(2)?;
    let region_ref = region(&engine);
    let engine = Arc::new(engine);
    PAUSE_STAGE.store(1, std::sync::atomic::Ordering::Release);
    let prior_engine = engine.clone();
    let prior_version = version.clone();
    let prior_region = region_ref.clone();
    let prior = std::thread::spawn(move || {
        call(
            &prior_engine,
            "browse.open",
            json!({"regionRef":prior_region,"viewKind":"list","requestSeq":90001,"resultVersion":prior_version}),
            Channel::Gui,
        )
    });
    gate.entered()?;
    let invalid = call(
        &engine,
        "browse.open",
        json!({"regionRef":region_ref,"viewKind":"list","requestSeq":2,"resultVersion":version,"pageSize":0}),
        Channel::Gui,
    );
    gate.release()?;
    assert!(invalid.error.is_some());
    assert!(
        prior
            .join()
            .map_err(|_| "old thread panicked")?
            .error
            .is_none()
    );

    // Cancellation between evaluation and return/queue submission must be observed.
    for stage in [2, 4] {
        let (engine, _, version) = fixture(if stage == 4 { SYNC_WORK } else { 2 })?;
        let region_ref = region(&engine);
        let engine = Arc::new(engine);
        if stage == 2 {
            let prime = call(
                &engine,
                "browse.open",
                json!({"regionRef":region_ref,"viewKind":"list","requestSeq":1,"resultVersion":version}),
                Channel::Gui,
            );
            assert!(prime.error.is_none());
        }
        PAUSE_STAGE.store(stage, std::sync::atomic::Ordering::Release);
        let correlation_id = Ids.next_id();
        let caller = CallerCtx::new(Channel::Gui, correlation_id.clone(), None);
        let other = caller.clone();
        let engine_clone = engine.clone();
        let prior_version = version.clone();
        let prior_region = region_ref.clone();
        let pending = std::thread::spawn(move || {
            engine_clone.call(CallEnvelope {
            name: "browse.open".into(), contract_version: ENVELOPE_VERSION, correlation_id,
            body: json!({"regionRef":prior_region,"viewKind":"list","requestSeq":90001,"resultVersion":prior_version}),
        }, &other)
        });
        gate.entered()?;
        caller.cancel();
        gate.release()?;
        assert!(
            pending
                .join()
                .map_err(|_| "cancelled thread panicked")?
                .error
                .is_some(),
            "stage {stage}"
        );
        let retry = call(
            &engine,
            "browse.open",
            json!({"regionRef":region_ref,"viewKind":"list","requestSeq":90003,"resultVersion":version}),
            Channel::Gui,
        );
        assert!(retry.error.is_none(), "stage {stage}: {:?}", retry.error);
    }
    verify_unadmitted_cancel(gate)?;
    PAUSE_STAGE.store(0, std::sync::atomic::Ordering::Release);
    Ok(())
}

fn verify_unadmitted_cancel(
    gate: &Arc<super::super::test_gate::Gate>,
) -> Result<(), Box<dyn std::error::Error>> {
    // An unadmitted request is not cancellable; region closure fences its preparation.
    let (engine, _, version) = fixture(2)?;
    let region_ref = region(&engine);
    let engine = Arc::new(engine);
    let first = call(
        &engine,
        "browse.open",
        json!({"regionRef":region_ref,"viewKind":"list","requestSeq":1,"resultVersion":version}),
        Channel::Gui,
    );
    assert!(first.error.is_none());
    let query_ref = first.body["page"]["queryRef"].clone();
    PAUSE_STAGE.store(1, std::sync::atomic::Ordering::Release);
    let worker_engine = engine.clone();
    let worker_region = region_ref.clone();
    let worker_version = version.clone();
    let worker = std::thread::spawn(move || {
        call(
            &worker_engine,
            "browse.open",
            json!({"regionRef":worker_region,"viewKind":"list","requestSeq":90001,"resultVersion":worker_version}),
            Channel::Gui,
        )
    });
    gate.entered()?;
    let cancelled = call(
        &engine,
        "query.cancel",
        json!({"regionRef":region_ref,"target":{"kind":"request","requestSeq":90001}}),
        Channel::Gui,
    );
    assert_eq!(
        cancelled
            .error
            .ok_or("unadmitted cancellation accepted")?
            .reason_key,
        "ref.unavailable"
    );
    let closed = call(
        &engine,
        "query.region.close",
        json!({"regionRef":region_ref}),
        Channel::Gui,
    );
    assert!(closed.error.is_none(), "{:?}", closed.error);
    gate.release()?;
    assert!(
        worker
            .join()
            .map_err(|_| "pending thread panicked")?
            .error
            .is_some()
    );
    let old_page = call(
        &engine,
        "query.page",
        json!({"queryRef":query_ref,"resultVersion":version,"start":0,"count":1}),
        Channel::Gui,
    );
    assert!(
        old_page.error.is_none(),
        "existing result was lost: {:?}",
        old_page.error
    );
    Ok(())
}

#[test]
fn rejected_open_preserves_running_job() -> Result<(), Box<dyn std::error::Error>> {
    let _serial = GATE_TEST_LOCK.lock().map_err(|_| "gate lock poisoned")?;
    for bad_sort in [
        json!([{"id":"sort.unknown","revision":1,"descending":false}]),
        json!([{"id":"sort.path","revision":99,"descending":false}]),
        json!([{"id":"sort.path","revision":1,"descending":false}, {"id":"sort.unknown","revision":1,"descending":false}]),
    ] {
        let (engine, runtime, version) = fixture(SYNC_WORK)?;
        let region_ref = region(&engine);
        let gate = super::super::test_gate::Gate::new();
        super::super::test_gate::set(Some((version.clone(), gate.clone())))?;
        let opened = call(
            &engine,
            "browse.open",
            json!({"regionRef":region_ref,"viewKind":"list","requestSeq":1,"resultVersion":version}),
            Channel::Gui,
        );
        let job: Id = serde_json::from_value(opened.body["jobRef"].clone())?;
        gate.entered()?;
        let requests = [
            json!({"regionRef":region_ref,"viewKind":"list","requestSeq":2,"resultVersion":Ids.next_id()}),
            json!({"regionRef":region_ref,"viewKind":"list","requestSeq":3,"resultVersion":version,"location":{"resultVersion":version,"itemNo":99999999}}),
            json!({"regionRef":region_ref,"viewKind":"list","requestSeq":4,"resultVersion":version,"pageSize":0}),
            json!({"regionRef":region_ref,"viewKind":"list","requestSeq":5,"resultVersion":version,"sortKeys":bad_sort}),
        ];
        let rejected: Vec<_> = requests
            .into_iter()
            .map(|body| call(&engine, "browse.open", body, Channel::Gui))
            .collect();
        let state = runtime.state(&job)?;
        super::super::test_gate::set(None)?;
        gate.release()?;
        assert_eq!(
            state,
            JobState::Running,
            "invalid requests cancelled the running job"
        );
        assert!(rejected.iter().all(|response| response.error.is_some()));
        await_query(&runtime, &job)?;
    }
    // Close and window retirement cancel the bound physical worker, then fence publication.
    for retire_session in [false, true] {
        let (engine, runtime, version) = fixture(SYNC_WORK)?;
        let region_ref = region(&engine);
        let gate = super::super::test_gate::Gate::new();
        super::super::test_gate::set(Some((version.clone(), gate.clone())))?;
        let opened = call(
            &engine,
            "browse.open",
            json!({"regionRef":region_ref,"viewKind":"list","requestSeq":1,"resultVersion":version}),
            Channel::Gui,
        );
        let job: Id = serde_json::from_value(opened.body["jobRef"].clone())?;
        gate.entered()?;
        if retire_session {
            engine.retire_window_session();
        } else {
            let closed = call(
                &engine,
                "query.region.close",
                json!({"regionRef":region_ref}),
                Channel::Gui,
            );
            assert!(closed.error.is_none(), "{:?}", closed.error);
        }
        super::super::test_gate::set(None)?;
        gate.release()?;
        let start = Instant::now();
        loop {
            let snapshot = runtime.snapshot(&job)?;
            if snapshot.state.is_terminal() {
                assert_eq!(snapshot.state, JobState::Cancelled);
                assert!(snapshot.result_version_id.is_none());
                break;
            }
            if start.elapsed() > Duration::from_secs(4) {
                return Err("worker cancellation timed out".into());
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    Ok(())
}
