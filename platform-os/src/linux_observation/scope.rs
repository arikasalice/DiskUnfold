//! Scope admission: resolve an absolute path from `/` one component at a time.
use super::{
    AncestorChain, Anchor, DIRECTORY, LinuxDirectory,
    errors::{errno, invalid},
    metadata::{self, Facts},
};
use du_core_contracts::{
    OsPath, PathBytes, PathEncoding,
    ports::PortError,
    scan_observation::{
        DirectoryObservationPort, DirectoryObservations, OpenedScopeRoot, ScopeRootPort,
    },
};
use rustix::{
    fd::{AsFd, OwnedFd},
    fs::{CWD, Mode, ResolveFlags, fstatvfs, major, minor, openat2},
};
use std::{ffi::CString, sync::Arc};

/// Each step resolves one name beneath the previous handle; mounts may be crossed.
const WALK: ResolveFlags = ResolveFlags::BENEATH
    .union(ResolveFlags::NO_SYMLINKS)
    .union(ResolveFlags::NO_MAGICLINKS);

/// Opens verified scope roots on mounted Linux file systems.
///
/// The absolute path is resolved from `/` one component at a time without
/// following symbolic links. Empty and `.` components are ignored and `..` is
/// rejected. The canonical path, identity, ancestor evidence and collection
/// handle all come from the descriptors opened by this walk.
pub struct LinuxScopeRoots;

impl ScopeRootPort for LinuxScopeRoots {
    fn open_root(&self, absolute_path: &[u8]) -> Result<OpenedScopeRoot, PortError> {
        let names = components(absolute_path)?;
        let mut current = openat2(
            CWD,
            c"/",
            DIRECTORY,
            Mode::empty(),
            ResolveFlags::NO_SYMLINKS | ResolveFlags::NO_MAGICLINKS,
        )
        .map_err(errno)?;
        let mut facts = metadata::handle(&current)?;
        let mut walked = Vec::with_capacity(names.len());
        for name in &names {
            let next = openat2(&current, name.as_c_str(), DIRECTORY, Mode::empty(), WALK)
                .map_err(errno)?;
            let next_facts = metadata::handle(&next)?;
            walked.push((
                std::mem::replace(&mut current, next),
                std::mem::replace(&mut facts, next_facts),
            ));
        }
        let inode = facts
            .inode
            .filter(|&inode| inode != 0)
            .ok_or_else(invalid)?;
        if !facts.is_directory() {
            return Err(invalid());
        }
        let volume_id: Arc<[u8]> = volume_id(&current, facts.device).into();
        let ancestors = same_mount_ancestors(walked, &facts);
        Ok(OpenedScopeRoot {
            canonical_path: canonical(&names),
            volume_id: volume_id.to_vec(),
            file_id: inode.to_le_bytes().to_vec(),
            directory: Box::new(LinuxDirectory::new(
                Anchor {
                    fd: Arc::new(current),
                    device: facts.device,
                    mount: facts.mount,
                    volume_id,
                },
                ancestors,
            )),
        })
    }
}

impl DirectoryObservationPort for LinuxScopeRoots {
    fn open(&self, absolute_path: &[u8]) -> Result<Box<dyn DirectoryObservations>, PortError> {
        Ok(self.open_root(absolute_path)?.directory)
    }
}

/// Splits an absolute path into components without resolving any of them.
fn components(path: &[u8]) -> Result<Vec<CString>, PortError> {
    let relative = path.strip_prefix(b"/").ok_or_else(invalid)?;
    relative
        .split(|&byte| byte == b'/')
        .filter(|name| !name.is_empty() && *name != b".")
        .map(|name| {
            if name == b".." {
                Err(invalid())
            } else {
                CString::new(name).map_err(|_| invalid())
            }
        })
        .collect()
}

fn canonical(names: &[CString]) -> OsPath {
    let mut raw = Vec::new();
    for name in names {
        raw.push(b'/');
        raw.extend_from_slice(name.as_bytes());
    }
    if raw.is_empty() {
        raw.push(b'/');
    }
    let display = String::from_utf8(raw.clone());
    OsPath {
        encoding: PathEncoding::Unix,
        raw: PathBytes::from_bytes(&raw),
        lossy: display.is_err(),
        display: display.unwrap_or_else(|_| String::from_utf8_lossy(&raw).into_owned()),
    }
}

/// Builds the adapter-owned volume identity from the device and file system IDs.
///
/// The device number separates volumes within a session and the file system ID
/// separates different file systems that later reuse a device number.
pub(super) fn volume_id(fd: impl AsFd, device: u64) -> Vec<u8> {
    let fsid = fstatvfs(fd).map_or(0, |value| value.f_fsid);
    format!("linux:{}:{}:{fsid:016x}", major(device), minor(device)).into_bytes()
}

/// Keeps the IDs of walked ancestors on the root's own mount.
///
/// Without mount IDs a bind mount on the same device cannot be told apart, so
/// the evidence is withheld rather than overstated.
fn same_mount_ancestors(walked: Vec<(OwnedFd, Facts)>, root: &Facts) -> Option<AncestorChain> {
    root.mount?;
    let mut handles = Vec::new();
    let mut ids = Vec::new();
    for (handle, facts) in walked.into_iter().rev() {
        if facts.device != root.device || facts.mount != root.mount {
            break;
        }
        ids.push(facts.inode?.to_le_bytes().to_vec());
        handles.push(handle);
    }
    ids.reverse();
    handles.reverse();
    Some(AncestorChain {
        _handles: handles,
        ids,
    })
}
