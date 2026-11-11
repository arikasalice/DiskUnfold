//! Scope admission from the exact directory handle passed to the collector.
use std::{ffi::OsString, os::windows::ffi::OsStringExt, path::PathBuf};

use super::{DirectoryIdentity, WindowsDirectory, bytes, invalid, open_verified};
use du_core_contracts::{
    OsPath, PathBytes, PathEncoding,
    ports::PortError,
    scan_observation::{OpenedScopeRoot, OptionalRecentScopeObserver, ScopeRootPort},
};

#[cfg(test)]
thread_local! {
    pub(super) static OPEN_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Opens a verified scope root on a local NTFS drive.
pub struct WindowsScopeRoots;

impl ScopeRootPort for WindowsScopeRoots {
    fn open_root(&self, absolute_path: &[u8]) -> Result<OpenedScopeRoot, PortError> {
        let (directory, identity) = open_scope(absolute_path)?;
        Ok(into_scope_root(directory, identity))
    }

    fn open_root_with_recent(
        &self,
        absolute_path: &[u8],
    ) -> Result<(OpenedScopeRoot, OptionalRecentScopeObserver), PortError> {
        super::recent_probe::open_port_with_recent(absolute_path)
    }
}

pub(super) fn open_scope(
    absolute_path: &[u8],
) -> Result<(WindowsDirectory, DirectoryIdentity), PortError> {
    #[cfg(test)]
    OPEN_COUNT.with(|count| count.set(count.get() + 1));
    let (pairs, tail) = absolute_path.as_chunks::<2>();
    if !tail.is_empty() {
        return Err(invalid());
    }
    let wide: Vec<_> = pairs.iter().copied().map(u16::from_le_bytes).collect();
    if wide.contains(&0) {
        return Err(invalid());
    }
    let directory = open_verified(&PathBuf::from(OsString::from_wide(&wide)))?;
    let identity = directory.identity()?;
    Ok((directory, identity))
}

pub(super) fn into_scope_root(
    directory: WindowsDirectory,
    identity: DirectoryIdentity,
) -> OpenedScopeRoot {
    let volume_id = directory.volume_guid.to_vec();
    let physical_path = identity.physical_path;
    let display = String::from_utf16(&physical_path);
    OpenedScopeRoot {
        canonical_path: OsPath {
            encoding: PathEncoding::WindowsUtf16le,
            raw: PathBytes::from_bytes(&bytes(&physical_path)),
            lossy: display.is_err(),
            display: display.unwrap_or_else(|_| String::from_utf16_lossy(&physical_path)),
        },
        volume_id,
        file_id: identity.file_id.to_le_bytes().to_vec(),
        directory: Box::new(directory),
    }
}

pub(crate) fn volume_name(path: &[u16]) -> Result<Vec<u8>, PortError> {
    let prefix: Vec<_> = r"\\?\Volume{".encode_utf16().collect();
    let identifier = path.strip_prefix(prefix.as_slice()).ok_or_else(invalid)?;
    let close = identifier
        .iter()
        .position(|&unit| unit == u16::from(b'}'))
        .ok_or_else(invalid)?;
    if close != 36 || identifier.get(close + 1) != Some(&u16::from(b'\\')) {
        return Err(invalid());
    }
    Ok(bytes(&path[..prefix.len() + close + 1]))
}
