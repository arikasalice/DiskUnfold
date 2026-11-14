//! Handle-relative, read-only scan observations on Linux.
//!
//! Directories are opened relative to an already verified parent descriptor
//! without following links, and entries are observed with
//! `statx(AT_SYMLINK_NOFOLLOW | AT_NO_AUTOMOUNT)`. Collection never reads file
//! content, never triggers an automount and never requests more privilege.
mod enumeration;
mod errors;
mod metadata;
mod scope;
#[cfg(test)]
mod tests;
mod volume_space;

pub use scope::LinuxScopeRoots;

use du_core_contracts::{
    Timestamp,
    ports::PortError,
    scan_observation::{
        AnchoredDirectory, ObservedEntry, ObservedKind, TimedObservedEntry, VolumeSpaceReader,
    },
};
use errors::{errno, invalid};
use rustix::{
    fd::OwnedFd,
    fs::{Dir, Mode, OFlags, ResolveFlags, openat2},
};
use std::{collections::VecDeque, ffi::CString, sync::Arc};

/// Read-only directory opens that never follow a link or leak into children.
const DIRECTORY: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC)
    .union(OFlags::NOCTTY);

/// Child opens stay beneath the parent handle and on its mount.
const CHILD: ResolveFlags = ResolveFlags::BENEATH
    .union(ResolveFlags::NO_SYMLINKS)
    .union(ResolveFlags::NO_MAGICLINKS)
    .union(ResolveFlags::NO_XDEV);

/// A verified directory descriptor and the evidence that bounds its traversal.
#[derive(Clone)]
struct Anchor {
    fd: Arc<OwnedFd>,
    /// Device of this directory; entries on another device are outside the volume.
    device: u64,
    /// Mount ID when the kernel reports it; it also separates same-device bind mounts.
    mount: Option<u64>,
    /// Adapter-owned volume identity shared by every directory of one scope root.
    volume_id: Arc<[u8]>,
}

/// Same-mount ancestor descriptors, retained while their IDs are offered as evidence.
struct AncestorChain {
    _handles: Vec<OwnedFd>,
    ids: Vec<Vec<u8>>,
}

/// One verified, open directory. Entry names are the raw directory entry bytes.
struct LinuxDirectory {
    anchor: Anchor,
    entries: Option<Dir>,
    pending: VecDeque<TimedObservedEntry>,
    pending_error: Option<PortError>,
    filtered: Option<bool>,
    filter_failed: bool,
    finished: bool,
    ancestors: Option<AncestorChain>,
}

impl LinuxDirectory {
    fn new(anchor: Anchor, ancestors: Option<AncestorChain>) -> Self {
        Self {
            anchor,
            entries: None,
            pending: VecDeque::new(),
            pending_error: None,
            filtered: None,
            filter_failed: false,
            finished: false,
            ancestors,
        }
    }
}

/// Converts an enumerated name into a single path component.
fn component(name: &[u8]) -> Result<CString, PortError> {
    if name.is_empty() || name == b"." || name == b".." || name.contains(&b'/') {
        return Err(invalid());
    }
    CString::new(name).map_err(|_| invalid())
}

fn same_mount(left: Option<u64>, right: Option<u64>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => left == right,
        _ => true,
    }
}

impl AnchoredDirectory for LinuxDirectory {
    fn open_child(&self, entry: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        let meta = entry.metadata.as_ref().map_err(Clone::clone)?;
        if meta.kind != ObservedKind::Directory
            || meta.device != self.anchor.device
            || meta.inode == 0
            || entry.enumerated_inode != meta.inode
        {
            return Err(invalid());
        }
        let name = component(&entry.name)?;
        let fd = openat2(
            &*self.anchor.fd,
            name.as_c_str(),
            DIRECTORY,
            Mode::empty(),
            CHILD,
        )
        .map_err(errno)?;
        // The entry may have been replaced after its metadata was observed.
        let opened = metadata::handle(&fd)?;
        if !opened.is_directory()
            || opened.device != self.anchor.device
            || opened.inode != Some(meta.inode)
            || !same_mount(opened.mount, self.anchor.mount)
        {
            return Err(invalid());
        }
        Ok(Box::new(LinuxDirectory::new(
            Anchor {
                fd: Arc::new(fd),
                ..self.anchor.clone()
            },
            None,
        )))
    }

    fn volume_space_reader(&self) -> Result<Box<dyn VolumeSpaceReader>, PortError> {
        Ok(Box::new(volume_space::Reader::new(&self.anchor)))
    }

    fn observed_modified_at(&self) -> Option<Timestamp> {
        metadata::handle(&*self.anchor.fd)
            .ok()
            .and_then(|facts| facts.modified_at())
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
        self.ancestors.as_ref().map(|chain| chain.ids.as_slice())
    }
}
