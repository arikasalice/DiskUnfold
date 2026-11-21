//! Read-only macOS mount observations.
//!
//! Mounts are listed with `getfsstat` without asking file systems to refresh
//! their statistics. Paths are opened as `MacScopeRoots` opens scope roots, and
//! identities come from the same function, so a listed mount, an observed path
//! and a scope root on one volume report equal volume identities.
use crate::macos_observation::{errors, scope, volume};
mod group;
use du_core_contracts::{
    ports::{PortError, PortErrorKind},
    volume::{
        Capability, CapabilityState, RootVolumeGroupObservation, SystemVolumeRole,
        VolumeCapabilities, VolumeCapacity, VolumeEnumeration, VolumeGroupRole, VolumeMount,
        VolumePort, VolumeProbe, VolumeRole,
    },
};
use std::{
    ffi::{CString, c_char, c_int},
    os::fd::{AsFd, BorrowedFd, OwnedFd},
};

/// Observes mounts and directories as the current user, without changing files.
///
/// Paths are absolute native bytes; a symbolic link anywhere in a path is
/// refused with `path.unresolvable`, so pass resolved paths such as
/// `/private/tmp`. Volume identities are listed only for local mounts that are
/// not automounted, so listing never waits on a server or triggers a mount.
/// Device kind and internal or external placement are not established.
pub struct MacVolumes;

/// Attempts to read a complete list while mounts keep appearing.
const LIST_ATTEMPTS: usize = 4;
/// Spare records offered beyond the counted mounts.
const LIST_SLACK: usize = 8;

fn capability(state: CapabilityState, reason_key: Option<&'static str>) -> Capability {
    Capability { state, reason_key }
}

fn unsupported() -> Capability {
    capability(CapabilityState::Unsupported, Some("os.unsupported"))
}

fn malformed() -> PortError {
    PortError {
        kind: PortErrorKind::Io,
        reason_key: "os.query_failed".into(),
        os_code: None,
    }
}

/// Every mounted file system, from one complete `getfsstat` observation.
fn listed() -> Result<Vec<libc::statfs>, PortError> {
    for _ in 0..LIST_ATTEMPTS {
        // SAFETY: a null buffer of size zero only asks for the number of mounts.
        let counted = unsafe { libc::getfsstat(std::ptr::null_mut(), 0, libc::MNT_NOWAIT) };
        let capacity = usize::try_from(counted).map_err(|_| errors::last())? + LIST_SLACK;
        // SAFETY: `statfs` holds only integers and byte arrays, so all zero bytes
        // form a valid value.
        let mut entries = vec![unsafe { std::mem::zeroed::<libc::statfs>() }; capacity];
        let size =
            c_int::try_from(capacity * size_of::<libc::statfs>()).map_err(|_| malformed())?;
        // SAFETY: `entries` holds `capacity` writable records of `size` bytes in all.
        let written = unsafe { libc::getfsstat(entries.as_mut_ptr(), size, libc::MNT_NOWAIT) };
        let written = usize::try_from(written).map_err(|_| errors::last())?;
        // A full buffer may have left out mounts that appeared in between.
        if written < capacity {
            entries.truncate(written);
            return Ok(entries);
        }
    }
    Err(PortError {
        kind: PortErrorKind::Busy,
        reason_key: "os.query_failed".into(),
        os_code: None,
    })
}

/// Bytes of a NUL-terminated record field; a field without NUL is malformed.
fn field(raw: &[c_char]) -> Result<Vec<u8>, PortError> {
    let end = raw.iter().position(|&c| c == 0).ok_or_else(malformed)?;
    Ok(raw
        .iter()
        .take(end)
        .map(|&c| u8::from_ne_bytes(c.to_ne_bytes()))
        .collect())
}

// Only file system types with a direct, unambiguous mapping are classified.
fn role(filesystem: &[u8]) -> Option<VolumeRole> {
    (filesystem == b"devfs").then_some(VolumeRole::VirtualFs)
}

