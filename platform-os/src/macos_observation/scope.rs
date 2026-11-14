//! Scope roots opened component by component from `/` without following links.
use super::{MacDirectory, dataless, errors, volume};
use du_core_contracts::{
    OsPath, PathBytes, PathEncoding,
    ports::PortError,
    scan_observation::{OpenedScopeRoot, ScopeRootPort},
};
use rustix::{
    fs::{FileType, Mode, OFlags, Stat, fstat, getpath, open, openat},
    io::Errno,
};
use std::{
    ffi::{CStr, CString},
    os::fd::{AsFd, BorrowedFd, OwnedFd},
    sync::Arc,
};

/// Opens verified, read-only scope roots on macOS volumes for the scan collector.
///
/// `absolute_path` holds native path bytes starting with `/`. Each component is
/// opened relative to its already open parent with `O_NOFOLLOW`, so a symbolic
/// link anywhere in the path is rejected with `path.unresolvable`; pass a
/// resolved path such as `/private/tmp` instead of `/tmp`. `.` and `..` are
/// rejected. Firmlinks and mount points along the path are crossed, and the
/// root belongs to the volume of the final directory.
///
/// The canonical path is read back from the opened descriptor. `volume_id` is
/// `uuid:` and the volume UUID, or `dev:` and the hexadecimal device number when
/// the file system reports no UUID; `file_id` is the inode as little-endian
/// bytes. Ancestor identities are available only when every ancestor up to a
/// confirmed volume root is on the same volume; an APFS firmlink such as
/// `/Users` leaves them unavailable. Pass the returned directory to the
/// collector unchanged; reopening the canonical path is not equivalent.
pub struct MacScopeRoots;

impl ScopeRootPort for MacScopeRoots {
    fn open_root(&self, absolute_path: &[u8]) -> Result<OpenedScopeRoot, PortError> {
        let directory = open_verified(absolute_path)?;
        let canonical = getpath(&directory.fd).map_err(errors::os)?;
        let raw = canonical.as_bytes();
        Ok(OpenedScopeRoot {
            canonical_path: OsPath {
                encoding: PathEncoding::Unix,
                raw: PathBytes::from_bytes(raw),
                lossy: std::str::from_utf8(raw).is_err(),
                display: String::from_utf8_lossy(raw).into_owned(),
            },
            volume_id: directory.volume_id.to_vec(),
            file_id: directory.inode.to_le_bytes().to_vec(),
            directory: Box::new(directory),
        })
    }
}

fn open_verified(path: &[u8]) -> Result<MacDirectory, PortError> {
    let names = components(path)?;
    let mut current =
        dataless::without_materialization(|| retry(|| open("/", flags(), Mode::empty())))?
            .map_err(errors::os)?;
    let mut walked = Vec::with_capacity(names.len());
    for name in &names {
        let (next, _) = open_directory(current.as_fd(), name)?;
        walked.push(std::mem::replace(&mut current, next));
    }
    let stat = directory_stat(&current)?;
    let volume_id: Arc<[u8]> = volume::identity(current.as_fd())?.into();
    let root = volume::is_mount_root(current.as_fd()).unwrap_or(false);
    let ancestors = Ancestors::collect(&volume_id, root, walked);
    Ok(MacDirectory::new(
        current,
        (device(&stat), stat.st_ino),
        volume_id,
        ancestors,
    ))
}

fn flags() -> OFlags {
    // NONBLOCK keeps a FIFO from blocking before DIRECTORY rejects it.
    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK
}

/// Opens one directory entry relative to `parent` without following a link.
///
/// A link reports `ENOTDIR`. A dataless directory is refused, because reading it
/// would ask its file provider to materialize the listing.
pub(super) fn open_directory(
    parent: BorrowedFd<'_>,
    name: &CStr,
) -> Result<(OwnedFd, Stat), PortError> {
    let fd = dataless::without_materialization(|| {
        retry(|| openat(parent, name, flags(), Mode::empty()))
    })?
    .map_err(errors::os)?;
    let stat = directory_stat(&fd)?;
    Ok((fd, stat))
}

fn directory_stat(fd: &OwnedFd) -> Result<Stat, PortError> {
    let stat = fstat(fd).map_err(errors::os)?;
    if FileType::from_raw_mode(stat.st_mode) != FileType::Directory {
        return Err(errors::invalid());
    }
    if stat.st_flags & dataless::SF_DATALESS != 0 {
        return Err(errors::unavailable());
    }
    Ok(stat)
}

/// Device number in the form enumeration records report it.
pub(super) fn device(stat: &Stat) -> u64 {
    u64::from(stat.st_dev.cast_unsigned())
}

/// Validates one native name as a single path component.
pub(super) fn component(name: &[u8]) -> Result<CString, PortError> {
    if name.is_empty() || name == b"." || name == b".." || name.contains(&b'/') {
        return Err(errors::invalid());
    }
    CString::new(name).map_err(|_| errors::invalid())
}

fn components(path: &[u8]) -> Result<Vec<CString>, PortError> {
    if path.first() != Some(&b'/') {
        return Err(errors::invalid());
    }
    path.split(|&byte| byte == b'/')
        .filter(|part| !part.is_empty())
        .map(component)
        .collect()
}

fn retry<T>(mut call: impl FnMut() -> rustix::io::Result<T>) -> rustix::io::Result<T> {
    loop {
        match call() {
            Err(Errno::INTR) => {}
            result => return result,
        }
    }
}

/// Verified same-volume ancestors, kept open so their identities stay meaningful.
pub(super) struct Ancestors {
    _handles: Vec<OwnedFd>,
    ids: Vec<Vec<u8>>,
}

impl Ancestors {
    pub(super) fn ids(&self) -> &[Vec<u8>] {
        &self.ids
    }

    /// Walks back from the parent of the opened directory. Evidence exists only
    /// when the chain reaches a confirmed volume root without leaving the volume.
    /// An opened directory that is itself a volume root has no ancestors.
    fn collect(volume_id: &[u8], root: bool, mut walked: Vec<OwnedFd>) -> Option<Self> {
        let mut chain = Self {
            _handles: Vec::new(),
            ids: Vec::new(),
        };
        let mut at_root = root;
        while !at_root {
            let handle = walked.pop()?;
            if volume::identity(handle.as_fd()).ok()? != volume_id {
                return None;
            }
            chain
                .ids
                .push(fstat(&handle).ok()?.st_ino.to_le_bytes().to_vec());
            at_root = volume::is_mount_root(handle.as_fd()).ok()?;
            chain._handles.push(handle);
        }
        chain.ids.reverse();
        Some(chain)
    }
}
