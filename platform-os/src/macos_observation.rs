//! Handle-relative, read-only observations on macOS volumes.
//!
//! Directories are read with `getattrlistbulk`, which returns each name with
//! its kind, sizes, identity and modification time in one record, so no item
//! is opened to read its metadata. Child directories are opened relative to the
//! parent descriptor with `O_NOFOLLOW` and must match the enumerated identity.
//! Symbolic links, firmlinks, mount points and automount triggers are reported
//! as links and never entered. Directory reads and opens refuse dataless-object
//! materialization on the calling thread, so no file provider is asked to
//! download content.
mod bulk;
mod dataless;
mod enumeration;
mod errors;
mod scope;
#[cfg(test)]
mod tests;
mod volume;

pub use scope::MacScopeRoots;

use du_core_contracts::{
    Timestamp,
    ports::PortError,
    scan_observation::{
        AnchoredDirectory, ObservedEntry, ObservedKind, TimedObservedEntry, VolumeSpaceReader,
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
            finished: false,
            ancestors,
        }
    }
}

impl AnchoredDirectory for MacDirectory {
    fn open_child(&self, entry: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        let meta = entry.metadata.as_ref().map_err(Clone::clone)?;
        if meta.kind != ObservedKind::Directory
            || meta.device != self.device
            || meta.inode == 0
            || entry.enumerated_inode != meta.inode
        {
            return Err(errors::invalid());
        }
        let name = scope::component(&entry.name)?;
        let (fd, stat) = scope::open_directory(self.fd.as_fd(), &name)?;
        if scope::device(&stat) != self.device || stat.st_ino != meta.inode {
            return Err(errors::changed());
        }
        Ok(Box::new(Self::new(
            fd,
            (self.device, meta.inode),
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
}
