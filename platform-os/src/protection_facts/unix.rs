use std::{fs, path::Path};

use du_core_contracts::protection_facts::{
    LinkBoundaryFact, ObservedFileIdentity, ObservedProtectionItem, ProtectionGuardFact,
    ProtectionOperation, VolumeRoleFact,
};

#[cfg(target_os = "linux")]
mod linux {
    use std::{
        fs::{self, File},
        os::unix::fs::MetadataExt,
        path::Path,
    };

    use du_core_contracts::protection_facts::{
        ObservedFileIdentity, ProtectionGuardFact, VolumeRoleFact,
    };
    use rustix::fs::{AtFlags, StatxFlags, fstatvfs, major, minor};

    pub(super) fn identity(path: &Path) -> Option<ObservedFileIdentity> {
        let metadata = fs::symlink_metadata(path).ok()?;
        if metadata.file_type().is_symlink() || (!metadata.is_file() && !metadata.is_dir()) {
            return None;
        }
        let file = File::open(path).ok()?;
        let fsid = fstatvfs(&file).ok()?.f_fsid;
        let device = metadata.dev();
        let volume_id =
            format!("linux:{}:{}:{fsid:016x}", major(device), minor(device)).into_bytes();
        let mount_id = mount_info(path)?.id.to_le_bytes().to_vec();
        Some(ObservedFileIdentity {
            volume_id,
            file_id: metadata.ino().to_le_bytes().to_vec(),
            volume_serial: None,
            mount_id: Some(mount_id),
        })
    }

    pub(super) fn volume_role(path: &Path) -> Option<VolumeRoleFact> {
        let mount = mount_info(path)?;
        Some(match mount.fs_type.as_slice() {
            b"tmpfs" | b"ramfs" => VolumeRoleFact::MemoryTemporary,
            b"proc" | b"sysfs" | b"cgroup" | b"cgroup2" | b"devtmpfs" => {
                VolumeRoleFact::VirtualFilesystem
            }
            b"ext2" | b"ext3" | b"ext4" | b"xfs" | b"btrfs" | b"vfat" | b"exfat" | b"ntfs" => {
                VolumeRoleFact::Ordinary
            }
            _ => return None,
        })
    }

    pub(super) fn permission(
        path: &Path,
        _operation: super::ProtectionOperation,
    ) -> Option<ProtectionGuardFact> {
        use std::os::unix::ffi::OsStrExt;
        let parent = path.parent().unwrap_or(path);
        let name = std::ffi::CString::new(parent.as_os_str().as_bytes()).ok()?;
        // SAFETY: the path is NUL-terminated; this call only checks effective access.
        let status = unsafe {
            libc::faccessat(
                libc::AT_FDCWD,
                name.as_ptr(),
                libc::W_OK | libc::X_OK,
                libc::AT_EACCESS,
            )
        };
        if status == 0 {
            return Some(ProtectionGuardFact::Clear);
        }
        match std::io::Error::last_os_error().raw_os_error()? {
            libc::EACCES | libc::EPERM => Some(ProtectionGuardFact::Protected),
            _ => None,
        }
    }

    pub(super) fn disk_backed(path: &Path) -> Option<ProtectionGuardFact> {
        match mount_info(path)?.fs_type.as_slice() {
            b"tmpfs" | b"ramfs" => Some(ProtectionGuardFact::Protected),
            b"ext2" | b"ext3" | b"ext4" | b"xfs" | b"btrfs" | b"vfat" | b"exfat" | b"ntfs" => {
                Some(ProtectionGuardFact::Clear)
            }
            _ => None,
        }
    }

    struct MountInfo {
        id: u64,
        mount_point: Vec<u8>,
        fs_type: Vec<u8>,
    }

    fn mount_info(path: &Path) -> Option<MountInfo> {
        use std::os::unix::ffi::OsStrExt;
        let path = path.canonicalize().ok()?;
        let path_bytes = path.as_os_str().as_bytes();
        let contents = fs::read("/proc/self/mountinfo").ok()?;
        contents
            .split(|byte| *byte == b'\n')
            .filter_map(parse_mount)
            .filter(|mount| path_is_within(path_bytes, &mount.mount_point))
            .max_by_key(|mount| mount.mount_point.len())
    }

    fn parse_mount(line: &[u8]) -> Option<MountInfo> {
        let fields = line.split(|byte| *byte == b' ').collect::<Vec<_>>();
        let id = std::str::from_utf8(*fields.first()?).ok()?.parse().ok()?;
        let mount_point = decode_mount_path(fields.get(4)?)?;
        let separator = fields.iter().position(|field| *field == b"-")?;
        Some(MountInfo {
            id,
            mount_point,
            fs_type: fields.get(separator + 1)?.to_vec(),
        })
    }

    fn decode_mount_path(value: &[u8]) -> Option<Vec<u8>> {
        let mut decoded = Vec::with_capacity(value.len());
        let mut index = 0;
        while index < value.len() {
            if value[index] == b'\\' {
                let code = value.get(index + 1..index + 4)?;
                let byte = match code {
                    b"040" => b' ',
                    b"011" => b'\t',
                    b"012" => b'\n',
                    b"134" => b'\\',
                    _ => return None,
                };
                decoded.push(byte);
                index += 4;
            } else {
                decoded.push(value[index]);
                index += 1;
            }
        }
        Some(decoded)
    }

