use std::{fs, os::windows::fs::MetadataExt, path::Path};

use du_core_contracts::protection_facts::{
    LinkBoundaryFact, ObservedFileIdentity, ObservedProtectionItem, ProtectionGuardFact,
    ProtectionOperation, VolumeRoleFact,
};

pub(super) fn identity(path: &Path) -> Option<ObservedFileIdentity> {
    let (volume_id, file_id, serial) =
        crate::windows_observation::protection_identity(path).ok()?;
    Some(ObservedFileIdentity {
        volume_id,
        file_id,
        volume_serial: Some(serial),
        mount_id: None,
    })
}

pub(super) fn link_boundary(
    item: &ObservedProtectionItem,
    path: &Path,
) -> Option<LinkBoundaryFact> {
    let root = super::native_path(item.scan_root_path.as_ref()?)?;
    if has_reparse_component(path)? {
        return Some(LinkBoundaryFact::OutsideVolume);
    }
    let target = identity(path)?;
    let scan = identity(&root)?;
    Some(if target.volume_id == scan.volume_id {
        LinkBoundaryFact::WithinVolume
    } else {
        LinkBoundaryFact::OutsideVolume
    })
}

pub(super) fn volume_role(path: &Path) -> Option<VolumeRoleFact> {
    let current = identity(path)?;
    let system_root = std::env::var_os("SystemRoot")?;
    let system = identity(Path::new(&system_root))?;
    Some(if current.volume_id == system.volume_id {
        VolumeRoleFact::SystemPartition
    } else {
        VolumeRoleFact::Ordinary
    })
}

pub(super) fn os_component(path: &Path) -> bool {
    let Some(root) = std::env::var_os("SystemRoot").map(std::path::PathBuf::from) else {
        return false;
    };
    match (path.canonicalize(), root.canonicalize()) {
        (Ok(path), Ok(root)) => path.starts_with(root),
        _ => false,
    }
}

pub(super) fn permission(
    path: &Path,
    _operation: ProtectionOperation,
) -> Option<ProtectionGuardFact> {
    use std::os::windows::{ffi::OsStrExt, io::FromRawHandle};
    use windows::{
        Win32::Storage::FileSystem::{
            CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
            FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
            OPEN_EXISTING,
        },
        core::PCWSTR,
    };

    const DELETE_ACCESS: u32 = 0x0001_0000;
    let wide = path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    // SAFETY: `wide` is NUL-terminated and remains live for the call.
    let handle = unsafe {
        CreateFileW(
            PCWSTR(wide.as_ptr()),
            FILE_READ_ATTRIBUTES.0 | DELETE_ACCESS,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            None,
        )
    };
    match handle {
        Ok(handle) => {
            // SAFETY: the API returned an owned valid handle.
            drop(unsafe { std::fs::File::from_raw_handle(handle.0) });
            Some(ProtectionGuardFact::Clear)
        }
        Err(error) if error.code().0 as u32 & 0xffff == 5 => Some(ProtectionGuardFact::Protected),
        Err(_) => None,
    }
}

pub(super) fn disk_backed(path: &Path) -> Option<ProtectionGuardFact> {
    use std::os::windows::ffi::OsStrExt;
    use windows::{Win32::Storage::FileSystem::GetDriveTypeW, core::PCWSTR};
    let wide = path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    // SAFETY: `wide` is a live, NUL-terminated path.
    let drive_type = unsafe { GetDriveTypeW(PCWSTR(wide.as_ptr())) };
    match drive_type {
        3 | 2 | 5 => Some(ProtectionGuardFact::Clear), // fixed, removable, optical
        6 => Some(ProtectionGuardFact::Protected),     // RAM disk
        _ => None,
    }
}

fn has_reparse_component(path: &Path) -> Option<bool> {
    let mut current = std::path::PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        let metadata = fs::symlink_metadata(&current).ok()?;
        if metadata.file_attributes() & 0x400 != 0 {
            return Some(true);
        }
    }
    Some(false)
}
