//! Verified, root-only Windows notification primitive.
//!
//! This is not an IF-10 registration: descendants are not observed, the full
//! subtree must remain uncovered, and no lifecycle or event publisher is wired.
//! It exists to exercise identity and native I/O without recursive mount escape.
use crate::{
    windows_observation::{WindowsDirectory, open_matching_scan_root},
    windows_watch_decode::Completion,
    windows_watch_native::NativeWatch,
};
use du_core_contracts::{
    change_notify::{ChangeHint, WatchEvent},
    ports::PortError,
};
use std::{io, path::Path};

#[derive(Debug)]
pub enum Error {
    Identity(PortError),
    Native(io::Error),
}

/// Evidence available after a root notification request has been submitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct RegistrationEvidence {
    /// The identity-checked root handle accepted a notification request.
    pub root_registration: RootRegistration,
    /// Recursive coverage has not been established by this root-only adapter.
    pub recursive_scope: RecursiveScopeEvidence,
}

/// Whether the root request was accepted by the native notification API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootRegistration {
    /// The initial overlapped request was successfully submitted.
    Submitted,
}

/// Evidence about notification coverage below the registered root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecursiveScopeEvidence {
    /// No recursive registration was requested or verified.
    NotEstablished,
}

/// Holds the verified root and ancestors until the native request is reaped.
pub struct RootWatch {
    native: NativeWatch,
    _identity: WindowsDirectory,
}

impl RootWatch {
    /// Starts only after checking the exact scan volume and root file identity.
    /// The OS request is nonrecursive and gives no subtree coverage guarantee.
    pub fn start(path: &Path, volume: &[u8], file: &[u8]) -> Result<Self, Error> {
        let identity = open_matching_scan_root(path, volume, file).map_err(Error::Identity)?;
        let exact = identity.identity().map_err(Error::Identity)?;
        let native =
            NativeWatch::start_root_only(path, volume, exact.file_id).map_err(Error::Native)?;
        Ok(Self {
            native,
            _identity: identity,
        })
    }

    /// Turns the completion into a whole-root hint without trusting path names.
    /// The caller supplies the observation time in its own clock domain.
    pub fn poll(&mut self, observed_at: u64) -> io::Result<Option<WatchEvent>> {
        Ok(match self.native.poll()? {
            Some(Completion::Changes(_)) => Some(WatchEvent::Signal {
                path: None,
                hint: ChangeHint::Unknown,
                observed_at,
            }),
            Some(Completion::Loss) => Some(WatchEvent::Loss { observed_at }),
            Some(Completion::Interrupted(reason)) => Some(WatchEvent::Interrupted {
                reason,
                observed_at,
            }),
            Some(Completion::Cancelled) | None => None,
        })
    }

    /// Reports request registration separately from descendant coverage.
    ///
    /// This records the initial request acceptance. Later failures are reported
    /// by `poll`. Descendant coverage remains unestablished.
    pub fn registration_evidence(&self) -> RegistrationEvidence {
        RegistrationEvidence {
            root_registration: RootRegistration::Submitted,
            recursive_scope: RecursiveScopeEvidence::NotEstablished,
        }
    }

    /// Fences and reaps the request before releasing the held root identity.
    pub fn stop(self) -> io::Result<()> {
        self.native.stop()
    }
}

#[cfg(test)]
mod tests;
