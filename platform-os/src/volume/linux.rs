use du_core_contracts::{
    ports::{PortError, PortErrorKind},
    volume::{
        Capability, CapabilityState, MountTopology, VolumeCapabilities, VolumeCapacity,
        VolumeEnumeration, VolumeMount, VolumePort,
    },
};
use rustix::{
    fd::OwnedFd,
    fs::{CWD, Mode, OFlags, ResolveFlags, openat2},
    io::Errno,
};
use std::{fs, io, os::unix::ffi::OsStrExt, path::Path};

mod capacity;

/// Reads the current process mount namespace without changing mounts.
pub struct LinuxVolumes;

fn port_error(error: io::Error) -> PortError {
    let kind = match error.kind() {
        io::ErrorKind::NotFound => PortErrorKind::NotFound,
        io::ErrorKind::PermissionDenied => PortErrorKind::PermissionDenied,
        io::ErrorKind::Unsupported => PortErrorKind::Unsupported,
        _ => PortErrorKind::Io,
    };
    PortError {
        kind,
        reason_key: "os.query_failed".into(),
        os_code: error.raw_os_error(),
    }
}

fn decode(field: &[u8]) -> Result<Vec<u8>, io::Error> {
    let mut out = Vec::with_capacity(field.len());
    let mut i = 0;
    while i < field.len() {
        if field[i] == b'\\' {
            if i + 3 >= field.len()
                || !field[i + 1..i + 4]
                    .iter()
                    .all(|c| (b'0'..=b'7').contains(c))
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid mount escape",
                ));
            }
            let n = u16::from(field[i + 1] - b'0') * 64
                + u16::from(field[i + 2] - b'0') * 8
                + u16::from(field[i + 3] - b'0');
            out.push(
                u8::try_from(n).map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidData, "invalid mount escape")
                })?,
            );
            i += 4;
        } else {
            out.push(field[i]);
            i += 1;
        }
    }
    Ok(out)
}

fn numeric(field: &[u8]) -> bool {
    !field.is_empty() && field.iter().all(u8::is_ascii_digit)
}

fn topology(fields: &[&[u8]]) -> Result<MountTopology, io::Error> {
    let invalid = || io::Error::new(io::ErrorKind::InvalidData, "invalid mount topology");
    let index = fields[2]
        .iter()
        .position(|c| *c == b':')
        .ok_or_else(invalid)?;
    let (major, remainder) = fields[2].split_at(index);
    let minor = &remainder[1..];
    if !numeric(fields[0]) || !numeric(fields[1]) || !numeric(major) || !numeric(minor) {
        return Err(invalid());
    }
    let root = decode(fields[3])?;
    if !root.starts_with(b"/") || root.contains(&0) {
        return Err(invalid());
    }
    Ok(MountTopology {
        mount_id: fields[0].to_vec(),
        parent_id: fields[1].to_vec(),
        device_major: major.to_vec(),
        device_minor: minor.to_vec(),
        root,
    })
}

fn parse_mounts(data: &[u8]) -> Result<Vec<VolumeMount>, io::Error> {
    data.split(|c| *c == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| {
            let fields: Vec<&[u8]> = line.split(|c| *c == b' ').collect();
            let separator = fields
                .iter()
                .position(|f| *f == b"-")
                .filter(|i| *i >= 6 && fields.len() >= *i + 4)
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid mountinfo"))?;
            let mount_path = decode(fields[4])?;
            if !mount_path.starts_with(b"/")
                || mount_path.contains(&0)
                || fields[separator + 1].is_empty()
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid mountinfo fields",
                ));
            }
            Ok(VolumeMount {
                display_path: String::from_utf8_lossy(&mount_path).into_owned(),
                mount_path,
                filesystem: fields[separator + 1].to_vec(),
                topology: Some(topology(&fields)?),
            })
        })
        .collect()
}

fn unsupported(reason: &'static str) -> Capability {
    Capability {
        state: CapabilityState::Unsupported,
        reason_key: Some(reason),
    }
}

fn supported() -> Capability {
    Capability {
        state: CapabilityState::Supported,
        reason_key: None,
    }
}

