//! Directory identities reserved during one job, with every root first.
use super::Boundary;
use du_core_contracts::{
    scan_observation::{HandleIdentity, MetadataObservation, ObservedEntry, ObservedKind},
    volume::RootVolumeGroup,
};
use std::{
    collections::HashSet,
    sync::{Mutex, MutexGuard, PoisonError},
};

/// How one scope root starts after every root identity was reserved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::scan) enum RootStart {
    /// Enumerate the root and compare its children with this boundary.
    Collect(Boundary),
    /// A root with a lower index reserved the same directory first. This root
    /// is recorded as revisited and never enumerated.
    Revisit,
}

/// Volume and file identities of the directories reserved during one job.
///
/// Every root identity is reserved in root order before any enumeration, so a
/// root reached below another root, or listed twice, is always a revisit
/// there and is collected by itself, whatever the worker timing. Any other
/// directory is reserved by the first admission that observes it. Verified
/// ancestors of a root block only that root's own traversal, so another root
/// that legitimately contains them can still enter them.
///
/// The set is unbounded and lives for the job. It costs one `(device, inode)`
/// pair plus hash overhead, about 40 bytes, per entered directory and per
/// retained root ancestor, and nothing per file. That is a small fraction of
/// the result tree, which already keeps an item for each entered directory. A
/// cap would have to stop admission and leave the rest of the scope unread.
#[derive(Default)]
pub(in crate::scan) struct Visited {
    entered: Mutex<HashSet<(u64, u64)>>,
    ancestors: Vec<HashSet<(u64, u64)>>,
    pub(super) group: Option<RootVolumeGroup>,
}

impl Visited {
    /// Reserves root identities in root order before any root is enumerated.
    ///
    /// `identities` holds one value per root; `None` leaves the boundary of
    /// that root unknown. The returned starts follow the same order.
    pub(in crate::scan) fn reserve_roots(
        identities: Vec<Option<HandleIdentity>>,
    ) -> (Self, Vec<RootStart>) {
        Self::reserve_roots_with_group(identities, None)
    }

    pub(in crate::scan) fn reserve_roots_with_group(
        identities: Vec<Option<HandleIdentity>>,
        group: Option<RootVolumeGroup>,
    ) -> (Self, Vec<RootStart>) {
        let mut entered = HashSet::new();
        let mut ancestors = Vec::with_capacity(identities.len());
        let mut starts = Vec::with_capacity(identities.len());
        for identity in identities {
            let Some(identity) = identity else {
                ancestors.push(HashSet::new());
                starts.push(RootStart::Collect(Boundary::Unknown));
                continue;
            };
            let device = identity.device;
            ancestors.push(identity.ancestors.iter().map(|&id| (device, id)).collect());
            starts.push(if entered.insert((device, identity.inode)) {
                RootStart::Collect(Boundary::Volume(device))
            } else {
                RootStart::Revisit
            });
        }
        let visited = Self {
            entered: Mutex::new(entered),
            ancestors,
            group,
        };
        (visited, starts)
    }

    pub(in crate::scan) fn child_boundary(
        &self,
        entry: &ObservedEntry,
        parent: Boundary,
    ) -> Boundary {
        let Some(group) = &self.group else {
            return Boundary::child(entry);
        };
        if matches!(parent, Boundary::Data(_)) {
            return Boundary::Data(group.device);
        }
        if is_data_mount(entry, group) {
            return Boundary::Data(group.device);
        }
        Boundary::child(entry)
    }

    /// Whether the directory is one end of a confirmed firmlink: the entry on the
    /// system volume and its target below the data mount share this identity.
    pub(super) fn group_firmlink(&self, meta: &MetadataObservation) -> bool {
        self.group.as_ref().is_some_and(|group| {
            group
                .firmlinks
                .iter()
                .any(|link| link.device == meta.device && link.inode == meta.inode)
        })
    }

    pub(super) fn group_data_mount(&self, entry: &ObservedEntry) -> bool {
        self.group
            .as_ref()
            .is_some_and(|group| is_data_mount(entry, group))
    }

    /// True when the directory was reserved or is a verified ancestor of `root`.
    pub(super) fn known(&self, root: u32, device: u64, inode: u64) -> bool {
        self.ancestor(root, device, inode) || self.entered().contains(&(device, inode))
    }

    /// Reserves a directory for `root`; false when it was already known.
    pub(super) fn reserve(&self, root: u32, device: u64, inode: u64) -> bool {
        !self.ancestor(root, device, inode) && self.entered().insert((device, inode))
    }

    fn ancestor(&self, root: u32, device: u64, inode: u64) -> bool {
        usize::try_from(root)
            .ok()
            .and_then(|index| self.ancestors.get(index))
            .is_some_and(|set| set.contains(&(device, inode)))
    }

    fn entered(&self) -> MutexGuard<'_, HashSet<(u64, u64)>> {
        self.entered.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn is_data_mount(entry: &ObservedEntry, group: &RootVolumeGroup) -> bool {
    entry.metadata.as_ref().is_ok_and(|meta| {
        meta.kind == ObservedKind::MountPoint
            && meta.device == group.device
            && meta.inode == group.data_inode
            && matches!(
                entry.enumerated_kind,
                ObservedKind::Unknown | ObservedKind::Directory | ObservedKind::MountPoint
            )
    })
}
