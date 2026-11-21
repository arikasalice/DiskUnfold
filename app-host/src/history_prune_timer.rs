//! Wake the history-prune submitter every 24 hours while retention is enabled.

use std::{
    sync::{Arc, Condvar, Mutex, Weak},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use du_engine_api::history::retention::HistoryPruneTrigger;

const INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Default)]
struct TimerState {
    enabled: bool,
    stopped: bool,
}

/// Owns the app-host retention timer and stops it when the GUI lifecycle ends.
pub(super) struct HistoryPruneTimer {
    state: Arc<(Mutex<TimerState>, Condvar)>,
    worker: Option<JoinHandle<()>>,
}

impl HistoryPruneTimer {
    pub(super) fn new(enabled: bool) -> Self {
        Self {
            state: Arc::new((
                Mutex::new(TimerState {
                    enabled,
                    stopped: false,
                }),
                Condvar::new(),
            )),
            worker: None,
        }
    }

    /// Lets the retention-policy saver wake or cancel the timer without owning it.
    pub(super) fn policy_changed(&self) -> Arc<dyn Fn(bool) + Send + Sync> {
        let state: Weak<(Mutex<TimerState>, Condvar)> = Arc::downgrade(&self.state);
        Arc::new(move |enabled| {
            if let Some(state) = state.upgrade() {
                let (lock, wake) = &*state;
                if let Ok(mut current) = lock.lock() {
                    current.enabled = enabled;
                    wake.notify_all();
                }
            }
        })
    }

    pub(super) fn start(&mut self, trigger: HistoryPruneTrigger) -> std::io::Result<()> {
        self.start_with_interval(trigger, INTERVAL)
    }

    fn start_with_interval(
        &mut self,
        trigger: HistoryPruneTrigger,
        interval: Duration,
    ) -> std::io::Result<()> {
        let state = self.state.clone();
        self.worker = Some(
            thread::Builder::new()
                .name("history-prune-timer".into())
                .spawn(move || wait_and_submit(state, trigger, interval))?,
        );
        Ok(())
    }
}

impl Drop for HistoryPruneTimer {
    fn drop(&mut self) {
        let (lock, wake) = &*self.state;
        if let Ok(mut state) = lock.lock() {
            state.stopped = true;
            wake.notify_all();
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn wait_and_submit(
    state: Arc<(Mutex<TimerState>, Condvar)>,
    trigger: HistoryPruneTrigger,
    interval: Duration,
) {
    let (lock, wake) = &*state;
    loop {
        let Ok(mut current) = lock.lock() else {
            return;
        };
        while !current.stopped && !current.enabled {
            current = match wake.wait(current) {
                Ok(state) => state,
                Err(_) => return,
            };
        }
        if current.stopped {
            return;
        }
        let deadline = Instant::now() + interval;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                drop(current);
                let _ = trigger();
                break;
            }
            let (next, timeout) = match wake.wait_timeout(current, remaining) {
                Ok(result) => result,
                Err(_) => return,
            };
            current = next;
            if current.stopped {
                return;
            }
            if !current.enabled {
                break;
            }
            if timeout.timed_out() {
                drop(current);
                let _ = trigger();
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use du_core_contracts::Id;
    use std::sync::mpsc;

    #[test]
    fn disabling_cancels_the_wait_and_reenabling_starts_a_new_interval()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut timer = HistoryPruneTimer::new(true);
        let (sender, receiver) = mpsc::channel::<Option<Id>>();
        let trigger: HistoryPruneTrigger = Arc::new(move || {
            let _ = sender.send(None);
            None
        });
        let update = timer.policy_changed();
        timer.start_with_interval(trigger, Duration::from_millis(80))?;
        update(false);
        assert!(receiver.recv_timeout(Duration::from_millis(100)).is_err());
        update(true);
        receiver.recv_timeout(Duration::from_secs(1))?;
        Ok(())
    }
}