impl VolumePort for LinuxVolumes {
    fn enumerate(&self) -> VolumeEnumeration {
        let mounts = fs::read("/proc/self/mountinfo")
            .and_then(|bytes| parse_mounts(&bytes))
            .map_err(port_error);
        let enumerate = if mounts.is_ok() {
            supported()
        } else {
            unsupported("os.query_failed")
        };
        VolumeEnumeration {
            mounts,
            capabilities: VolumeCapabilities {
                enumerate,
                identity: unsupported("os.unsupported"),
                capacity: supported(),
                path_access: supported(),
                path_compare_rule: unsupported("os.unsupported"),
                mount_boundary: unsupported("os.unsupported"),
                alias: unsupported("os.unsupported"),
            },
        }
    }

    fn path_access(&self, path: &[u8]) -> Result<(), PortError> {
        open_directory(path).map(|_| ())
    }

    fn capacity(&self, path: &[u8]) -> Result<VolumeCapacity, PortError> {
        let directory = open_directory(path)?;
        capacity::read(&directory)
    }
}

fn open_directory(path: &[u8]) -> Result<OwnedFd, PortError> {
    if !path.starts_with(b"/") || path.contains(&0) {
        return Err(PortError {
            kind: PortErrorKind::Unsupported,
            reason_key: "path.unresolvable".into(),
            os_code: None,
        });
    }
    let path = Path::new(std::ffi::OsStr::from_bytes(path));
    let directory = openat2(
        CWD,
        path,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
        ResolveFlags::NO_SYMLINKS,
    )
    .map_err(access_error)?;
    // Verify search permission on the same directory without reading entries.
    openat2(
        &directory,
        ".",
        OFlags::PATH | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
        ResolveFlags::NO_SYMLINKS,
    )
    .map_err(access_error)?;
    Ok(directory)
}