fn identity(entry: &libc::statfs, mount_path: &[u8]) -> Option<Vec<u8>> {
    let flags = entry.f_flags;
    if flags & libc::MNT_LOCAL.cast_unsigned() == 0
        || flags & libc::MNT_AUTOMOUNTED.cast_unsigned() != 0
    {
        return None;
    }
    let path = CString::new(mount_path).ok()?;
    volume::mount_identity(&path, volume::fsid(entry))
}

fn mount(entry: &libc::statfs) -> Result<VolumeMount, PortError> {
    let mount_path = field(&entry.f_mntonname)?;
    let filesystem = field(&entry.f_fstypename)?;
    let mount_source = field(&entry.f_mntfromname)?;
    if !mount_path.starts_with(b"/") || filesystem.is_empty() {
        return Err(malformed());
    }
    let flags = entry.f_flags;
    let group_role = if entry.f_flags_ext & group::MNT_EXT_ROOT_DATA_VOL != 0 {
        Some(VolumeGroupRole::Data)
    } else if mount_path == b"/" {
        Some(VolumeGroupRole::System)
    } else {
        None
    };
    let system_role = system_role(&mount_path, flags);
    let container = container(&mount_source);
    let used = used_bytes(entry, &mount_path);
    Ok(VolumeMount {
        display_path: String::from_utf8_lossy(&mount_path).into_owned(),
        volume_id: identity(entry, &mount_path),
        role: role(&filesystem),
        mount_path,
        filesystem,
        mount_source,
        topology: None,
        target_kind: None,
        // Finder lists exactly the mounts without MNT_DONTBROWSE (F01 #263).
        browsable: Some(flags & libc::MNT_DONTBROWSE.cast_unsigned() == 0),
        local: Some(flags & libc::MNT_LOCAL.cast_unsigned() != 0),
        listed_capacity: listed_capacity(entry),
        group_role,
        system_role,
        container,
        used_bytes: used,
    })
}

/// The volume's own usage when it reports one; otherwise the listing's used blocks,
/// which on APFS describe the whole container (#351).
fn used_bytes(entry: &libc::statfs, mount_path: &[u8]) -> Option<u64> {
    let own = CString::new(mount_path)
        .ok()
        .and_then(|path| volume::mount_used_bytes(&path, volume::fsid(entry)));
    own.or_else(|| {
        entry
            .f_blocks
            .checked_sub(entry.f_bfree)
            .and_then(|blocks| blocks.checked_mul(u64::from(entry.f_bsize)))
    })
}

fn system_role(path: &[u8], flags: u32) -> Option<SystemVolumeRole> {
    if path == b"/System/Volumes/VM" {
        Some(SystemVolumeRole::Vm)
    } else if path == b"/System/Volumes/Preboot" {
        Some(SystemVolumeRole::Preboot)
    } else if path == b"/System/Volumes/Recovery" {
        Some(SystemVolumeRole::Recovery)
    } else if path == b"/System/Volumes/Update" {
        Some(SystemVolumeRole::Update)
    } else if path != b"/System/Volumes/Data"
        && path.starts_with(b"/System/Volumes/")
        && flags & libc::MNT_DONTBROWSE.cast_unsigned() != 0
    {
        Some(SystemVolumeRole::Other)
    } else {
        None
    }
}

fn container(source: &[u8]) -> Option<Vec<u8>> {
    // A sealed system volume is mounted from its snapshot: `<snapshot>@/dev/diskNsM`.
    let source = source
        .iter()
        .rposition(|&byte| byte == b'@')
        .map_or(source, |at| &source[at + 1..]);
    let name = source.strip_prefix(b"/dev/")?;
    let rest = name.strip_prefix(b"disk")?;
    let separator = rest.iter().position(|&byte| byte == b's')?;
    let (number, suffix) = rest.split_at(separator);
    let slice = suffix.get(1..)?;
    if number.is_empty()
        || slice.is_empty()
        || !number.iter().all(u8::is_ascii_digit)
        || !slice.iter().all(u8::is_ascii_digit)
    {
        return None;
    }
    Some([b"disk".as_slice(), number].concat())
}

