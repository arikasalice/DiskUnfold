//! Interrupt requests (Ctrl+C) for the command-line mode (F39 FR10.HD1.cli.DD1).
//!
//! The handler only counts requests; the command decides what each one means
//! (the first cancels its job, the second stops waiting). Installing it once
//! per process is enough, and later installs share the same count.
use std::sync::{
    OnceLock,
    atomic::{AtomicUsize, Ordering},
};

use du_core_contracts::ports::{PortError, PortErrorKind};

static REQUESTS: AtomicUsize = AtomicUsize::new(0);
static INSTALLED: OnceLock<bool> = OnceLock::new();

/// Interrupt requests received by this process since the handler was installed.
#[derive(Clone, Copy, Debug)]
pub struct Interrupts;

impl Interrupts {
    /// Installs the process handler, once.
    ///
    /// # Errors
    /// `os.unsupported` when the platform refuses the handler; the command then
    /// runs without interrupt handling and the default action applies.
    pub fn install() -> Result<Self, PortError> {
        if *INSTALLED.get_or_init(platform::install) {
            Ok(Self)
        } else {
            Err(PortError {
                kind: PortErrorKind::Unsupported,
                reason_key: "os.unsupported".into(),
                os_code: None,
            })
        }
    }

    /// How many interrupt requests arrived so far.
    pub fn count(self) -> usize {
        REQUESTS.load(Ordering::Acquire)
    }
}

fn record() {
    REQUESTS.fetch_add(1, Ordering::AcqRel);
}

#[cfg(windows)]
mod platform {
    use windows::Win32::{
        Foundation::{FALSE, TRUE},
        System::Console::{CTRL_BREAK_EVENT, CTRL_C_EVENT, SetConsoleCtrlHandler},
    };

    unsafe extern "system" fn handler(kind: u32) -> windows::core::BOOL {
        if kind == CTRL_C_EVENT || kind == CTRL_BREAK_EVENT {
            super::record();
            TRUE
        } else {
            FALSE
        }
    }

    pub(super) fn install() -> bool {
        // SAFETY: the handler only touches an atomic and lives for the process.
        unsafe { SetConsoleCtrlHandler(Some(handler), true) }.is_ok()
    }
}

#[cfg(unix)]
mod platform {
    extern "C" fn handler(_: libc::c_int) {
        // Async-signal-safe: one atomic increment.
        super::record();
    }

    pub(super) fn install() -> bool {
        // SAFETY: a zeroed sigaction is a valid empty action; the handler only
        // touches an atomic and lives for the process.
        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = handler as *const () as libc::sighandler_t;
            action.sa_flags = libc::SA_RESTART;
            libc::sigemptyset(&mut action.sa_mask);
            libc::sigaction(libc::SIGINT, &action, std::ptr::null_mut()) == 0
        }
    }
}

#[cfg(test)]
mod tests;
