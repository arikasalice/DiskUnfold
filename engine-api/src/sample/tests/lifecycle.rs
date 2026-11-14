//! Session lifecycle edges: an unreadable clock, reload keys, window retirement, a close that
//! races the load, and commands from several threads.

use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};

use du_analysis_index::{IngestSink, SampleSessionKey};
use du_core_contracts::{
    Id, Timestamp,
    job::{Checkpoint, JobBody, JobContext, JobFlow, JobFlowError, JobOutcome, JobState},
    sample_ipc::{
        SampleEventState as E, SampleObservationRef, SampleSession, SampleSessionState as S,
        SampleStateChanged,
    },
};
use serde_json::json;

use super::{
    Rig,
    embedded::{embedded, embedded_with},
    engine::{call, engine},
    job_id, ok,
    real::{Ids, wait_for},
};
use crate::{
    api::Channel,
    sample::{EmbeddedSampleLoader, SampleBundleInfo, SampleLoadReport, SampleLoader},
};

const EPOCH: &str = "1970-01-01T00:00:00.000Z";

fn broken_clock() -> Option<Timestamp> {
    None
}

#[test]
fn an_unreadable_clock_still_publishes_each_transition_at_the_epoch() -> Result<(), String> {
    let rig = Rig::with_clock(broken_clock);
    rig.seed_observations(1);
    let opened = ok(rig.open())?;
    rig.finish(&opened.job_ref.clone().ok_or("no job")?, JobState::Done);
    ok(rig.close(&opened.session_ref))?;
    let events = rig.events();
    let sequences: Vec<u64> = events.iter().map(|event| event.sequence).collect();
    assert_eq!(sequences, [1, 2, 3]);
    assert!(
        events
            .iter()
            .all(|event| event.observed_at.as_str() == EPOCH)
    );
    Ok(())
}

static CLOCK_READABLE: AtomicBool = AtomicBool::new(true);

fn flaky_clock() -> Option<Timestamp> {
    if CLOCK_READABLE.load(Ordering::SeqCst) {
        Timestamp::from_unix_millis(5_000)
    } else {
        None
    }
}

#[test]
fn an_event_without_a_clock_reuses_the_previous_time_of_its_session() -> Result<(), String> {
    let rig = Rig::with_clock(flaky_clock);
    rig.seed_observations(1);
    let opened = ok(rig.open())?;
    CLOCK_READABLE.store(false, Ordering::SeqCst);
    rig.finish(&opened.job_ref.clone().ok_or("no job")?, JobState::Done);
    ok(rig.close(&opened.session_ref))?;
    CLOCK_READABLE.store(true, Ordering::SeqCst);
    let events = rig.events();
    assert_eq!(events.len(), 3);
    assert_eq!(events[0].observed_at.as_str(), "1970-01-01T00:00:05.000Z");
    assert!(
        events
            .iter()
            .all(|event| event.observed_at == events[0].observed_at)
    );
    Ok(())
}

#[test]
fn a_reset_reloads_under_a_new_session_key() -> Result<(), String> {
    let rig = Rig::new();
    rig.seed_observations(1);
    let opened = ok(rig.open())?;
    rig.finish(&opened.job_ref.clone().ok_or("no job")?, JobState::Done);
    let reset = ok(rig.reset(&opened.session_ref))?;
    assert_eq!(reset.state, S::Preparing);
    let keys = rig.loader.keys.lock().map_err(|e| e.to_string())?.clone();
    let released = rig
        .loader
        .released
        .lock()
        .map_err(|e| e.to_string())?
        .clone();
    // The store closes a released key for good, so the second load must not reuse the first.
    assert_eq!(keys.len(), 2);
    assert_ne!(keys[0], keys[1]);
    assert_eq!(released, [keys[0].clone()]);
    // Closing releases the key of the current load.
    ok(rig.close(&opened.session_ref))?;
    let released = rig
        .loader
        .released
        .lock()
        .map_err(|e| e.to_string())?
        .clone();
    assert_eq!(released, [keys[0].clone(), keys[1].clone()]);
    Ok(())
}

#[test]
fn retiring_the_window_session_closes_the_open_sample_session() -> Result<(), String> {
    let rig = Rig::new();
    let engine = engine(&rig)?;
    // Nothing is open yet, so retiring changes nothing.
    engine.retire_window_session();
    assert!(rig.events().is_empty() && rig.releases() == 0);

    let opened = call(&engine, "sample.open", Channel::Gui, json!({}));
    let view: SampleSession = serde_json::from_value(opened.body).map_err(|e| e.to_string())?;
    let job = view.job_ref.clone().ok_or("no job")?;
    engine.retire_window_session();
    assert_eq!(rig.cancelled(), [job]);
    assert_eq!(rig.releases(), 1);
    let events = rig.events();
    assert_eq!(events.len(), 2);
    assert_eq!(events[1].state, E::Inactive);
    let status = call(
        &engine,
        "sample.status",
        Channel::Gui,
        json!({"sessionRef": view.session_ref.as_str()}),
    );
    assert_eq!(
        status.error.ok_or("expected closed")?.reason_key,
        "sample.session_closed"
    );
    // A second retirement finds nothing left to close.
    engine.retire_window_session();
    assert_eq!((rig.events().len(), rig.releases()), (2, 1));
    Ok(())
}

