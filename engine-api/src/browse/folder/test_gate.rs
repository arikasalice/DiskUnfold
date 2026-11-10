//! Deterministic worker synchronization for folder command tests.
use du_core_contracts::Id;
use std::sync::{Arc, Barrier, Mutex, OnceLock};
type WorkerGate = (Id, Arc<Barrier>, Arc<Barrier>);
static GATE: OnceLock<Mutex<Option<WorkerGate>>> = OnceLock::new();
pub(super) fn set(gate: Option<WorkerGate>) -> Result<(), &'static str> {
    *GATE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map_err(|_| "gate lock")? = gate;
    Ok(())
}
pub(super) fn pause_worker(version: &Id) {
    let gates = GATE.get().and_then(|gate| {
        gate.lock()
            .ok()
            .and_then(|entry| entry.as_ref().filter(|(id, _, _)| id == version).cloned())
    });
    if let Some((_, entered, release)) = gates {
        entered.wait();
        release.wait();
    }
}
