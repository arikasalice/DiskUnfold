//! Bounded worker synchronization for asynchronous search tests.
use du_core_contracts::Id;
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::time::Duration;

const WAIT: Duration = Duration::from_secs(10);
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Stage {
    Admission,
    Worker,
    Candidate,
    Publish,
    CancelPreflight,
    RequestCancel,
}
pub(super) fn pause_admission(version: &Id) -> Result<(), &'static str> {
    pause(version, Stage::Admission)
}
pub(crate) struct Gate {
    entered_tx: mpsc::Sender<()>,
    entered_rx: Mutex<mpsc::Receiver<()>>,
    release_tx: mpsc::Sender<()>,
    release_rx: Mutex<mpsc::Receiver<()>>,
    stage: Stage,
}
impl Gate {
    pub(super) fn new(stage: Stage) -> Arc<Self> {
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        Arc::new(Self {
            entered_tx,
            entered_rx: Mutex::new(entered_rx),
            release_tx,
            release_rx: Mutex::new(release_rx),
            stage,
        })
    }
    fn pause(&self) -> Result<(), &'static str> {
        self.entered_tx.send(()).map_err(|_| "entry closed")?;
        self.release_rx
            .lock()
            .map_err(|_| "release lock")?
            .recv_timeout(WAIT)
            .map_err(|_| "release timeout")
    }
    pub(super) fn entered(&self) -> Result<(), &'static str> {
        self.entered_rx
            .lock()
            .map_err(|_| "entry lock")?
            .recv_timeout(WAIT)
            .map_err(|_| "entry timeout")
    }
    pub(super) fn release(&self) -> Result<(), &'static str> {
        self.release_tx.send(()).map_err(|_| "release closed")
    }
}
type GateState = Mutex<Option<(Id, Arc<Gate>)>>;
static GATE: OnceLock<GateState> = OnceLock::new();
static CANCEL_GATE: OnceLock<GateState> = OnceLock::new();
static TEST_LOCK: Mutex<()> = Mutex::new(());
pub(super) fn serial() -> Result<std::sync::MutexGuard<'static, ()>, &'static str> {
    TEST_LOCK.lock().map_err(|_| "test lock poisoned")
}
pub(super) fn set(gate: Option<(Id, Arc<Gate>)>) -> Result<(), &'static str> {
    *GATE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map_err(|_| "gate lock")? = gate;
    Ok(())
}
pub(crate) fn set_cancel_gate(gate: Option<(Id, Arc<Gate>)>) -> Result<(), &'static str> {
    *CANCEL_GATE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map_err(|_| "cancel gate lock")? = gate;
    Ok(())
}
pub(super) fn pause_worker(version: &Id) -> Result<(), &'static str> {
    pause(version, Stage::Worker)
}
pub(super) fn pause_publish(version: &Id) -> Result<(), &'static str> {
    pause(version, Stage::Publish)
}
pub(super) fn pause_candidate(version: &Id) -> Result<(), &'static str> {
    pause(version, Stage::Candidate)
}
fn pause(version: &Id, stage: Stage) -> Result<(), &'static str> {
    let gate = GATE.get().and_then(|gate| {
        gate.lock().ok().and_then(|value| {
            value
                .as_ref()
                .filter(|(expected, gate)| expected == version && gate.stage == stage)
                .map(|(_, gate)| gate.clone())
        })
    });
    if let Some(gate) = gate {
        gate.pause()?;
    }
    Ok(())
}
fn pause_cancel(version: &Id, stage: Stage) -> Result<(), &'static str> {
    let gate = CANCEL_GATE.get().and_then(|gate| {
        gate.lock().ok().and_then(|value| {
            value
                .as_ref()
                .filter(|(expected, gate)| expected == version && gate.stage == stage)
                .map(|(_, gate)| gate.clone())
        })
    });
    if let Some(gate) = gate {
        gate.pause()?;
    }
    Ok(())
}
pub(crate) fn pause_cancel_preflight(job: &Id) -> Result<(), &'static str> {
    pause_cancel(job, Stage::CancelPreflight)
}
pub(crate) fn pause_request_cancel(region: &Id) -> Result<(), &'static str> {
    pause_cancel(region, Stage::RequestCancel)
}
