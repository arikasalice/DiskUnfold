use super::access_error;
use du_core_contracts::{
    ports::{PortError, PortErrorKind},
    volume::VolumeCapacity,
};
use rustix::{fd::AsFd, fs::fstatvfs};

fn unknown() -> PortError {
    PortError {
        kind: PortErrorKind::Io,
        reason_key: "os.query_failed".into(),
        os_code: None,
    }
}

fn bytes(fragment: u64, total: u64, available: u64) -> Result<VolumeCapacity, PortError> {
    if fragment == 0 || available > total {
        return Err(unknown());
    }
    Ok(VolumeCapacity {
        total_bytes: total.checked_mul(fragment).ok_or_else(unknown)?,
        available_bytes: available.checked_mul(fragment).ok_or_else(unknown)?,
    })
}

pub(super) fn read(fd: impl AsFd) -> Result<VolumeCapacity, PortError> {
    let stat = fstatvfs(fd).map_err(access_error)?;
    bytes(stat.f_frsize, stat.f_blocks, stat.f_bavail)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::volume::LinuxVolumes;
    use du_core_contracts::volume::VolumePort;
    use std::{
        fs,
        os::unix::{
            ffi::OsStrExt,
            fs::{PermissionsExt, symlink},
        },
    };

    #[test]
    fn computes_bytes_and_refuses_missing_evidence_or_overflow() -> Result<(), PortError> {
        let value = bytes(4096, 20, 7)?;
        assert_eq!((value.total_bytes, value.available_bytes), (81920, 28672));
        assert_eq!(bytes(1, 0, 0)?.available_bytes, 0);
        for result in [bytes(0, 5, 3), bytes(2, u64::MAX, 1), bytes(1, 3, 4)] {
            let error = result.err().ok_or_else(unknown)?;
            assert_eq!(error.reason_key, "os.query_failed");
        }
        Ok(())
    }

    #[test]
    fn reads_fresh_path_and_never_reuses_an_old_observation()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = std::env::temp_dir().join(format!("du-capacity-{}", std::process::id()));
        fs::create_dir(&root)?;
        let target = root.join("target");
        fs::create_dir(&target)?;
        let path = target.as_os_str().as_bytes();
        let first = LinuxVolumes.capacity(path).map_err(|e| format!("{e:?}"))?;
        assert!(first.available_bytes <= first.total_bytes);
        fs::remove_dir(&target)?;
        assert_eq!(
            LinuxVolumes.capacity(path).err().map(|e| e.kind),
            Some(PortErrorKind::NotFound)
        );
        symlink(&root, &target)?;
        assert_eq!(
            LinuxVolumes.capacity(path).err().map(|e| e.kind),
            Some(PortErrorKind::Unsupported)
        );
        fs::remove_file(&target)?;
        fs::create_dir(&target)?;
        assert!(LinuxVolumes.capacity(path).is_ok());
        if std::env::var_os("DU_TEST_UNPRIVILEGED").is_some() {
            fs::set_permissions(&target, fs::Permissions::from_mode(0o0))?;
            assert_eq!(
                LinuxVolumes.capacity(path).err().map(|e| e.kind),
                Some(PortErrorKind::PermissionDenied)
            );
            fs::set_permissions(&target, fs::Permissions::from_mode(0o700))?;
        }
        fs::remove_dir(target)?;
        fs::remove_dir(root)?;
        Ok(())
    }
}
