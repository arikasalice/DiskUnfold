use super::*;
use crate::{api::Engine, desktop::DesktopEvents};
use du_core_contracts::{
    CallEnvelope, ENVELOPE_VERSION, Id, ResultEnvelope,
    job::{HistoryAppend, JobHistory, JobState, JobSummary},
    ports::PortError,
    result::ResultCompleteness,
};
use du_job_runtime::{JobClock, JobRuntime};
use serde_json::{Value, json};

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
    let runtime = Arc::new(JobRuntime::new(
        Arc::new(Ids),
        Arc::new(Clock),
        Arc::new(History),
        Arc::new(DesktopEvents::default()),
    )?);
    let workers = Arc::new(Workers::start(runtime.clone())?);
    let mut builder = EngineBuilder::default();
    register(&mut builder, store, Arc::new(Ids), workers, &Entropy)?;
    Ok((builder.build()?, runtime, version))
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

#[test]
fn registered_gui_commands_open_and_continue_with_the_same_registry()
-> Result<(), Box<dyn std::error::Error>> {
    let (engine, _, version) = fixture(2)?;
    let request = json!({"viewKind":"list","requestSeq":4,"resultVersion":version,"pageSize":1});
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
    Ok(())
}

#[test]
fn large_folder_returns_a_job_whose_reference_reads_the_original_result()
-> Result<(), Box<dyn std::error::Error>> {
    let (engine, runtime, version) = fixture(SYNC_WORK)?;
    let open = call(
        &engine,
        "browse.open",
        json!({"viewKind":"list","requestSeq":8,"resultVersion":version}),
        Channel::Gui,
    );
    assert!(open.error.is_none(), "{:?}", open.error);
    assert_eq!(open.body["kind"], "job");
    assert_eq!(open.body["requestSeq"], 8);
    let job: Id = serde_json::from_value(open.body["jobRef"].clone())?;
    let query_ref = await_query(&runtime, &job)?;
    let first = call(
        &engine,
        "query.page",
        json!({"queryRef":query_ref,"resultVersion":version,"start":0,"count":200}),
        Channel::Gui,
    );
    assert!(first.error.is_none(), "{:?}", first.error);
    assert_eq!(first.body["resultVersion"], json!(version));
    assert_eq!(
        first.body["rows"].as_array().ok_or("missing rows")?.len(),
        200
    );
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

static PREPARE_GATE: std::sync::OnceLock<(std::sync::Barrier, std::sync::Barrier)> =
    std::sync::OnceLock::new();
static PAUSE_STAGE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
pub(super) fn pause_at(sequence: u32, stage: usize) {
    if sequence == 90001
        && PAUSE_STAGE.load(std::sync::atomic::Ordering::Acquire) == stage
        && let Some((entered, release)) = PREPARE_GATE.get()
    {
        entered.wait();
        release.wait();
    }
}

#[test]
fn late_preparation_cannot_supersede_newer_open() -> Result<(), Box<dyn std::error::Error>> {
    let (entered, release) =
        PREPARE_GATE.get_or_init(|| (std::sync::Barrier::new(2), std::sync::Barrier::new(2)));
    for stage in 1..=4 {
        let (engine, runtime, version) = fixture(if stage == 4 { SYNC_WORK } else { 2 })?;
        let engine = Arc::new(engine);
        if stage == 2 {
            let prime = call(
                &engine,
                "browse.open",
                json!({"viewKind":"list","requestSeq":1,"resultVersion":version}),
                Channel::Gui,
            );
            assert!(prime.error.is_none(), "cache setup: {:?}", prime.error);
        }
        PAUSE_STAGE.store(stage, std::sync::atomic::Ordering::Release);
        let prior_engine = engine.clone();
        let prior_version = version.clone();
        let prior = std::thread::spawn(move || {
            call(
                &prior_engine,
                "browse.open",
                json!({"viewKind":"list","requestSeq":90001,"resultVersion":prior_version}),
                Channel::Gui,
            )
        });
        entered.wait();
        let newer = call(
            &engine,
            "browse.open",
            json!({"viewKind":"list","requestSeq":2,"resultVersion":version}),
            Channel::Gui,
        );
        release.wait();
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
            json!({"viewKind":"list","requestSeq":3,"resultVersion":version}),
            Channel::Gui,
        );
        assert!(
            retry.error.is_none(),
            "stage {stage}: retry failed: {:?}",
            retry.error
        );
    }
    PAUSE_STAGE.store(0, std::sync::atomic::Ordering::Release);
    Ok(())
}
