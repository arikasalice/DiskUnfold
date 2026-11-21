//! No-follow identity observations for macOS and Linux (F11, AFR16.platform-os.ADD1).
use du_core_contracts::{
    ByteSize, OsPath, PathEncoding, Timestamp,
    platform_os_change::{ChangeIdentity, ChangeItemObservation},
    ports::{EntryKind, PortError, PortErrorKind},
};
use std::{
    ffi::OsString, fs, os::unix::ffi::OsStringExt, os::unix::fs::MetadataExt, path::PathBuf,
    time::UNIX_EPOCH,
};

pub(crate) fn lookup_change_item(path: &OsPath) -> Result<ChangeItemObservation, PortError> {
    if path.encoding != PathEncoding::Unix {
        return Err(invalid_path());
    }
    let bytes = path.raw.bytes().map_err(|_| invalid_path())?;
    if bytes.first() != Some(&b'/') || bytes.contains(&0) {
        return Err(invalid_path());
    }
    let path = PathBuf::from(OsString::from_vec(bytes));
    let metadata = fs::symlink_metadata(path).map_err(classify_io_error)?;
    let inode = metadata.ino();
    if inode == 0 {
        return Err(PortError {
            kind: PortErrorKind::Unsupported,
            reason_key: "os.query_failed".into(),
            os_code: None,
        });
    }
    let kind = if metadata.file_type().is_symlink() {
        EntryKind::Link
    } else if metadata.is_dir() {
        EntryKind::Directory
    } else {
        EntryKind::File
    };
    Ok(ChangeItemObservation {
        identity: ChangeIdentity {
            volume_id: metadata.dev().to_le_bytes().to_vec(),
            file_id: inode.to_le_bytes().to_vec(),
            change_token: None,
        },
        kind,
        size: Some(ByteSize::from(metadata.len())),
        modified_at: modified_at(&metadata),
    })
}

fn modified_at(metadata: &fs::Metadata) -> Option<Timestamp> {
    let modified = metadata.modified().ok()?;
    let millis = match modified.duration_since(UNIX_EPOCH) {
        Ok(duration) => i128::try_from(duration.as_millis()).ok()?,
        Err(error) => -i128::try_from(error.duration().as_millis()).ok()?,
    };
    Timestamp::from_unix_millis(i64::try_from(millis).ok()?)
}

fn invalid_path() -> PortError {
    PortError {
        kind: PortErrorKind::Unsupported,
        reason_key: "path.unresolvable".into(),
        os_code: None,
    }
}

fn classify_io_error(error: std::io::Error) -> PortError {
    let os_code = error.raw_os_error();
    let (kind, reason_key) = match error.kind() {
        std::io::ErrorKind::NotFound => (PortErrorKind::NotFound, "os.query_failed"),
        std::io::ErrorKind::PermissionDenied => {
            (PortErrorKind::PermissionDenied, "os.query_failed")
        }
        std::io::ErrorKind::Unsupported => (PortErrorKind::Unsupported, "os.unsupported"),
        std::io::ErrorKind::WouldBlock => (PortErrorKind::Busy, "os.query_failed"),
        _ if os_code == Some(libc::EBUSY) => (PortErrorKind::InUse, "os.query_failed"),
        _ if os_code == Some(libc::EAGAIN) => (PortErrorKind::Busy, "os.query_failed"),
        _ if os_code.is_some_and(|code| {
            code == libc::ELOOP || code == libc::ENOTDIR || code == libc::ENAMETOOLONG
        }) =>
        {
            (PortErrorKind::Unsupported, "path.unresolvable")
        }
        _ if os_code.is_some_and(|code| code == libc::ENOTSUP || code == libc::EOPNOTSUPP) => {
            (PortErrorKind::Unsupported, "os.unsupported")
        }
        _ => (PortErrorKind::Io, "os.query_failed"),
    };
    PortError {
        kind,
        reason_key: reason_key.into(),
        os_code,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::change_ops::SystemChangeOps;
    use du_core_contracts::platform_os_change::ChangeOpsPort;
    use std::os::unix::{ffi::OsStrExt, fs::symlink};

    fn os_path(path: &std::path::Path) -> OsPath {
        let bytes = path.as_os_str().as_bytes();
        OsPath {
            encoding: PathEncoding::Unix,
            raw: du_core_contracts::PathBytes::from_bytes(bytes),
            display: String::from_utf8_lossy(bytes).into_owned(),
            lossy: std::str::from_utf8(bytes).is_err(),
        }
    }

    fn port_result<T>(result: Result<T, PortError>) -> Result<T, Box<dyn std::error::Error>> {
        result.map_err(|error| std::io::Error::other(format!("{error:?}")).into())
    }

    #[test]
    fn lookup_reads_native_identity_and_does_not_follow_a_link()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let target = directory.path().join("target.txt");
        let link = directory.path().join("link.txt");
        fs::write(&target, b"data")?;
        symlink(&target, &link)?;

        let target_observation = port_result(lookup_change_item(&os_path(&target)))?;
        let link_observation = port_result(lookup_change_item(&os_path(&link)))?;

        assert_eq!(target_observation.kind, EntryKind::File);
        assert_eq!(target_observation.size.as_ref().map(ByteSize::get), Some(4));
        assert_eq!(link_observation.kind, EntryKind::Link);
        assert_ne!(link_observation.identity, target_observation.identity);
        assert_eq!(link_observation.identity.file_id.len(), 8);
        assert_eq!(
            port_result(SystemChangeOps.query_trash_outcome(
                &os_path(&target),
                &target_observation.identity,
                None,
            ))?,
            du_core_contracts::platform_os_change::TrashOutcomeQuery::NotMoved
        );
        assert!(target.exists());
        Ok(())
    }
}
