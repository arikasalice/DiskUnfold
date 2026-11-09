//! Read-only Windows logical-drive observations, using UTF-16LE path bytes.
use du_core_contracts::{
    ports::{PortError, PortErrorKind},
    volume::{
        Capability, CapabilityState, VolumeCapabilities, VolumeCapacity, VolumeEnumeration,
        VolumeMount, VolumePort, VolumeTargetKind,
    },
};
use std::{
    fs::OpenOptions,
    io,
    os::windows::fs::{MetadataExt, OpenOptionsExt},
};
use windows::{
    Win32::Foundation::HANDLE,
    Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_LIST_DIRECTORY, FILE_TRAVERSE, GetDiskFreeSpaceExW, GetDriveTypeW,
        GetFinalPathNameByHandleW, GetLogicalDrives, GetVolumeNameForVolumeMountPointW,
        VOLUME_NAME_GUID,
    },
    core::PCWSTR,
};

/// Observes logical drive roots as the current user, without changing files.
/// Nested paths and UNC paths are rejected until no-reparse traversal exists.
#[derive(Default)]
pub struct WindowsVolumes;

fn cap(state: CapabilityState, reason_key: Option<&'static str>) -> Capability {
    Capability { state, reason_key }
}
fn partial() -> Capability {
    cap(CapabilityState::Partial, Some("os.unsupported"))
}
fn unsupported() -> Capability {
    cap(CapabilityState::Unsupported, Some("os.unsupported"))
}
fn invalid() -> PortError {
    PortError {
        kind: PortErrorKind::Unsupported,
        reason_key: "path.unresolvable".into(),
        os_code: None,
    }
}
fn error(error: io::Error) -> PortError {
    let code = error.raw_os_error();
    let (kind, reason) = match code {
        Some(5 | 65 | 1314) => (PortErrorKind::PermissionDenied, "os.query_failed"),
        Some(2 | 3 | 15) => (PortErrorKind::NotFound, "os.query_failed"),
        Some(21 | 55 | 64 | 67 | 1167 | 1231) => (PortErrorKind::Io, "scan.disconnected"),
        _ => (PortErrorKind::Io, "os.query_failed"),
    };
    PortError {
        kind,
        reason_key: reason.into(),
        os_code: code,
    }
}
fn bytes(wide: &[u16]) -> Vec<u8> {
    wide.iter().flat_map(|u| u.to_le_bytes()).collect()
}
fn root(path: &[u8]) -> Result<Vec<u16>, PortError> {
    if path.len() != 6 {
        return Err(invalid());
    }
    let units: Vec<u16> = path
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    if !matches!(units.as_slice(), [letter, 58, 92] if (65..=90).contains(letter) || (97..=122).contains(letter))
    {
        return Err(invalid());
    }
    Ok(units)
}
fn terminated(path: &[u16]) -> Vec<u16> {
    path.iter().copied().chain([0]).collect()
}
fn identity(path: &[u16]) -> Result<Vec<u8>, PortError> {
    let name = terminated(path);
    let mut buffer = [0u16; 64];
    // SAFETY: the path is NUL-terminated and the output buffer is writable.
    unsafe { GetVolumeNameForVolumeMountPointW(PCWSTR(name.as_ptr()), &mut buffer) }
        .map_err(|e| error(io::Error::from_raw_os_error(e.code().0 & 0xffff)))?;
    let end = buffer.iter().position(|u| *u == 0).ok_or_else(invalid)?;
    if end == 0 {
        return Err(invalid());
    }
    Ok(bytes(&buffer[..end]))
}
fn enumerate() -> Result<Vec<VolumeMount>, PortError> {
    // SAFETY: no pointers or caller-owned buffers are passed.
    let mask = unsafe { GetLogicalDrives() };
    if mask == 0 {
        return Err(error(io::Error::last_os_error()));
    }
    let mut mounts = Vec::new();
    for index in 0..26 {
        if mask & (1 << index) == 0 {
            continue;
        }
        let drive = [u16::from(b'A') + index as u16, 58, 92];
        let name = terminated(&drive);
        // SAFETY: name is NUL-terminated and lives throughout the call.
        let drive_type = unsafe { GetDriveTypeW(PCWSTR(name.as_ptr())) };
        // A fixed/removable drive type does not prove whether the device is internal.
        let target_kind = (drive_type == 4).then_some(VolumeTargetKind::NetworkMount);
        let volume_id = if drive_type == 4 {
            None
        } else {
            identity(&drive).ok()
        };
        mounts.push(VolumeMount {
            mount_path: bytes(&drive),
            display_path: String::from_utf16_lossy(&drive),
            filesystem: Vec::new(),
            mount_source: Vec::new(),
            role: None,
            topology: None,
            target_kind,
            volume_id,
        });
    }
    Ok(mounts)
}
fn open_root(path: &[u8]) -> Result<(std::fs::File, Vec<u8>), PortError> {
    use std::{
        ffi::OsString,
        os::windows::{ffi::OsStringExt, io::AsRawHandle},
        path::PathBuf,
    };
    let drive = root(path)?;
    // A drive mapping without a confirmed volume GUID may be SUBST or network.
    let before = identity(&drive)?;
    let path = PathBuf::from(OsString::from_wide(&drive));
    let file = OpenOptions::new()
        .access_mode(FILE_LIST_DIRECTORY.0 | FILE_TRAVERSE.0)
        .share_mode(1 | 2 | 4)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS.0 | FILE_FLAG_OPEN_REPARSE_POINT.0)
        .open(path)
        .map_err(error)?;
    let metadata = file.metadata().map_err(error)?;
    if !metadata.is_dir() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
        return Err(invalid());
    }
    let mut final_path = [0u16; 256];
    // SAFETY: the handle remains owned by file and the buffer is writable.
    let length = unsafe {
        GetFinalPathNameByHandleW(
            HANDLE(file.as_raw_handle()),
            &mut final_path,
            VOLUME_NAME_GUID,
        )
    } as usize;
    if length == 0
        || length >= final_path.len()
        || bytes(&final_path[..length]) != before
        || identity(&drive).as_ref() != Ok(&before)
    {
        return Err(PortError {
            kind: PortErrorKind::IdentityChanged,
            reason_key: "scan.disconnected".into(),
            os_code: None,
        });
    }
    Ok((file, before))
}
impl VolumePort for WindowsVolumes {
    fn enumerate(&self) -> VolumeEnumeration {
        let mounts = enumerate();
        let enumeration = if mounts.is_ok() {
            partial()
        } else {
            cap(CapabilityState::Unsupported, Some("os.query_failed"))
        };
        VolumeEnumeration {
            mounts,
            capabilities: VolumeCapabilities {
                enumerate: enumeration,
                identity: partial(),
                capacity: partial(),
                path_access: partial(),
                path_compare_rule: unsupported(),
                mount_boundary: unsupported(),
                alias: partial(),
            },
        }
    }
    fn path_access(&self, path: &[u8]) -> Result<(), PortError> {
        open_root(path).map(|_| ())
    }
    fn capacity(&self, path: &[u8]) -> Result<VolumeCapacity, PortError> {
        let (_handle, volume) = open_root(path)?;
        let drive = root(path)?;
        let wide = terminated(
            &volume
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect::<Vec<_>>(),
        );
        let (mut available, mut total) = (0, 0);
        // SAFETY: path is NUL-terminated and output pointers are valid.
        unsafe {
            GetDiskFreeSpaceExW(
                PCWSTR(wide.as_ptr()),
                Some(&mut available),
                Some(&mut total),
                None,
            )
        }
        .map_err(|e| error(io::Error::from_raw_os_error(e.code().0 & 0xffff)))?;
        if identity(&drive).as_ref() != Ok(&volume) {
            return Err(PortError {
                kind: PortErrorKind::IdentityChanged,
                reason_key: "scan.disconnected".into(),
                os_code: None,
            });
        }
        Ok(VolumeCapacity {
            total_bytes: total,
            available_bytes: available,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn root_path_boundary_and_encoding() {
        assert_eq!(root(&bytes(&[67, 58, 92])).ok(), Some(vec![67, 58, 92]));
        for path in [
            vec![],
            vec![67],
            bytes(&[67, 58]),
            bytes(&[67, 58, 92, 97]),
            bytes(&[92, 92, 115, 92, 120]),
            bytes(&[67, 58, 0]),
        ] {
            assert!(root(&path).is_err());
        }
        assert_eq!(bytes(&[0xd800, 0xffff]), vec![0, 0xd8, 0xff, 0xff]);
    }
    #[test]
    fn maps_errors() {
        assert_eq!(
            error(io::Error::from_raw_os_error(5)).kind,
            PortErrorKind::PermissionDenied
        );
        assert_eq!(
            error(io::Error::from_raw_os_error(3)).kind,
            PortErrorKind::NotFound
        );
        assert_eq!(
            error(io::Error::from_raw_os_error(21)).reason_key,
            "scan.disconnected"
        );
        assert_eq!(
            error(io::Error::from_raw_os_error(1167)).reason_key,
            "scan.disconnected"
        );
    }
    #[test]
    fn observes_current_system_drive() -> Result<(), Box<dyn std::error::Error>> {
        use std::os::windows::ffi::OsStrExt;
        let system = std::env::var_os("SystemDrive").ok_or("missing SystemDrive")?;
        let mut wide: Vec<u16> = system.encode_wide().collect();
        wide.push(92);
        let path = bytes(&wide);
        let result = WindowsVolumes.enumerate();
        assert!(
            result
                .mounts
                .map_err(|e| format!("{e:?}"))?
                .iter()
                .any(|m| m.mount_path == path)
        );
        WindowsVolumes
            .path_access(&path)
            .map_err(|e| format!("{e:?}"))?;
        let capacity = WindowsVolumes
            .capacity(&path)
            .map_err(|e| format!("{e:?}"))?;
        assert!(capacity.total_bytes >= capacity.available_bytes);
        Ok(())
    }
}
