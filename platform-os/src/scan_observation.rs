//! Linux read-only directory observation, anchored to an open directory descriptor.
use du_core_contracts::{
    ports::{PortError, PortErrorKind},
    scan_observation::{
        DirectoryObservationPort, DirectoryObservations, MetadataObservation, ObservedEntry,
        ObservedKind,
    },
};
use rustix::{
    fs::{AtFlags, CWD, Dir, DirEntry, FileType, Mode, OFlags, ResolveFlags, openat2, statat},
    io::Errno,
};
use std::{os::unix::ffi::OsStrExt, path::Path};

const BATCH_SIZE: usize = 256;

pub struct LinuxDirectoryObservations;

struct Directory {
    entries: Dir,
    pending_error: Option<PortError>,
    finished: bool,
}

fn errno(error: Errno) -> PortError {
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

fn kind(file_type: FileType) -> ObservedKind {
    match file_type {
        FileType::RegularFile => ObservedKind::File,
        FileType::Directory => ObservedKind::Directory,
        FileType::Symlink => ObservedKind::Link,
        FileType::Unknown => ObservedKind::Unknown,
        _ => ObservedKind::Other,
    }
}

fn observe(dir: &Dir, entry: DirEntry) -> ObservedEntry {
    let enumerated_kind = kind(entry.file_type());
    let metadata = dir
        .fd()
        .and_then(|fd| statat(fd, entry.file_name(), AtFlags::SYMLINK_NOFOLLOW))
        .map(|m| MetadataObservation {
            kind: kind(FileType::from_raw_mode(m.st_mode)),
            device: m.st_dev,
            inode: m.st_ino,
            size: u64::try_from(m.st_size).map_err(|_| PortError {
                kind: PortErrorKind::Unsupported,
                reason_key: "os.query_failed".into(),
                os_code: None,
            }),
            allocated: None,
            link_count: None,
            space_identity: None,
            modified_at: None,
            native_modified_at: None,
        })
        .map_err(errno);
    ObservedEntry {
        name: entry.file_name().to_bytes().to_vec(),
        enumerated_inode: entry.ino(),
        enumerated_kind,
        metadata,
    }
}

impl DirectoryObservationPort for LinuxDirectoryObservations {
    fn open(&self, absolute_path: &[u8]) -> Result<Box<dyn DirectoryObservations>, PortError> {
        if !absolute_path.starts_with(b"/") || absolute_path.contains(&0) {
            return Err(PortError {
                kind: PortErrorKind::Unsupported,
                reason_key: "path.unresolvable".into(),
                os_code: None,
            });
        }
        let path = Path::new(std::ffi::OsStr::from_bytes(absolute_path));
        let fd = openat2(
            CWD,
            path,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
            Mode::empty(),
            ResolveFlags::NO_SYMLINKS,
        )
        .map_err(errno)?;
        let entries = Dir::new(fd).map_err(errno)?;
        Ok(Box::new(Directory {
            entries,
            pending_error: None,
            finished: false,
        }))
    }
}

impl DirectoryObservations for Directory {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        if let Some(error) = self.pending_error.take() {
            return Err(error);
        }
        if self.finished {
            return Ok(Vec::new());
        }
        let mut batch = Vec::with_capacity(BATCH_SIZE);
        while batch.len() < BATCH_SIZE {
            let Some(entry) = self.entries.next() else {
                self.finished = true;
                break;
            };
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    self.finished = true;
                    if batch.is_empty() {
                        return Err(errno(error));
                    }
                    self.pending_error = Some(errno(error));
                    break;
                }
            };
            if entry.file_name().to_bytes() == b"." || entry.file_name().to_bytes() == b".." {
                continue;
            }
            batch.push(observe(&self.entries, entry));
        }
        Ok(batch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        os::unix::{
            ffi::OsStrExt,
            fs::{PermissionsExt, symlink},
        },
        path::PathBuf,
    };

    fn fixture(label: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
        let path = std::env::temp_dir().join(format!("du-observe-{label}-{}", std::process::id()));
        fs::create_dir(&path)?;
        Ok(path)
    }

    #[test]
    fn streams_raw_names_and_never_follows_links() -> Result<(), Box<dyn std::error::Error>> {
        let root = fixture("stream")?;
        for n in 0..300 {
            fs::write(root.join(format!("f{n}")), b"abc")?;
        }
        fs::write(root.join(std::ffi::OsStr::from_bytes(b"raw-\xff")), b"x")?;
        symlink("f0", root.join("link"))?;
        let mut reader = LinuxDirectoryObservations
            .open(root.as_os_str().as_bytes())
            .map_err(|e| format!("{e:?}"))?;
        let first = reader.next_batch().map_err(|e| format!("{e:?}"))?;
        assert_eq!(first.len(), BATCH_SIZE);
        let second = reader.next_batch().map_err(|e| format!("{e:?}"))?;
        assert_eq!(second.len(), 46);
        let entries: Vec<_> = first.into_iter().chain(second).collect();
        assert!(entries.iter().any(|e| e.name == b"raw-\xff"));
        let link = entries
            .iter()
            .find(|e| e.name == b"link")
            .ok_or("link missing")?;
        assert_eq!(
            link.metadata.as_ref().map(|m| &m.kind),
            Ok(&ObservedKind::Link)
        );
        assert!(
            reader
                .next_batch()
                .map_err(|e| format!("{e:?}"))?
                .is_empty()
        );
        assert!(
            LinuxDirectoryObservations
                .open(root.join("link").as_os_str().as_bytes())
                .is_err()
        );
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn records_disappearance_and_replacement() -> Result<(), Box<dyn std::error::Error>> {
        let root = fixture("race")?;
        fs::write(root.join("gone"), b"old")?;
        let fd = openat2(
            CWD,
            &root,
            OFlags::RDONLY | OFlags::DIRECTORY,
            Mode::empty(),
            ResolveFlags::NO_SYMLINKS,
        )?;
        let mut dir = Dir::new(fd)?;
        let entry = dir
            .by_ref()
            .find_map(|e| e.ok().filter(|e| e.file_name().to_bytes() == b"gone"))
            .ok_or("missing entry")?;
        fs::remove_file(root.join("gone"))?;
        assert_eq!(
            observe(&dir, entry).metadata.err().map(|e| e.kind),
            Some(PortErrorKind::NotFound)
        );
        fs::write(root.join("old"), b"old")?;
        dir.rewind();
        let entry = dir
            .by_ref()
            .find_map(|e| e.ok().filter(|e| e.file_name().to_bytes() == b"old"))
            .ok_or("missing entry")?;
        fs::rename(root.join("old"), root.join("saved"))?;
        symlink("saved", root.join("old"))?;
        let seen = observe(&dir, entry);
        let metadata = seen.metadata.map_err(|e| format!("{e:?}"))?;
        assert_ne!(seen.enumerated_inode, metadata.inode);
        assert_eq!(metadata.kind, ObservedKind::Link);
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn permission_error_is_not_empty_directory() -> Result<(), Box<dyn std::error::Error>> {
        if std::env::var_os("DU_TEST_UNPRIVILEGED").is_none() {
            return Ok(());
        }
        let root = fixture("permission")?;
        fs::set_permissions(&root, fs::Permissions::from_mode(0o000))?;
        let result = LinuxDirectoryObservations.open(root.as_os_str().as_bytes());
        assert_eq!(
            result.err().map(|e| e.kind),
            Some(PortErrorKind::PermissionDenied)
        );
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;
        fs::remove_dir(root)?;
        Ok(())
    }
}