/// The capacity in the listing's own record, so a network volume needs no open (#266).
fn listed_capacity(entry: &libc::statfs) -> Option<VolumeCapacity> {
    let block = u64::from(entry.f_bsize);
    if block == 0 || entry.f_bavail > entry.f_blocks {
        return None;
    }
    Some(VolumeCapacity {
        total_bytes: entry.f_blocks.checked_mul(block)?,
        available_bytes: entry.f_bavail.checked_mul(block)?,
    })
}

/// The mounts whose records can be read, and whether any record could not be.
///
/// One unreadable record, such as a name without its terminating NUL, leaves
/// out that mount only; the listing reports itself as partial.
fn readable(entries: &[libc::statfs]) -> (Vec<VolumeMount>, bool) {
    let mut skipped = false;
    let mounts = entries
        .iter()
        .filter_map(|entry| {
            let mount = mount(entry).ok();
            skipped |= mount.is_none();
            mount
        })
        .collect();
    (mounts, skipped)
}

/// Opens a directory as scope roots are opened and checks search permission by
/// looking up `.` in it, without reading its entries.
fn open_checked(path: &[u8]) -> Result<OwnedFd, PortError> {
    let directory = scope::open_path(path)?;
    scope::open_directory(directory.as_fd(), c".")?;
    Ok(directory)
}

/// File system capacity; on APFS these are the container's values.
fn capacity_of(directory: BorrowedFd<'_>) -> Result<VolumeCapacity, PortError> {
    let stats = rustix::fs::fstatfs(directory).map_err(errors::os)?;
    let block = u64::from(stats.f_bsize);
    if block == 0 || stats.f_bavail > stats.f_blocks {
        return Err(malformed());
    }
    Ok(VolumeCapacity {
        total_bytes: stats.f_blocks.checked_mul(block).ok_or_else(malformed)?,
        available_bytes: stats.f_bavail.checked_mul(block).ok_or_else(malformed)?,
    })
}

impl VolumePort for MacVolumes {
    fn enumerate(&self) -> VolumeEnumeration {
        let (mounts, enumerate) = match listed() {
            Ok(entries) => match readable(&entries) {
                (mounts, false) => (Ok(mounts), capability(CapabilityState::Supported, None)),
                // The mounts that were read are kept; the skipped ones are counted
                // nowhere else, so the listing says it is not complete.
                (mounts, true) => (
                    Ok(mounts),
                    capability(CapabilityState::Partial, Some("os.query_failed")),
                ),
            },
            Err(error) => (
                Err(error),
                capability(CapabilityState::Unsupported, Some("os.query_failed")),
            ),
        };
        VolumeEnumeration {
            mounts,
            capabilities: VolumeCapabilities {
                enumerate,
                // Not every mount is identified, and `dev:` forms change on remount.
                identity: capability(CapabilityState::Partial, Some("os.unsupported")),
                capacity: capability(CapabilityState::Supported, None),
                path_access: capability(CapabilityState::Supported, None),
                path_compare_rule: unsupported(),
                mount_boundary: unsupported(),
                alias: unsupported(),
            },
        }
    }

    fn root_volume_group(&self) -> RootVolumeGroupObservation {
        listed()
            .map(|entries| group::observe(&entries))
            .unwrap_or(RootVolumeGroupObservation::Unconfirmed)
    }

    fn home_directory(&self) -> Option<Vec<u8>> {
        use std::os::unix::ffi::OsStrExt;
        // The user database answers, not `HOME`; a missing or non-directory home
        // is no suggestion (#263).
        let home = crate::startup::fda::passwd::home_directory()?;
        std::fs::metadata(&home)
            .is_ok_and(|meta| meta.is_dir())
            .then(|| home.as_os_str().as_bytes().to_vec())
    }

    fn path_access(&self, path: &[u8]) -> Result<(), PortError> {
        open_checked(path).map(|_| ())
    }

    fn capacity(&self, path: &[u8]) -> Result<VolumeCapacity, PortError> {
        capacity_of(open_checked(path)?.as_fd())
    }

