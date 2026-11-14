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
    /// `Some(true)` for a mount root or automount trigger; `None` if not reported.
    boundary: Option<bool>,
}

impl Facts {
    fn from_statx(value: &Statx) -> Self {
        let returned = StatxFlags::from_bits_retain(value.stx_mask);
        let has = |flag| returned.contains(flag);
        let edges = StatxAttributes::MOUNT_ROOT | StatxAttributes::AUTOMOUNT;
        let known = value.stx_attributes_mask & edges;
        let boundary = if !(value.stx_attributes & known).is_empty() {
            Some(true)
        } else {
            known.contains(StatxAttributes::MOUNT_ROOT).then_some(false)
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
            boundary,
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

    /// True when the entry belongs to another volume or mount than the directory.
    fn crosses(&self, anchor: &Anchor) -> bool {
        self.device != anchor.device
            || self.boundary == Some(true)
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
/// An entry on another device or mount, or an automount trigger, is reported as
/// a per-entry failure so the collector never records or enters it.
pub(super) fn observe(anchor: &Anchor, name: &CStr) -> Result<MetadataObservation, PortError> {
    #[cfg(test)]
    PROBES.with(|count| count.set(count.get() + 1));
    let facts = query(
        &*anchor.fd,
        name,
        AtFlags::SYMLINK_NOFOLLOW | AtFlags::NO_AUTOMOUNT,
    )?;
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