    fn path_is_within(path: &[u8], root: &[u8]) -> bool {
        path == root
            || (path.starts_with(root)
                && (root.ends_with(b"/") || path.get(root.len()) == Some(&b'/')))
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use std::{ffi::CString, os::fd::AsFd, os::unix::ffi::OsStrExt, path::Path};

    use du_core_contracts::protection_facts::{ObservedFileIdentity, VolumeRoleFact};
    use rustix::fs::{Mode, OFlags, fstat, open};

    pub(super) fn identity(path: &Path) -> Option<ObservedFileIdentity> {
        let metadata = std::fs::symlink_metadata(path).ok()?;
        if metadata.file_type().is_symlink() || (!metadata.is_file() && !metadata.is_dir()) {
            return None;
        }
        let name = CString::new(path.as_os_str().as_bytes()).ok()?;
        let fd = open(
            name.as_c_str(),
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
            Mode::empty(),
        )
        .ok()?;
        let stat = fstat(&fd).ok()?;
        let volume_id = crate::macos_observation::volume::identity(fd.as_fd()).ok()?;
        Some(ObservedFileIdentity {
            volume_id,
            file_id: stat.st_ino.to_le_bytes().to_vec(),
            volume_serial: None,
            mount_id: None,
        })
    }

    pub(super) fn volume_role(path: &Path) -> Option<VolumeRoleFact> {
        let current = identity(path)?;
        let system = identity(Path::new("/System"))?;
        Some(if current.volume_id == system.volume_id {
            VolumeRoleFact::SystemPartition
        } else {
            VolumeRoleFact::Ordinary
        })
    }

    pub(super) fn permission(
        path: &Path,
        _operation: super::ProtectionOperation,
    ) -> Option<ProtectionGuardFact> {
        use std::os::unix::ffi::OsStrExt;
        let parent = path.parent().unwrap_or(path);
        let name = CString::new(parent.as_os_str().as_bytes()).ok()?;
        // SAFETY: the path is NUL-terminated; this call only checks effective access.
        let status = unsafe {
            libc::faccessat(
                libc::AT_FDCWD,
                name.as_ptr(),
                libc::W_OK | libc::X_OK,
                libc::AT_EACCESS,
            )
        };
        if status == 0 {
            return Some(ProtectionGuardFact::Clear);
        }
        match std::io::Error::last_os_error().raw_os_error()? {
            libc::EACCES | libc::EPERM => Some(ProtectionGuardFact::Protected),
            _ => None,
        }
    }

    pub(super) fn disk_backed(path: &Path) -> Option<ProtectionGuardFact> {
        use std::os::unix::ffi::OsStrExt;
        let name = CString::new(path.as_os_str().as_bytes()).ok()?;
        // SAFETY: zeroed statfs is a valid writable output buffer for this C API.
        let mut info = unsafe { std::mem::zeroed::<libc::statfs>() };
        // SAFETY: `name` is NUL-terminated and `info` points to a writable statfs.
        if unsafe { libc::statfs(name.as_ptr(), &mut info) } != 0 {
            return None;
        }
        // SAFETY: statfs fills its fixed-size, NUL-terminated filesystem type field.
        let file_system =
            unsafe { std::ffi::CStr::from_ptr(info.f_fstypename.as_ptr()) }.to_bytes();
        match file_system {
            b"apfs" | b"hfs" => Some(ProtectionGuardFact::Clear),
            _ => None,
        }
    }
}

pub(super) fn identity(path: &Path) -> Option<ObservedFileIdentity> {
    #[cfg(target_os = "linux")]
    return linux::identity(path);
    #[cfg(target_os = "macos")]
    return macos::identity(path);
}

pub(super) fn link_boundary(
    item: &ObservedProtectionItem,
    path: &Path,
) -> Option<LinkBoundaryFact> {
    let root = super::native_path(item.scan_root_path.as_ref()?)?;
    if has_link_component(path)? {
        return Some(LinkBoundaryFact::OutsideVolume);
    }
    let target = identity(path)?;
    let scan = identity(&root)?;
    #[cfg(target_os = "linux")]
    let same_mount = match (target.mount_id.as_ref(), scan.mount_id.as_ref()) {
        (Some(target_mount), Some(scan_mount)) => target_mount == scan_mount,
        _ => return None,
    };
    #[cfg(target_os = "macos")]
    let same_mount = target.volume_id == scan.volume_id;
    Some(if target.volume_id == scan.volume_id && same_mount {
        LinkBoundaryFact::WithinVolume
    } else {
        LinkBoundaryFact::OutsideVolume
    })
}

pub(super) fn volume_role(path: &Path) -> Option<VolumeRoleFact> {
    #[cfg(target_os = "linux")]
    return linux::volume_role(path);
    #[cfg(target_os = "macos")]
    return macos::volume_role(path);
}

pub(super) fn os_component(path: &Path) -> bool {
    #[cfg(target_os = "linux")]
    {
        ["/usr", "/etc", "/bin", "/sbin", "/lib", "/lib64", "/boot"]
            .iter()
            .any(|root| path.canonicalize().is_ok_and(|path| path.starts_with(root)))
    }
    #[cfg(target_os = "macos")]
    {
        ["/System", "/usr", "/bin", "/sbin"]
            .iter()
            .any(|root| path.canonicalize().is_ok_and(|path| path.starts_with(root)))
    }
}

pub(super) fn permission(
    path: &Path,
    operation: ProtectionOperation,
) -> Option<ProtectionGuardFact> {
    #[cfg(target_os = "linux")]
    return linux::permission(path, operation);
    #[cfg(target_os = "macos")]
    return macos::permission(path, operation);
}

pub(super) fn disk_backed(path: &Path) -> Option<ProtectionGuardFact> {
    #[cfg(target_os = "linux")]
    return linux::disk_backed(path);
    #[cfg(target_os = "macos")]
    {
        return macos::disk_backed(path);
    }
}

fn has_link_component(path: &Path) -> Option<bool> {
    let mut current = std::path::PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        if fs::symlink_metadata(&current)
            .ok()?
            .file_type()
            .is_symlink()
        {
            return Some(true);
        }
    }
    Some(false)
}