#[test]
fn retiring_the_window_session_releases_loaded_sample_versions() -> Result<(), String> {
    let rig = embedded()?;
    let opened = call(&rig.engine, "sample.open", Channel::Gui, json!({}));
    let view: SampleSession = serde_json::from_value(opened.body).map_err(|e| e.to_string())?;
    let reference = json!({"sessionRef": view.session_ref.as_str()});
    let loaded = || -> Result<SampleSession, String> {
        let status = call(
            &rig.engine,
            "sample.status",
            Channel::Gui,
            reference.clone(),
        );
        serde_json::from_value(status.body).map_err(|e| e.to_string())
    };
    wait_for(|| loaded().is_ok_and(|view| view.state == S::Active))?;
    let versions: Vec<Id> = loaded()?
        .observations
        .iter()
        .map(|observation| observation.result_version.clone())
        .collect();
    assert_eq!(versions.len(), 2);
    for version in &versions {
        rig.store
            .pin_current(version)
            .map_err(|e| format!("{e:?}"))?;
    }
    rig.engine.retire_window_session();
    for version in &versions {
        assert!(rig.store.pin_current(version).is_err());
    }
    Ok(())
}

/// Lets the real load body run while ignoring cancellation, which models a close that
/// arrives after the body's last checkpoint but before it registers the next lineage.
struct Masked(Arc<dyn JobContext>);

impl JobContext for Masked {
    fn checkpoint(&self) -> Checkpoint {
        Checkpoint::Continue
    }

    fn abort_flows(&self) {
        self.0.abort_flows();
    }

    fn utc_now(&self) -> Option<Timestamp> {
        self.0.utc_now()
    }

    fn monotonic_now(&self) -> Option<Duration> {
        self.0.monotonic_now()
    }

    fn try_start_flow(&self) -> Result<Box<dyn JobFlow>, JobFlowError> {
        self.0.try_start_flow()
    }
}

struct GatedBody {
    inner: Arc<dyn JobBody>,
    entered: mpsc::Sender<()>,
    gate: Arc<Mutex<mpsc::Receiver<()>>>,
}

impl JobBody for GatedBody {
    fn kind_id(&self) -> &str {
        self.inner.kind_id()
    }

    fn run(&self, context: Arc<dyn JobContext>) -> JobOutcome {
        let _ = self.entered.send(());
        if let Ok(gate) = self.gate.lock() {
            let _ = gate.recv_timeout(Duration::from_secs(10));
        }
        self.inner.run(Arc::new(Masked(context)))
    }
}

/// The embedded loader, with each load held at its start until the test lets it go.
struct GatedLoader {
    inner: EmbeddedSampleLoader,
    entered: mpsc::Sender<()>,
    gate: Arc<Mutex<mpsc::Receiver<()>>>,
}

impl SampleLoader for GatedLoader {
    fn open(&self) -> Result<SampleBundleInfo, &'static str> {
        self.inner.open()
    }

    fn job(&self, session: &SampleSessionKey) -> (Arc<dyn JobBody>, Arc<dyn SampleLoadReport>) {
        let (inner, report) = self.inner.job(session);
        let body = GatedBody {
            inner,
            entered: self.entered.clone(),
            gate: self.gate.clone(),
        };
        (Arc::new(body), report)
    }

    fn release(&self, session: &SampleSessionKey) -> u64 {
        self.inner.release(session)
    }
}

#[test]
fn a_close_before_the_next_observation_registers_leaves_no_version() -> Result<(), String> {
    let (entered, started) = mpsc::channel();
    let (open_gate, gate) = mpsc::channel();
    let gate = Arc::new(Mutex::new(gate));
    let rig = embedded_with(|store: IngestSink, ids: Arc<Ids>| {
        Arc::new(GatedLoader {
            inner: EmbeddedSampleLoader::new(store, ids),
            entered,
            gate,
        })
    })?;
    let opened = call(&rig.engine, "sample.open", Channel::Gui, json!({}));
    let view: SampleSession = serde_json::from_value(opened.body).map_err(|e| e.to_string())?;
    let job = view.job_ref.clone().ok_or("no job")?;
    started
        .recv_timeout(Duration::from_secs(10))
        .map_err(|_| "the load did not start")?;
    let closed = call(
        &rig.engine,
        "sample.close",
        Channel::Gui,
        json!({"sessionRef": view.session_ref.as_str()}),
    );
    assert_eq!(closed.body["releasedVersions"], 0);
    // The body now registers its first lineage although its session is already closed.
    open_gate.send(()).map_err(|e| e.to_string())?;
    wait_for(|| {
        rig.runtime
            .state(&job)
            .is_ok_and(|state| state.is_terminal())
    })?;
    assert_no_version_left(&rig.store, &rig.ids)?;
    let events = rig.sink.0.lock().map_err(|e| e.to_string())?;
    let last = events.last().ok_or("no events")?;
    assert_eq!(last.0.state, E::Inactive);
    assert_eq!(events.len(), 2);
    Ok(())
}

