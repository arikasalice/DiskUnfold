//! Handle-relative, read-only observations on macOS volumes.
//!
//! Directories are read with `getattrlistbulk`, which returns each name with
//! its kind, sizes, identity and modification time in one record, so no item
//! is opened to read its metadata. Child directories are opened relative to the
//! parent descriptor with `O_NOFOLLOW` and must match the enumerated identity.
//! Mount points are reported as such, firmlinks as firmlinks, and symbolic links
//! and automount triggers as links. The collector enters a firmlink or the data
//! mount only in a scope of the confirmed root volume group (#263); everything else
//! of these is never entered. Directory
//! reads and opens refuse dataless-object
//! materialization on the calling thread, so no file provider is asked to
//! download content.
mod bulk;
mod dataless;
mod enumeration;
pub(crate) mod errors;
pub(crate) mod scope;
#[cfg(test)]
mod tests;
pub(crate) mod volume;

pub use scope::MacScopeRoots;

use du_core_contracts::{
    Timestamp,
    ports::PortError,
    scan_observation::{
        AnchoredDirectory, HandleIdentity, ObservedEntry, ObservedKind, TimedObservedEntry,
        VolumeDevice, VolumeSpaceReader,
    },
};
use std::{
    collections::VecDeque,
    os::fd::{AsFd, OwnedFd},
    sync::Arc,
};

/// A verified, open directory. Entry names are the native bytes the volume stores.
struct MacDirectory {
    fd: OwnedFd,
    /// Device number as enumeration reports it; shared by an APFS volume group.
    device: u64,
    inode: u64,
    /// Identity of the actual volume, also used in file space identities.
    volume_id: Arc<[u8]>,
    pending: VecDeque<TimedObservedEntry>,
    filtered: Option<bool>,
    filter_failed: bool,
    correlated: bool,
    correlated_records: VecDeque<bulk::Record>,
    correlated_next_child_id: Option<u64>,
    correlated_pending_observation: Option<(u64, TimedObservedEntry)>,
    correlated_stream_error: Option<PortError>,
    correlated_ended: bool,
    finished: bool,
    ancestors: Option<scope::Ancestors>,
}

impl MacDirectory {
    fn new(
        fd: OwnedFd,
        (device, inode): (u64, u64),
        volume_id: Arc<[u8]>,
        ancestors: Option<scope::Ancestors>,
    ) -> Self {
        Self {
            fd,
            device,
            inode,
            volume_id,
            pending: VecDeque::new(),
            filtered: None,
            filter_failed: false,
            correlated: false,
            correlated_records: VecDeque::new(),
            correlated_next_child_id: None,
            correlated_pending_observation: None,
            correlated_stream_error: None,
            correlated_ended: false,
            finished: false,
            ancestors,
        }
    }

    /// A failure on this directory, reported as a disconnection when its own verified
    /// descriptor no longer answers. After a forced eject macOS answers reads below the
    /// volume with `EBADF`, `ENOTDIR` or `ENOENT`, which alone do not tell a removed
    /// volume from a changed entry; the descriptor does. Any other failure is kept.
    pub(super) fn after_failure(&self, error: PortError) -> PortError {
        if error.reason_key == "scan.disconnected" {
            return error;
        }
        match rustix::fs::fstat(&self.fd) {
            Err(code) if errors::revoked(Some(code.raw_os_error())) => {
                errors::disconnected(error.os_code)
            }
            _ => error,
        }
    }
}

impl AnchoredDirectory for MacDirectory {
    fn device(&self) -> VolumeDevice {
        crate::device::map_probe(crate::device::macos::probe_fd(self.fd.as_fd()))
    }

    fn open_child(&self, entry: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        let meta = entry.metadata.as_ref().map_err(Clone::clone)?;
        // A firmlink's metadata already names the directory it leads to, and the
        // data mount of the root volume group shares the system volume's device.
        let expected = match meta.kind {
            ObservedKind::Directory if entry.enumerated_inode == meta.inode => Some(meta.inode),
            ObservedKind::Firmlink => Some(meta.inode),
            ObservedKind::MountPoint => None,
            _ => return Err(errors::invalid()),
        };
        if meta.device != self.device || meta.inode == 0 {
            return Err(errors::invalid());
        }
        let name = scope::component(&entry.name)?;
        let (fd, stat) = scope::open_directory(self.fd.as_fd(), &name)
            .map_err(|error| self.after_failure(error))?;
        let same = scope::device(&stat) == self.device
            && match expected {
                Some(inode) => stat.st_ino == inode,
                // The mounted root reports its own inode; it must be a mount root.
                None => volume::is_mount_root(fd.as_fd()).unwrap_or(false),
            };
        if !same {
            return Err(errors::changed());
        }
        Ok(Box::new(Self::new(
            fd,
            (self.device, stat.st_ino),
            Arc::clone(&self.volume_id),
            None,
        )))
    }

    fn volume_space_reader(&self) -> Result<Box<dyn VolumeSpaceReader>, PortError> {
        volume::reader(self.fd.as_fd())
    }

    fn observed_modified_at(&self) -> Option<Timestamp> {
        let stat = rustix::fs::fstat(&self.fd).ok()?;
        bulk::timestamp(stat.st_mtime, stat.st_mtime_nsec)
    }

    fn observed_modified_at_timed(
        &self,
        clock: &dyn Fn() -> Option<Timestamp>,
    ) -> (Option<Timestamp>, Option<Timestamp>) {
        let modified_at = self.observed_modified_at();
        let observed_at = modified_at.as_ref().and_then(|_| clock());
        (modified_at, observed_at)
    }

    fn ancestor_file_ids(&self) -> Option<&[Vec<u8>]> {
        self.ancestors.as_ref().map(scope::Ancestors::ids)
    }

    /// Reports the device and inode this descriptor was verified with when it
    /// was opened; an open descriptor keeps referring to that directory.
    fn observed_identity(&self) -> Option<HandleIdentity> {
        scope::handle_identity(self.device, self.inode, self.ancestor_file_ids())
    }
}
