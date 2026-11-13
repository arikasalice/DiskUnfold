//! Thread-safe front of the shutdown coordinator for IPC and the native loop.

use std::sync::{
    Mutex, MutexGuard,
    atomic::{AtomicBool, Ordering},
};

use du_core_contracts::shutdown_ipc::{
    AppQuitMode, AppQuitOutcome, AppQuitRequest, AppQuitResponse,
};

use super::{
    QuitAdmission, QuitMode, ShutdownCoordinator, ShutdownError, ShutdownOutcome, ShutdownPorts,
    ShutdownTimeouts, current_time_ms,
};

/// IPC surface shared by the dispatcher and the native close-request handlers.
pub(crate) trait QuitHandler: Send + Sync {
    fn request_quit(&self, request: &AppQuitRequest) -> Result<AppQuitResponse, ShutdownError>;
    fn is_closing(&self) -> bool;
}

/// Serializes quit admission, coordinator progress, and shutdown event publication.
///
/// A poisoned coordinator lock means a panic left the state machine in an unknown
/// place. The controller then reports that the loop should exit and that the
/// exit failed, rather than driving storage steps from unverified state.
pub(crate) struct ShutdownController<P> {
    coordinator: Mutex<ShutdownCoordinator<P>>,
    poisoned: AtomicBool,
}

impl<P: ShutdownPorts> ShutdownController<P> {
    pub(crate) fn new(ports: P) -> Self {
        Self::with_timeouts(ports, ShutdownTimeouts::default())
    }

    pub(crate) fn with_timeouts(ports: P, timeouts: ShutdownTimeouts) -> Self {
        Self {
            coordinator: Mutex::new(ShutdownCoordinator::with_timeouts(ports, timeouts)),
            poisoned: AtomicBool::new(false),
        }
    }

    fn lock(&self) -> Result<MutexGuard<'_, ShutdownCoordinator<P>>, ShutdownError> {
        self.coordinator.lock().map_err(|_| {
            if !self.poisoned.swap(true, Ordering::AcqRel) {
                eprintln!("DiskUnfold shutdown: coordinator lock is poisoned; forcing exit");
            }
            ShutdownError::Poisoned
        })
    }

    /// Publishes the initial `running` lifecycle event (IF-07).
    pub(crate) fn announce_running(&self) -> Result<(), ShutdownError> {
        self.lock()?.announce_running(current_time_ms())
    }

    /// Advances one bounded shutdown step on the UI thread.
    ///
    /// Errors are logged by the coordinator and retried on later ticks until the
    /// per-phase bound forces the exit; the returned error is for the caller's
    /// information only.
    pub(crate) fn pump(&self, observed_at_ms: i64) -> Result<(), ShutdownError> {
        if self.poisoned.load(Ordering::Acquire) {
            return Err(ShutdownError::Poisoned);
        }
        self.lock()?.step(observed_at_ms)
    }

    /// True once the native loop may leave: shutdown closed with its final event
    /// flushed, or the controller can no longer be trusted.
    pub(crate) fn should_exit(&self) -> bool {
        self.lock()
            .map_or(true, |coordinator| coordinator.may_exit())
    }

    /// Maps how the shutdown ended to the process result.
    ///
    /// Clean and user-chosen abandonment succeed. A drain timeout, an exhausted
    /// retry bound and a poisoned lock are failures and exit nonzero.
    pub(crate) fn exit_result(&self) -> Result<(), ShutdownError> {
        let coordinator = self.lock()?;
        match coordinator.outcome() {
            ShutdownOutcome::Clean | ShutdownOutcome::Abandoned { .. } => Ok(()),
            ShutdownOutcome::DrainTimedOut { jobs, .. } => {
                Err(ShutdownError::DrainTimedOut { remaining: *jobs })
            }
            ShutdownOutcome::Forced(reason) => Err(ShutdownError::RetriesExhausted {
                phase: "forced",
                last: reason.clone(),
            }),
        }
    }

    /// A native window close or application quit: a normal exit (F15 IF-06).
    ///
    /// When change jobs are running the quit is not admitted. The window is told
    /// through an IF-07 `running` event that carries the in-progress change count,
    /// so it can ask the user instead of ignoring the click.
    pub(crate) fn request_window_close(&self) {
        let result = self.lock().and_then(|mut coordinator| {
            let admission = coordinator.request_quit(QuitMode::Waiting, false)?;
            if let QuitAdmission::ConfirmRequired { active_change_jobs } = admission {
                coordinator.announce_confirmation_needed(active_change_jobs, current_time_ms())?;
            }
            Ok(())
        });
        if let Err(error) = result {
            eprintln!("DiskUnfold shutdown: window close was not admitted: {error}");
        }
    }

    #[cfg(test)]
    pub(crate) fn remaining_tasks(&self) -> usize {
        self.lock()
            .map_or(0, |coordinator| coordinator.remaining_tasks())
    }

    #[cfg(test)]
    pub(crate) fn with_ports<R>(&self, read: impl FnOnce(&P) -> R) -> Option<R> {
        self.lock().ok().map(|coordinator| read(&coordinator.ports))
    }

    #[cfg(test)]
    pub(crate) fn with_ports_mut<R>(&self, update: impl FnOnce(&mut P) -> R) -> Option<R> {
        self.lock()
            .ok()
            .map(|mut coordinator| update(&mut coordinator.ports))
    }

    #[cfg(test)]
    pub(crate) fn poison_for_test(&self) {
        let _ = std::thread::scope(|scope| {
            scope
                .spawn(|| {
                    let _guard = self.coordinator.lock();
                    std::panic::resume_unwind(Box::new("poison the coordinator lock"));
                })
                .join()
        });
    }
}

impl<P: ShutdownPorts> QuitHandler for ShutdownController<P> {
    fn request_quit(&self, request: &AppQuitRequest) -> Result<AppQuitResponse, ShutdownError> {
        let mode = match request.mode {
            AppQuitMode::Normal => QuitMode::Waiting,
            AppQuitMode::WithoutWaiting => QuitMode::WithoutWaiting,
        };
        // A change in progress needs the user's confirmation in either mode; the
        // coordinator refuses to admit the quit without it and mutates nothing.
        let admission = self
            .lock()?
            .request_quit(mode, request.confirms_unfinished_changes())?;
        Ok(response(admission))
    }

    fn is_closing(&self) -> bool {
        self.lock()
            .map_or(true, |coordinator| coordinator.is_closing())
    }
}

fn response(admission: QuitAdmission) -> AppQuitResponse {
    let (outcome, count) = match admission {
        QuitAdmission::Accepted { .. } => (AppQuitOutcome::Accepted, None),
        QuitAdmission::ConfirmRequired { active_change_jobs } => (
            AppQuitOutcome::ConfirmRequired,
            Some(u64::try_from(active_change_jobs).unwrap_or(u64::MAX)),
        ),
        QuitAdmission::AlreadyClosing => (AppQuitOutcome::AlreadyClosing, None),
    };
    AppQuitResponse {
        outcome,
        in_progress_change_count: count,
    }
}