/// Every identifier the run issued, version identifiers included, must be gone from the store.
fn assert_no_version_left(store: &IngestSink, ids: &Ids) -> Result<(), String> {
    let issued = ids.0.load(Ordering::Relaxed) as u64;
    for n in 0..issued {
        let id = job_id(10_000 + n);
        assert!(store.pin_current(&id).is_err(), "version {n} survived");
    }
    Ok(())
}

const THREADS: u64 = 4;
const ROUNDS: u64 = 40;

/// Sends one command and accepts only the answers a racing caller can legitimately get.
fn step(
    engine: &crate::api::Engine,
    state: &mut u64,
    session: &mut Option<Id>,
) -> Result<(), String> {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    let reference = session.clone().unwrap_or_else(|| job_id(1));
    let by_reference = json!({"sessionRef": reference.as_str()});
    let (name, body) = match *state % 4 {
        0 => ("sample.open", json!({})),
        1 => ("sample.status", by_reference),
        2 => ("sample.reset", by_reference),
        _ => ("sample.close", by_reference),
    };
    let response = call(engine, name, Channel::Gui, body);
    if let Some(error) = response.error {
        let allowed = ["sample.session_closed", "job.queue_full"];
        if !allowed.contains(&error.reason_key.as_str()) {
            return Err(format!("{name} failed with {}", error.reason_key));
        }
        return Ok(());
    }
    if name == "sample.open" {
        let view: SampleSession =
            serde_json::from_value(response.body).map_err(|e| e.to_string())?;
        *session = Some(view.session_ref);
    }
    Ok(())
}

fn per_session(
    events: &[(SampleStateChanged, Option<Id>)],
) -> HashMap<Id, Vec<&SampleStateChanged>> {
    let mut sessions: HashMap<Id, Vec<&SampleStateChanged>> = HashMap::new();
    for (event, _) in events {
        sessions
            .entry(event.session_ref.clone())
            .or_default()
            .push(event);
    }
    sessions
}

#[test]
fn commands_from_several_threads_end_consistently_with_no_version_left() -> Result<(), String> {
    let rig = embedded()?;
    // One loaded session exists before the race, so a close inside it has real versions to drop.
    let first = call(&rig.engine, "sample.open", Channel::Gui, json!({}));
    let first: SampleSession = serde_json::from_value(first.body).map_err(|e| e.to_string())?;
    let reference = json!({"sessionRef": first.session_ref.as_str()});
    let loaded = || -> Result<SampleSession, String> {
        let status = call(
            &rig.engine,
            "sample.status",
            Channel::Gui,
            reference.clone(),
        );
        serde_json::from_value(status.body).map_err(|e| e.to_string())
    };
    wait_for(|| loaded().is_ok_and(|view| view.state == S::Active))?;
    let loaded_versions: Vec<SampleObservationRef> = loaded()?.observations;
    assert_eq!(loaded_versions.len(), 2);

    let engine = Arc::new(rig.engine);
    let (done, finished) = mpsc::channel();
    for thread in 0..THREADS {
        let (engine, done) = (engine.clone(), done.clone());
        std::thread::spawn(move || {
            let mut state = 0x9E37_79B9_7F4A_7C15 ^ (thread + 1);
            let mut session = None;
            let result = (0..ROUNDS).try_for_each(|_| step(&engine, &mut state, &mut session));
            let _ = done.send(result);
        });
    }
    for _ in 0..THREADS {
        finished
            .recv_timeout(Duration::from_secs(60))
            .map_err(|_| "a command thread did not finish; the session may be deadlocked")??;
    }

    engine.retire_window_session();
    wait_for(|| {
        rig.runtime
            .list(None)
            .is_ok_and(|rows| rows.iter().all(|row| row.snapshot.state.is_terminal()))
    })?;

    let events = rig.sink.0.lock().map_err(|e| e.to_string())?;
    let sessions = per_session(&events);
    assert!(sessions.contains_key(&first.session_ref));
    for (session, list) in &sessions {
        // Every transition adds exactly 1 to the sequence and chains to the previous state.
        for (index, event) in list.iter().enumerate() {
            assert_eq!(event.sequence, index as u64 + 1, "session {session:?}");
            let expected = match index {
                0 => E::Inactive,
                _ => list[index - 1].state,
            };
            assert_eq!(event.previous_state, expected, "session {session:?}");
        }
        assert_eq!(list.last().map(|event| event.state), Some(E::Inactive));
    }
    drop(events);
    for observation in &loaded_versions {
        assert!(rig.store.pin_current(&observation.result_version).is_err());
    }
    assert_no_version_left(&rig.store, &rig.ids)
}
