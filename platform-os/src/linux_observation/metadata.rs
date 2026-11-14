//! Entry and handle metadata, each observed with one `statx` call.
use super::{
    Anchor,
    errors::{errno, invalid, unavailable},
};
use du_core_contracts::{
    Timestamp,
    ports::PortError,
    scan_observation::{MetadataObservation, ObservedKind, SpaceIdentity},
};
use rustix::{
    fd::AsFd,
    fs::{AtFlags, FileType, Statx, StatxAttributes, StatxFlags, makedev, statx},
};
use std::ffi::CStr;

#[cfg(test)]
thread_local! {
    /// Number of entry metadata queries made on this thread.
    pub(super) static PROBES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Attributes requested together; any the kernel omits stay unconfirmed.
const MASK: StatxFlags = StatxFlags::TYPE
    .union(StatxFlags::NLINK)
    .union(StatxFlags::INO)
    .union(StatxFlags::SIZE)
    .union(StatxFlags::BLOCKS)
    .union(StatxFlags::MTIME)
    .union(StatxFlags::MNT_ID);

/// `stx_blocks` counts 512-byte units regardless of the file system block size.
const BLOCK_BYTES: u64 = 512;

/// Facts from one query; `None` marks a value the kernel did not return.
pub(super) struct Facts {
    file_type: Option<FileType>,
    pub(super) device: u64,
    pub(super) inode: Option<u64>,
    size: Option<u64>,
    blocks: Option<u64>,
    links: Option<u64>,
    modified: Option<(i64, u32)>,
    pub(super) mount: Option<u64>,
    /// `STATX_ATTR_MOUNT_ROOT`; `None` when the kernel does not report it.
    mount_root: Option<bool>,
    /// `STATX_ATTR_AUTOMOUNT` was reported for an automount trigger.
    automount: bool,
}

impl Facts {
    fn from_statx(value: &Statx) -> Self {
        let returned = StatxFlags::from_bits_retain(value.stx_mask);
        let has = |flag| returned.contains(flag);
        let attribute = |flag: StatxAttributes| {
            value
                .stx_attributes_mask
                .contains(flag)
                .then(|| value.stx_attributes.contains(flag))
        };
        Self {
            file_type: has(StatxFlags::TYPE)
                .then(|| FileType::from_raw_mode(u32::from(value.stx_mode))),
            device: makedev(value.stx_dev_major, value.stx_dev_minor),
            inode: has(StatxFlags::INO).then_some(value.stx_ino),
            size: has(StatxFlags::SIZE).then_some(value.stx_size),
            blocks: has(StatxFlags::BLOCKS).then_some(value.stx_blocks),
            links: has(StatxFlags::NLINK).then_some(u64::from(value.stx_nlink)),
            modified: has(StatxFlags::MTIME)
                .then_some((value.stx_mtime.tv_sec, value.stx_mtime.tv_nsec)),
            mount: has(StatxFlags::MNT_ID).then_some(value.stx_mnt_id),
            mount_root: attribute(StatxAttributes::MOUNT_ROOT),
            automount: attribute(StatxAttributes::AUTOMOUNT) == Some(true),
        }
    }

    pub(super) fn kind(&self) -> ObservedKind {
        kind(self.file_type)
    }

    pub(super) fn is_directory(&self) -> bool {
        self.file_type == Some(FileType::Directory)
    }

    /// Last modification time floored to whole milliseconds.
    pub(super) fn modified_at(&self) -> Option<Timestamp> {
        let (seconds, nanos) = self.modified?;
        if nanos >= 1_000_000_000 {
            return None;
        }
        let millis = seconds
            .checked_mul(1000)?
            .checked_add(i64::from(nanos / 1_000_000))?;
        Timestamp::from_unix_millis(millis)
    }

    /// True when this observation itself shows a directory with another mount
    /// on it: the kernel reports a mount root or a mount ID that differs from
    /// the parent's. A different device alone does not establish a mount.
    fn mounted_directory(&self, anchor: &Anchor) -> bool {
        self.is_directory()
            && (self.mount_root == Some(true) || !super::same_mount(self.mount, anchor.mount))
    }

    /// True for another mount of the directory's device or an automount trigger.
    ///
    /// Mounted directories and entries on another device are handled before
    /// this check, so this leaves file mounts and unmounted automount triggers.
    fn crosses(&self, anchor: &Anchor) -> bool {
        self.automount
            || self.mount_root == Some(true)
            || !super::same_mount(self.mount, anchor.mount)
    }
}

/// Maps an OS file type to the observation kind without following links.
pub(super) fn kind(file_type: Option<FileType>) -> ObservedKind {
    match file_type {
        Some(FileType::RegularFile) => ObservedKind::File,
        Some(FileType::Directory) => ObservedKind::Directory,
        Some(FileType::Symlink) => ObservedKind::Link,
        Some(FileType::Unknown) | None => ObservedKind::Unknown,
        Some(_) => ObservedKind::Other,
    }
}

/// Observes the open handle itself.
pub(super) fn handle(fd: impl AsFd) -> Result<Facts, PortError> {
    query(
        fd,
        c"",
        AtFlags::EMPTY_PATH | AtFlags::SYMLINK_NOFOLLOW | AtFlags::NO_AUTOMOUNT,
    )
}

fn query(dir: impl AsFd, name: &CStr, flags: AtFlags) -> Result<Facts, PortError> {
    statx(dir, name, flags, MASK)
        .map(|value| Facts::from_statx(&value))
        .map_err(errno)
}

/// Observes one admitted entry relative to its verified directory.
///
/// A directory with a mount on it, on the same device or another, is a
/// [`ObservedKind::MountPoint`] with the mounted root's device and inode only.
/// The lookup crosses into the mounted root, so the kernel reports the mount
/// root attribute and the mount ID there (Linux 5.8 and later). Without that
/// evidence, an entry on another device keeps its own kind under the same
/// rule. A file mount on the parent's device and an unmounted automount
/// trigger remain per-entry failures. None of these is ever entered.
pub(super) fn observe(anchor: &Anchor, name: &CStr) -> Result<MetadataObservation, PortError> {
    #[cfg(test)]
    PROBES.with(|count| count.set(count.get() + 1));
    let facts = query(
        &*anchor.fd,
        name,
        AtFlags::SYMLINK_NOFOLLOW | AtFlags::NO_AUTOMOUNT,
    )?;
    classify(anchor, &facts)
}

/// Applies the rules of [`observe`] to the facts of one entry query.
fn classify(anchor: &Anchor, facts: &Facts) -> Result<MetadataObservation, PortError> {
    if facts.mounted_directory(anchor) {
        return Ok(other_volume(facts, ObservedKind::MountPoint));
    }
    if facts.device != anchor.device {
        return Ok(other_volume(facts, facts.kind()));
    }
    if facts.crosses(anchor) {
        return Err(invalid());
    }
    let kind = facts.kind();
    let file = kind == ObservedKind::File;
    Ok(MetadataObservation {
        device: facts.device,
        inode: facts.inode.unwrap_or(0),
        size: facts.size.ok_or_else(unavailable),
        modified_at: facts.modified_at(),
        native_modified_at: None,
        allocated: facts
            .blocks
            .filter(|_| file)
            .and_then(|blocks| blocks.checked_mul(BLOCK_BYTES)),
        link_count: facts.links.filter(|&links| file && links > 0),
        space_identity: facts.inode.filter(|_| file).map(|inode| SpaceIdentity {
            volume: anchor.volume_id.to_vec(),
            file: inode.to_le_bytes().to_vec(),
        }),
        kind,
    })
}

/// An entry on another volume: its own identity and `kind`, with no size,
/// time, allocation, link count or space identity, as `MetadataObservation`
/// requires.
fn other_volume(facts: &Facts, kind: ObservedKind) -> MetadataObservation {
    MetadataObservation {
        kind,
        device: facts.device,
        inode: facts.inode.unwrap_or(0),
        size: Err(unavailable()),
        modified_at: None,
        native_modified_at: None,
        allocated: None,
        link_count: None,
        space_identity: None,
    }
}

#[cfg(test)]
mod tests;
