//! Bounded worker synchronization for folder command tests.
use du_core_contracts::Id;
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::time::Duration;

const WAIT: Duration = Duration::from_secs(10);
pub(super) struct Gate {
    entered_tx: mpsc::Sender<()>,
    entered_rx: Mutex<mpsc::Receiver<()>>,
    release_tx: mpsc::Sender<()>,
    release_rx: Mutex<mpsc::Receiver<()>>,
}
impl Gate {
    pub(super) fn new() -> Arc<Self> {
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        Arc::new(Self {
            entered_tx,
            entered_rx: Mutex::new(entered_rx),
            release_tx,
            release_rx: Mutex::new(release_rx),
        })
    }
    pub(super) fn pause(&self) -> Result<(), &'static str> {
        self.entered_tx
            .send(())
            .map_err(|_| "test gate entry receiver closed")?;
        self.release_rx
            .lock()
            .map_err(|_| "test gate release poisoned")?
            .recv_timeout(WAIT)
            .map_err(|_| "test gate release timed out")
    }
    pub(super) fn entered(&self) -> Result<(), &'static str> {
        self.entered_rx
            .lock()
            .map_err(|_| "test gate entry poisoned")?
            .recv_timeout(WAIT)
            .map_err(|_| "test gate entry timed out")
    }
    pub(super) fn release(&self) -> Result<(), &'static str> {
        self.release_tx
            .send(())
            .map_err(|_| "test gate release receiver closed")
    }
}
type WorkerGate = (Id, Arc<Gate>);
static GATE: OnceLock<Mutex<Option<WorkerGate>>> = OnceLock::new();
pub(super) fn set(gate: Option<WorkerGate>) -> Result<(), &'static str> {
    *GATE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map_err(|_| "gate lock")? = gate;
    Ok(())
}
pub(super) fn pause_worker(version: &Id) -> Result<(), &'static str> {
    let gate = GATE.get().and_then(|gate| {
        gate.lock().ok().and_then(|entry| {
            entry
                .as_ref()
                .filter(|(id, _)| id == version)
                .map(|(_, gate)| gate.clone())
        })
    });
    if let Some(gate) = gate {
        gate.pause()?;
    }
    Ok(())
}