fn access_error(error: Errno) -> PortError {
    let (kind, reason) = match error {
        Errno::NOENT => (PortErrorKind::NotFound, "os.query_failed"),
        Errno::ACCESS | Errno::PERM => (PortErrorKind::PermissionDenied, "os.query_failed"),
        Errno::NOSYS | Errno::OPNOTSUPP => (PortErrorKind::Unsupported, "os.unsupported"),
        Errno::LOOP => (PortErrorKind::Unsupported, "path.unresolvable"),
        Errno::AGAIN => (PortErrorKind::Busy, "os.query_failed"),
        _ => (PortErrorKind::Io, "os.query_failed"),
    };
    PortError {
        kind,
        reason_key: reason.into(),
        os_code: Some(error.raw_os_error()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_mount_bytes_and_escapes() -> Result<(), Box<dyn std::error::Error>> {
        let mounts = parse_mounts(b"42 1 8:1 /sub\\040\xff /a\\040b\\134c rw - ext4 /dev/a rw\n43 1 0:9 / /\xff rw - tmpfs tmpfs rw\n")?;
        assert_eq!(mounts[0].mount_path, b"/a b\\c");
        let topology = mounts[0].topology.as_ref().ok_or("missing topology")?;
        assert_eq!(
            (
                &*topology.mount_id,
                &*topology.parent_id,
                &*topology.device_major,
                &*topology.device_minor,
                &*topology.root
            ),
            (
                b"42".as_slice(),
                b"1".as_slice(),
                b"8".as_slice(),
                b"1".as_slice(),
                b"/sub \xff".as_slice()
            )
        );
        assert_eq!(mounts[1].mount_path, b"/\xff");
        assert_eq!(mounts[1].display_path, "/\u{fffd}");
        Ok(())
    }
    #[test]
    fn rejects_truncated_and_invalid_records() {
        assert!(parse_mounts(b"42 1 8:1 / /bad\\999 rw - ext4 /dev/a rw\n").is_err());
        assert!(parse_mounts(b"42 1 8:1 / / rw\n").is_err());
        assert!(parse_mounts(b"42 1 x:1 / / rw - ext4 /dev/a rw\n").is_err());
        assert!(parse_mounts(b"42 1 8:1 /bad\\000 / rw - ext4 /dev/a rw\n").is_err());
    }
    #[test]
    fn reports_error_kind_and_preserves_os_code() {
        let result = port_error(io::Error::from_raw_os_error(13));
        assert_eq!(result.kind, PortErrorKind::PermissionDenied);
        assert_eq!(result.os_code, Some(13));
        assert_eq!(result.reason_key, "os.query_failed");
    }
    #[test]
    fn access_errors_preserve_category_and_code() {
        for (errno, kind) in [
            (Errno::NOSYS, PortErrorKind::Unsupported),
            (Errno::ACCESS, PortErrorKind::PermissionDenied),
            (Errno::LOOP, PortErrorKind::Unsupported),
            (Errno::AGAIN, PortErrorKind::Busy),
            (Errno::NOENT, PortErrorKind::NotFound),
        ] {
            let error = access_error(errno);
            assert_eq!(error.kind, kind);
            assert_eq!(error.os_code, Some(errno.raw_os_error()));
        }
    }
    #[test]
    fn reads_current_mount_namespace() -> Result<(), Box<dyn std::error::Error>> {
        let result = LinuxVolumes.enumerate();
        let mounts = result.mounts.map_err(|e| format!("{e:?}"))?;
        assert!(mounts.iter().any(|mount| mount.mount_path == b"/"));
        assert_eq!(
            result.capabilities.enumerate.state,
            CapabilityState::Supported
        );
        Ok(())
    }
    #[test]
    fn access_checks_directory_without_following_final_link()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::{
            ffi::OsStrExt,
            fs::{PermissionsExt, symlink},
        };
        let root =
            std::env::temp_dir().join(format!("diskunfold-volume-access-{}", std::process::id()));
        fs::create_dir(&root)?;
        let dir = root.join(std::ffi::OsStr::from_bytes(b"nonutf-\xff"));
        fs::create_dir(&dir)?;
        let link = root.join("link");
        symlink(&dir, &link)?;
        assert!(LinuxVolumes.path_access(dir.as_os_str().as_bytes()).is_ok());
        let link_error = LinuxVolumes
            .path_access(link.as_os_str().as_bytes())
            .err()
            .ok_or("link was followed")?;
        assert_eq!(link_error.kind, PortErrorKind::Unsupported);
        assert_eq!(link_error.reason_key, "path.unresolvable");
        let via_link = link.join("child");
        fs::create_dir(dir.join("child"))?;
        assert_eq!(
            LinuxVolumes
                .path_access(via_link.as_os_str().as_bytes())
                .err()
                .map(|e| e.kind),
            Some(PortErrorKind::Unsupported)
        );
        let invalid = LinuxVolumes
            .path_access(b"")
            .err()
            .ok_or("empty path accepted")?;
        assert_eq!(invalid.kind, PortErrorKind::Unsupported);
        assert_eq!(invalid.reason_key, "path.unresolvable");
        assert_eq!(
            LinuxVolumes.path_access(b"relative").err().map(|e| e.kind),
            Some(PortErrorKind::Unsupported)
        );
        assert_eq!(
            LinuxVolumes
                .path_access(b"/nul\0path")
                .err()
                .map(|e| e.kind),
            Some(PortErrorKind::Unsupported)
        );
        fs::remove_dir(dir.join("child"))?;
        let missing = LinuxVolumes.path_access(root.join("missing").as_os_str().as_bytes());
        assert_eq!(missing.err().map(|e| e.kind), Some(PortErrorKind::NotFound));
        if std::env::var_os("DU_TEST_UNPRIVILEGED").is_some() {
            fs::set_permissions(&dir, fs::Permissions::from_mode(0o400))?;
            assert_eq!(
                LinuxVolumes
                    .path_access(dir.as_os_str().as_bytes())
                    .err()
                    .map(|e| e.kind),
                Some(PortErrorKind::PermissionDenied)
            );
            fs::set_permissions(&dir, fs::Permissions::from_mode(0o100))?;
            assert_eq!(
                LinuxVolumes
                    .path_access(dir.as_os_str().as_bytes())
                    .err()
                    .map(|e| e.kind),
                Some(PortErrorKind::PermissionDenied)
            );
            fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
        }
        fs::remove_file(link)?;
        fs::remove_dir(dir)?;
        fs::remove_dir(root)?;
        Ok(())
    }
}
