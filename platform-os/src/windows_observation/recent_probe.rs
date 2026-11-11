//! Optional same-handle recent-scope observation, not wired to scan admission.
use super::*;
use du_core_contracts::{OsPath, PathBytes, PathEncoding, scan_observation::OpenedScopeRoot};
use std::sync::atomic::{AtomicBool, Ordering};

use du_core_contracts::scan_observation::{OptionalRecentScopeObserver, RecentScopeObserver};
pub use du_core_contracts::scan_observation::{
    RecentScopeEvidence as RecentEvidence, RecentScopeObservation as ProbeOutcome,
};

#[cfg(test)]
thread_local! {
    static OBSERVATION_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Shared ownership of the originally opened directory handle for later review.
///
/// This is not a durable name lock. The probe may block, and cancellation does
/// not interrupt an in-flight Windows call. It is not a persisted scope format.
pub struct RecentProbe {
    handle: Arc<File>,
    canonical: Vec<u8>,
    volume: Vec<u8>,
    serial: u64,
    file: u64,
}

impl WindowsScopeRoots {
    /// Opens once and retains shared ownership of the scan root handle for review.
    ///
    /// This standalone opt-in entry point is not used by the normal scan port.
    pub fn open_with_recent_probe(
        &self,
        path: &[u8],
    ) -> Result<(OpenedScopeRoot, RecentProbe), PortError> {
        open_with_probe_using(path)
    }
}

pub(super) fn open_port_with_recent(
    path: &[u8],
) -> Result<(OpenedScopeRoot, OptionalRecentScopeObserver), PortError> {
    open_port_with_recent_using(path)
}

fn open_port_with_recent_using(
    path: &[u8],
) -> Result<(OpenedScopeRoot, OptionalRecentScopeObserver), PortError> {
    let (root, probe) = open_with_probe_using(path)?;
    Ok((
        root,
        Some(Ok(Box::new(probe) as Box<dyn RecentScopeObserver>)),
    ))
}

fn open_with_probe_using(path: &[u8]) -> Result<(OpenedScopeRoot, RecentProbe), PortError> {
    let (directory, identity) = scope::open_scope(path)?;
    let probe = create_probe(&directory, &identity);
    Ok((scope::into_scope_root(directory, identity), probe))
}

fn create_probe(directory: &WindowsDirectory, identity: &DirectoryIdentity) -> RecentProbe {
    RecentProbe {
        handle: Arc::clone(&directory.handle),
        canonical: bytes(&identity.physical_path),
        volume: directory.volume_guid.to_vec(),
        serial: identity.volume,
        file: identity.file_id,
    }
}

impl RecentScopeObserver for RecentProbe {
    fn observe(&self, cancelled: &AtomicBool) -> ProbeOutcome {
        RecentProbe::observe(self, cancelled)
    }
}

impl RecentProbe {
    /// Observes the retained handle with cancellation checks before, between,
    /// and after synchronous operations. A cancelled result contains no data.
    /// The publishing owner must also recheck cancellation before storing.
    pub fn observe(&self, cancelled: &AtomicBool) -> ProbeOutcome {
        self.observe_with(
            cancelled,
            |handle| final_path(handle, VOLUME_NAME_GUID),
            || {},
        )
    }

    fn observe_with(
        &self,
        cancelled: &AtomicBool,
        guid: impl FnOnce(&File) -> Result<Vec<u16>, PortError>,
        after: impl FnOnce(),
    ) -> ProbeOutcome {
        if cancelled.load(Ordering::Acquire) {
            return ProbeOutcome::Cancelled;
        }
        let result = self.capture(cancelled, guid);
        after();
        if cancelled.load(Ordering::Acquire) {
            return ProbeOutcome::Cancelled;
        }
        match result {
            Ok(value) => ProbeOutcome::Observed(value),
            Err(error) => ProbeOutcome::Unavailable(error),
        }
    }

    fn capture(
        &self,
        cancelled: &AtomicBool,
        guid: impl FnOnce(&File) -> Result<Vec<u16>, PortError>,
    ) -> Result<RecentEvidence, PortError> {
        #[cfg(test)]
        OBSERVATION_COUNT.with(|count| count.set(count.get() + 1));
        let before = info(&self.handle)?;
        recall::check_handle_attributes(before.dwFileAttributes)?;
        let dos = final_path(&self.handle, FILE_NAME_NORMALIZED)?;
        if cancelled.load(Ordering::Acquire) {
            return Err(invalid());
        }
        let guid = guid(&self.handle)?;
        if cancelled.load(Ordering::Acquire) {
            return Err(invalid());
        }
        let after = info(&self.handle)?;
        recall::check_handle_attributes(after.dwFileAttributes)?;
        let dos_after = final_path(&self.handle, FILE_NAME_NORMALIZED)?;
        let volume = scope::volume_name(&guid)?;
        if self.file == 0
            || kind(before.dwFileAttributes) != ObservedKind::Directory
            || kind(after.dwFileAttributes) != ObservedKind::Directory
            || identity(&before) != self.file
            || identity(&after) != self.file
            || u64::from(before.dwVolumeSerialNumber) != self.serial
            || u64::from(after.dwVolumeSerialNumber) != self.serial
            || bytes(&dos) != self.canonical
            || dos != dos_after
            || volume != self.volume
        {
            return Err(PortError {
                kind: PortErrorKind::IdentityChanged,
                reason_key: "scope.changed".into(),
                os_code: None,
            });
        }
        let (entry, relative) = split_paths(&dos, &guid, &volume)?;
        Ok(RecentEvidence {
            volume_id: volume,
            entry_path: native(entry),
            relative_path: native(relative),
            file_id: self.file.to_le_bytes().to_vec(),
        })
    }
}

fn native(path: &[u16]) -> OsPath {
    let display = String::from_utf16(path);
    OsPath {
        encoding: PathEncoding::WindowsUtf16le,
        raw: PathBytes::from_bytes(&bytes(path)),
        lossy: display.is_err(),
        display: display.unwrap_or_else(|_| String::from_utf16_lossy(path)),
    }
}

fn split_paths<'a>(
    dos: &'a [u16],
    guid: &'a [u16],
    volume: &[u8],
) -> Result<(&'a [u16], &'a [u16]), PortError> {
    let (pairs, tail) = volume.as_chunks::<2>();
    if !tail.is_empty() {
        return Err(invalid());
    }
    let prefix: Vec<u16> = pairs.iter().copied().map(u16::from_le_bytes).collect();
    let suffix = guid.strip_prefix(prefix.as_slice()).ok_or_else(invalid)?;
    if suffix.first() != Some(&(b'\\' as u16)) {
        return Err(invalid());
    }
    let [92, 92, 63, 92, drive, 58, 92, rest @ ..] = dos else {
        return Err(invalid());
    };
    if !u8::try_from(*drive).is_ok_and(|d| d.is_ascii_alphabetic()) || rest != &suffix[1..] {
        return Err(invalid());
    }
    Ok((&dos[..7], suffix))
}

#[cfg(test)]
mod tests;
