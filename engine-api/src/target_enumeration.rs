//! Single-flight volume enumeration with bounded waiting and explicit shutdown.
//!
//! A timed-out OS call keeps the one permit until the call actually returns.

use std::{
    sync::{Arc, Condvar, Mutex, MutexGuard},
    time::{Duration, Instant},
};

use du_core_contracts::volume::{VolumeEnumeration, VolumePort};

enum State {
    Idle,
    Running {
        abandoned: bool,
    },
    Ready {
        value: Result<Box<VolumeEnumeration>, ()>,
        finished: Instant,
    },
}

struct Shared {
    state: Mutex<Inner>,
    changed: Condvar,
}

struct Inner {
    state: State,
    shutdown: bool,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// Owns the single enumeration permit; it never forcibly interrupts the OS.
pub(crate) struct EnumerationWorker {
    shared: Arc<Shared>,
    volumes: Arc<dyn VolumePort>,
}

/// The request was not started, so no OS call was made for it.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum StartError {
    Busy,
    Shutdown,
    SpawnFailed,
}

/// An admitted observation, whose result is consumed at most once.
pub(crate) struct Pending {
    shared: Arc<Shared>,
    active: bool,
    started: Instant,
}

/// Internal outcome; a ready value retains the port's original error and capabilities.
pub(crate) enum WaitResult {
    Ready(Box<VolumeEnumeration>),
    TimedOut,
    Shutdown,
    Failed,
}

impl EnumerationWorker {
    pub(crate) fn new(volumes: Arc<dyn VolumePort>) -> Self {
        Self {
            shared: Arc::new(Shared {
                state: Mutex::new(Inner {
                    state: State::Idle,
                    shutdown: false,
                }),
                changed: Condvar::new(),
            }),
            volumes,
        }
    }

    /// Starts one call, or rejects it without queuing behind an existing call.
    pub(crate) fn start(&self) -> Result<Pending, StartError> {
        let mut inner = self.shared.lock();
        if inner.shutdown {
            return Err(StartError::Shutdown);
        }
        if !matches!(inner.state, State::Idle) {
            return Err(StartError::Busy);
        }
        inner.state = State::Running { abandoned: false };
        let started = Instant::now();
        let shared = self.shared.clone();
        let volumes = self.volumes.clone();
        let result = std::thread::Builder::new()
            .name("target-enumeration".into())
            .spawn(move || {
                let value =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| volumes.enumerate()));
                let finished = Instant::now();
                let mut inner = shared.lock();
                inner.state = match (&inner.state, inner.shutdown, value) {
                    (State::Running { abandoned: false }, false, value) => State::Ready {
                        value: value.map(Box::new).map_err(|_| ()),
                        finished,
                    },
                    _ => State::Idle,
                };
                shared.changed.notify_all();
            });
        if result.is_err() {
            inner.state = State::Idle;
            return Err(StartError::SpawnFailed);
        }
        Ok(Pending {
            shared: self.shared.clone(),
            active: true,
            started,
        })
    }

    /// Rejects new work immediately and waits at most `limit` for an active call.
    /// False means the native call still holds its permit after the wait.
    pub(crate) fn shutdown(&self, limit: Duration) -> bool {
        let start = Instant::now();
        let mut inner = self.shared.lock();
        inner.shutdown = true;
        if matches!(inner.state, State::Ready { .. }) {
            inner.state = State::Idle;
        }
        self.shared.changed.notify_all();
        while matches!(inner.state, State::Running { .. }) {
            let Some(remaining) = limit.checked_sub(start.elapsed()) else {
                return false;
            };
            if remaining.is_zero() {
                return false;
            }
            let result = self
                .shared
                .changed
                .wait_timeout(inner, remaining)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            inner = result.0;
        }
        true
    }
}

impl Pending {
    /// Applies a limit measured from admission; later observations are discarded.
    pub(crate) fn wait(mut self, limit: Duration) -> WaitResult {
        let start = self.started;
        let mut inner = self.shared.lock();
        loop {
            if inner.shutdown {
                self.active = false;
                return WaitResult::Shutdown;
            }
            match std::mem::replace(&mut inner.state, State::Idle) {
                State::Ready { value, finished } => {
                    self.active = false;
                    return if finished.saturating_duration_since(start) > limit {
                        WaitResult::TimedOut
                    } else if let Ok(value) = value {
                        WaitResult::Ready(value)
                    } else {
                        WaitResult::Failed
                    };
                }
                State::Idle => {
                    self.active = false;
                    return WaitResult::TimedOut;
                }
                running @ State::Running { .. } => inner.state = running,
            }
            let Some(remaining) = limit
                .checked_sub(start.elapsed())
                .filter(|duration| !duration.is_zero())
            else {
                inner.state = State::Running { abandoned: true };
                self.active = false;
                return WaitResult::TimedOut;
            };
            inner = self
                .shared
                .changed
                .wait_timeout(inner, remaining)
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .0;
        }
    }
}

impl Drop for EnumerationWorker {
    fn drop(&mut self) {
        self.shutdown(Duration::ZERO);
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        let mut inner = self.shared.lock();
        match inner.state {
            State::Running { .. } => inner.state = State::Running { abandoned: true },
            State::Ready { .. } => inner.state = State::Idle,
            State::Idle => {}
        }
        self.shared.changed.notify_all();
    }
}

#[cfg(test)]
mod tests;
