//! A job context whose flows park on pause and stop on cancellation.
use super::*;

#[derive(Default)]
pub(super) struct Switches {
    pub(super) paused: bool,
    pub(super) cancelled: bool,
    pub(super) active: usize,
    pub(super) parked: usize,
}

/// Pause and cancel switches with the flows they park.
#[derive(Default)]
pub(super) struct Remote {
    state: Mutex<Switches>,
    changed: Condvar,
}

impl Remote {
    pub(super) fn set(&self, change: impl FnOnce(&mut Switches)) -> Result<(), TestError> {
        change(&mut *self.state.lock().map_err(|_| "remote lock")?);
        self.changed.notify_all();
        Ok(())
    }

    pub(super) fn until(&self, ready: impl Fn(&Switches) -> bool) -> Result<(), TestError> {
        let state = self.state.lock().map_err(|_| "remote lock")?;
        let (state, _) = self
            .changed
            .wait_timeout_while(state, Duration::from_secs(10), |state| !ready(state))
            .map_err(|_| "remote lock")?;
        if ready(&state) {
            Ok(())
        } else {
            Err("flows did not reach the expected state".into())
        }
    }
}

/// Cancels and unparks every flow when a test returns early.
pub(super) struct Unpark(pub(super) Arc<Remote>);

impl Drop for Unpark {
    fn drop(&mut self) {
        let _ = self.0.set(|state| state.cancelled = true);
    }
}

pub(super) struct RemoteContext(pub(super) Arc<Remote>);
struct RemoteFlow(Arc<Remote>);

impl JobContext for RemoteContext {
    fn checkpoint(&self) -> Checkpoint {
        if self.0.state.lock().map_or(true, |state| state.cancelled) {
            Checkpoint::Cancel
        } else {
            Checkpoint::Continue
        }
    }

    fn abort_flows(&self) {
        let _ = self.0.set(|state| state.cancelled = true);
    }

    fn try_start_flow(&self) -> Result<Box<dyn JobFlow>, JobFlowError> {
        let mut state = self.0.state.lock().map_err(|_| JobFlowError::Closed)?;
        if state.cancelled {
            return Err(JobFlowError::Cancelled);
        }
        state.active += 1;
        Ok(Box::new(RemoteFlow(self.0.clone())))
    }
}

impl JobFlow for RemoteFlow {
    fn safe_point(&self) -> Checkpoint {
        let Ok(mut state) = self.0.state.lock() else {
            return Checkpoint::Cancel;
        };
        if state.paused && !state.cancelled {
            state.parked += 1;
            self.0.changed.notify_all();
            while state.paused && !state.cancelled {
                state = match self.0.changed.wait(state) {
                    Ok(state) => state,
                    Err(_) => return Checkpoint::Cancel,
                };
            }
            state.parked -= 1;
        }
        if state.cancelled {
            Checkpoint::Cancel
        } else {
            Checkpoint::Continue
        }
    }

    fn report_item(&self, _: &[u8]) -> Result<(), JobFlowError> {
        Ok(())
    }

    fn finish(self: Box<Self>, _: JobCounts) {}
}

impl Drop for RemoteFlow {
    fn drop(&mut self) {
        let _ = self.0.set(|state| state.active -= 1);
    }
}