    fn observe(
        &self,
        path: &[u8],
        expected_volume: Option<&[u8]>,
        should_read_capacity: &dyn Fn() -> bool,
    ) -> Result<VolumeProbe, PortError> {
        let directory = open_checked(path)?;
        let fd = directory.as_fd();
        let current = || volume::identity(fd);
        if let Some(expected) = expected_volume
            && current()? != expected
        {
            return Err(volume::changed());
        }
        Ok(VolumeProbe {
            capacity: should_read_capacity().then(|| match expected_volume {
                Some(expected) => volume::checked(expected, current, || capacity_of(fd)),
                None => capacity_of(fd),
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #351: a sealed system volume is mounted from its snapshot, and still shares
    /// the container of its disk.
    #[test]
    fn a_snapshot_mounted_volume_names_the_container_of_its_disk() {
        assert_eq!(
            container(b"com.apple.os.update-ABCDEF@/dev/disk3s3s1"),
            Some(b"disk3".to_vec())
        );
        assert_eq!(container(b"/dev/disk3s6"), Some(b"disk3".to_vec()));
        assert_eq!(container(b"map auto_home"), None);
    }

    /// A record with the given mount path and file system names.
    fn entry(mount_path: &[u8], filesystem: &[u8]) -> libc::statfs {
        // SAFETY: `statfs` holds only integers and byte arrays, so all zero
        // bytes form a valid value.
        let mut entry = unsafe { std::mem::zeroed::<libc::statfs>() };
        let copy = |target: &mut [c_char], bytes: &[u8]| {
            for (slot, &byte) in target.iter_mut().zip(bytes) {
                *slot = c_char::from_ne_bytes(byte.to_ne_bytes());
            }
        };
        copy(&mut entry.f_mntonname, mount_path);
        copy(&mut entry.f_fstypename, filesystem);
        copy(&mut entry.f_mntfromname, b"source");
        entry
    }

    #[test]
    fn an_unreadable_mount_is_left_out_and_the_listing_is_partial() {
        let mut unterminated = entry(b"/Volumes/broken", b"apfs");
        unterminated.f_mntonname.fill(c_char::from_ne_bytes(*b"x"));
        let entries = [
            entry(b"/", b"apfs"),
            unterminated,
            entry(b"relative", b"apfs"),
            entry(b"/Volumes/Data", b"apfs"),
        ];
        let (mounts, skipped) = readable(&entries);
        assert!(skipped);
        let paths: Vec<&[u8]> = mounts.iter().map(|m| m.mount_path.as_slice()).collect();
        assert_eq!(paths, [b"/".as_slice(), b"/Volumes/Data".as_slice()]);
    }

    #[test]
    fn readable_mounts_are_all_kept() {
        let entries = [entry(b"/", b"apfs"), entry(b"/dev", b"devfs")];
        let (mounts, skipped) = readable(&entries);
        assert!(!skipped);
        assert_eq!(mounts.len(), 2);
    }

    #[test]
    fn mount_metadata_uses_checked_space_and_extracts_container()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut entry = entry(b"/System/Volumes/VM", b"apfs");
        entry.f_mntfromname.fill(0);
        for (slot, byte) in entry.f_mntfromname.iter_mut().zip(b"/dev/disk3s1") {
            *slot = c_char::from_ne_bytes([*byte]);
        }
        entry.f_blocks = 10;
        entry.f_bfree = 4;
        entry.f_bsize = 4096;
        let mounted = match mount(&entry) {
            Ok(mounted) => mounted,
            Err(error) => return Err(format!("mount observation failed: {error:?}").into()),
        };
        assert_eq!(mounted.container.as_deref(), Some(b"disk3".as_slice()));
        assert_eq!(mounted.used_bytes, Some(24_576));
        assert_eq!(mounted.system_role, Some(SystemVolumeRole::Vm));

        entry.f_bfree = 11;
        let mounted = match mount(&entry) {
            Ok(mounted) => mounted,
            Err(error) => return Err(format!("mount observation failed: {error:?}").into()),
        };
        assert_eq!(mounted.used_bytes, None);
        Ok(())
    }
}
