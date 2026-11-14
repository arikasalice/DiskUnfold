//! A gate that blocks hooked calls until released.
use super::*;

/// Blocks hooked calls until released and counts how many wait at once.
#[derive(Default)]
pub(super) struct Gate {
    state: Mutex<GateState>,
    changed: Condvar,
}

#[derive(Default)]
pub(super) struct GateState {
    pub(super) released: bool,
    pub(super) waiting: usize,
    pub(super) most: usize,
    pub(super) entered: usize,
}

impl Gate {
    pub(super) fn wait(&self) -> Result<(), PortError> {
        let mut state = self.state.lock().map_err(|_| io_error("test.gate"))?;
        state.entered += 1;
        state.waiting += 1;
        state.most = state.most.max(state.waiting);
        self.changed.notify_all();
        while !state.released {
            state = self
                .changed
                .wait(state)
                .map_err(|_| io_error("test.gate"))?;
        }
        state.waiting -= 1;
        Ok(())
    }

    pub(super) fn release(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.released = true;
        }
        self.changed.notify_all();
    }

    /// Waits until `ready` holds for the gate state.
    pub(super) fn until(&self, ready: impl Fn(&GateState) -> bool) -> Result<(), TestError> {
        let state = self.state.lock().map_err(|_| "gate lock")?;
        let (state, _) = self
            .changed
            .wait_timeout_while(state, Duration::from_secs(10), |state| !ready(state))
            .map_err(|_| "gate lock")?;
        if ready(&state) {
            Ok(())
        } else {
            Err("gate condition was not reached".into())
        }
    }

    pub(super) fn most(&self) -> usize {
        self.state.lock().map_or(usize::MAX, |state| state.most)
    }
}

/// Releases the gate when a test returns early, so no worker stays blocked.
pub(super) struct Opened(pub(super) Arc<Gate>);

impl Drop for Opened {
    fn drop(&mut self) {
        self.0.release();
    }
}
