//! Native handle-relative storage for snapshot files.

use du_core_contracts::snapshot_file::{
    SnapshotFileDirectory, SnapshotFileError, SnapshotFileSystem, SnapshotFileSystemTempFile,
};
#[cfg(all(test, any(windows, target_os = "linux", target_os = "macos")))]
use std::path::Path;
use std::path::PathBuf;

/// Uses the application's private data directory for snapshot file storage.
///
/// The adapter keeps configured paths private and never includes them in debug or log output.
pub struct NativeSnapshotFileSystem {
    data_directory: PathBuf,
}

impl NativeSnapshotFileSystem {
    /// Resolves the current platform's application data directory.
    pub fn new() -> Result<Self, SnapshotFileError> {
        let data_directory =
            crate::application_data::directory().ok_or(SnapshotFileError::StoreUnavailable)?;
        Ok(Self { data_directory })
    }

    #[cfg(windows)]
    fn snapshots_path(&self) -> PathBuf {
        self.data_directory.join("snapshots")
    }

    #[cfg(all(test, any(windows, target_os = "linux", target_os = "macos")))]
    fn for_data_directory(data_directory: &Path) -> Self {
        Self {
            data_directory: data_directory.to_path_buf(),
        }
    }
}

#[cfg(windows)]
impl SnapshotFileSystem for NativeSnapshotFileSystem {
    fn open_or_create_snapshots(
        &self,
    ) -> Result<Box<dyn SnapshotFileDirectory + '_>, SnapshotFileError> {
        use std::fs;

        match fs::create_dir(self.snapshots_path()) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => return Err(SnapshotFileError::StoreUnavailable),
        }
        let directory = crate::windows_snapshot::SnapshotDirectory::open(&self.snapshots_path())
            .map_err(|_| SnapshotFileError::StoreUnavailable)?;
        let directory_path = self.snapshots_path();
        let available_bytes = windows_available_bytes(&directory_path);
        Ok(Box::new(WindowsSnapshotDirectory {
            directory,
            available_bytes,
        }))
    }
}

#[cfg(windows)]
struct WindowsSnapshotDirectory {
    directory: crate::windows_snapshot::SnapshotDirectory,
    available_bytes: Option<u64>,
}

#[cfg(windows)]
impl SnapshotFileDirectory for WindowsSnapshotDirectory {
    fn available_bytes(&self) -> Option<u64> {
        self.available_bytes
    }

    fn check_no_replace_register(&self) -> Result<(), SnapshotFileError> {
        // SnapshotDirectory::open already verifies a stable local NTFS directory.
        Ok(())
    }

    fn create_exclusive_temp(
        &self,
        snapshot_id: &du_core_contracts::Id,
    ) -> Result<Box<dyn SnapshotFileSystemTempFile>, SnapshotFileError> {
        let temporary = self
            .directory
            .create(snapshot_id)
            .map_err(|error| SnapshotFileError::WriteFailed(error.kind()))?;
        Ok(Box::new(temporary))
    }

    fn sync_folder(&self) -> std::io::Result<()> {
        self.directory.sync_folder()
    }
}

#[cfg(windows)]
fn windows_available_bytes(path: &std::path::Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows::{Win32::Storage::FileSystem::GetDiskFreeSpaceExW, core::PCWSTR};

    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    wide.push(0);
    let mut available = 0_u64;
    // SAFETY: `wide` is NUL-terminated and `available` is a valid output pointer.
    unsafe { GetDiskFreeSpaceExW(PCWSTR(wide.as_ptr()), Some(&mut available), None, None) }
        .ok()
        .map(|()| available)
}

#[cfg(windows)]
impl SnapshotFileSystemTempFile for crate::windows_snapshot::TemporarySnapshot {
    fn sync_file(&mut self) -> std::io::Result<()> {
        self.sync()
    }

    fn register(&mut self, snapshot_id: &du_core_contracts::Id) -> std::io::Result<()> {
        self.register_after_sync(snapshot_id)
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod unix {
    use super::*;
    use du_core_contracts::Id;
    use rustix::{
        fs::{
            AtFlags, FlockOperation, Mode, OFlags, RenameFlags, fchmod, flock, fsync, mkdirat,
            openat, renameat_with, unlinkat,
        },
        io::Errno,
    };
    use std::{
        fs::File,
        io::{self, Write},
        os::fd::AsRawFd,
        sync::atomic::{AtomicU64, Ordering},
    };

    const SNAPSHOTS: &str = "snapshots";
    const PRIVATE_DIR: Mode = Mode::RUSR.union(Mode::WUSR).union(Mode::XUSR);
    const PRIVATE_FILE: Mode = Mode::RUSR.union(Mode::WUSR);
    const DIRECTORY_FLAGS: OFlags = OFlags::RDONLY
        .union(OFlags::DIRECTORY)
        .union(OFlags::CLOEXEC)
        .union(OFlags::NOFOLLOW)
        .union(OFlags::NONBLOCK);
    static PROBE_COUNTER: AtomicU64 = AtomicU64::new(0);

    struct UnixSnapshotDirectory {
        directory: File,
    }

    impl SnapshotFileSystem for NativeSnapshotFileSystem {
        fn open_or_create_snapshots(
            &self,
        ) -> Result<Box<dyn SnapshotFileDirectory + '_>, SnapshotFileError> {
            let data = File::open(&self.data_directory)
                .map_err(|_| SnapshotFileError::StoreUnavailable)?;
            match mkdirat(&data, SNAPSHOTS, PRIVATE_DIR) {
                Ok(()) | Err(Errno::EXIST) => {}
                Err(_) => return Err(SnapshotFileError::StoreUnavailable),
            }
            let directory = openat(&data, SNAPSHOTS, DIRECTORY_FLAGS, Mode::empty())
                .map_err(|_| SnapshotFileError::StoreUnavailable)?;
            let directory = File::from(directory);
            fchmod(&directory, PRIVATE_DIR).map_err(|_| SnapshotFileError::StoreUnavailable)?;
            Ok(Box::new(UnixSnapshotDirectory { directory }))
        }
    }

    impl SnapshotFileDirectory for UnixSnapshotDirectory {
        fn available_bytes(&self) -> Option<u64> {
            let mut status = std::mem::MaybeUninit::<libc::statvfs>::uninit();
            // SAFETY: `directory` is an open directory descriptor and `status` is writable.
            let result = unsafe { libc::fstatvfs(self.directory.as_raw_fd(), status.as_mut_ptr()) };
            if result != 0 {
                return None;
            }
            // SAFETY: fstatvfs returned success and initialized the structure.
            let status = unsafe { status.assume_init() };
            u64::try_from(status.f_bavail)
                .ok()?
                .checked_mul(u64::try_from(status.f_frsize).ok()?)
        }

        fn check_no_replace_register(&self) -> Result<(), SnapshotFileError> {
            check_no_replace_register(&self.directory)
        }

        fn create_exclusive_temp(
            &self,
            snapshot_id: &Id,
        ) -> Result<Box<dyn SnapshotFileSystemTempFile>, SnapshotFileError> {
            let name = format!(".tmp-{}.dusnap.part", snapshot_id.as_str());
            let directory = self
                .directory
                .try_clone()
                .map_err(|error| SnapshotFileError::WriteFailed(error.kind()))?;
            let descriptor = openat(
                &self.directory,
                name.as_str(),
                OFlags::RDWR
                    .union(OFlags::CREATE)
                    .union(OFlags::EXCL)
                    .union(OFlags::CLOEXEC)
                    .union(OFlags::NOFOLLOW)
                    .union(OFlags::NONBLOCK),
                PRIVATE_FILE,
            )
            .map_err(|error| SnapshotFileError::WriteFailed(io::Error::from(error).kind()))?;
            let file = File::from(descriptor);
            let temporary = UnixSnapshotTemporary {
                directory,
                file,
                name,
                registered: false,
            };
            fchmod(&temporary.file, PRIVATE_FILE)
                .map_err(|error| SnapshotFileError::WriteFailed(io::Error::from(error).kind()))?;
            flock(&temporary.file, FlockOperation::NonBlockingLockExclusive)
                .map_err(|error| SnapshotFileError::WriteFailed(io::Error::from(error).kind()))?;
            Ok(Box::new(temporary))
        }

        fn sync_folder(&self) -> io::Result<()> {
            fsync(&self.directory).map_err(io::Error::from)
        }
    }

    struct UnixSnapshotTemporary {
        directory: File,
        file: File,
        name: String,
        registered: bool,
    }

    impl SnapshotFileSystemTempFile for UnixSnapshotTemporary {
        fn sync_file(&mut self) -> io::Result<()> {
            self.file.flush()?;
            self.file.sync_all()
        }

        fn register(&mut self, snapshot_id: &Id) -> io::Result<()> {
            let final_name = format!("{}.dusnap", snapshot_id.as_str());
            renameat_with(
                &self.directory,
                self.name.as_str(),
                &self.directory,
                final_name.as_str(),
                RenameFlags::NOREPLACE,
            )
            .map_err(io::Error::from)?;
            self.registered = true;
            Ok(())
        }
    }

    impl Write for UnixSnapshotTemporary {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.file.write(bytes)
        }

        fn flush(&mut self) -> io::Result<()> {
            self.file.flush()
        }
    }

    impl Drop for UnixSnapshotTemporary {
        fn drop(&mut self) {
            if !self.registered {
                let _ = unlinkat(&self.directory, self.name.as_str(), AtFlags::empty());
            }
        }
    }

    struct ProbeFiles<'a> {
        directory: &'a File,
        source: String,
        destination: String,
    }

    impl Drop for ProbeFiles<'_> {
        fn drop(&mut self) {
            let _ = unlinkat(self.directory, self.source.as_str(), AtFlags::empty());
            let _ = unlinkat(self.directory, self.destination.as_str(), AtFlags::empty());
        }
    }

    fn check_no_replace_register(directory: &File) -> Result<(), SnapshotFileError> {
        for _ in 0..16 {
            let suffix = format!(
                "{}-{}",
                std::process::id(),
                PROBE_COUNTER.fetch_add(1, Ordering::Relaxed)
            );
            let source = format!(".probe-source-{suffix}");
            let destination = format!(".probe-destination-{suffix}");
            let source_file = match create_probe(directory, &source) {
                Ok(file) => file,
                Err(Errno::EXIST) => continue,
                Err(error) => {
                    return Err(SnapshotFileError::WriteFailed(
                        io::Error::from(error).kind(),
                    ));
                }
            };
            let destination_file = match create_probe(directory, &destination) {
                Ok(file) => file,
                Err(Errno::EXIST) => {
                    drop(source_file);
                    let _ = unlinkat(directory, source.as_str(), AtFlags::empty());
                    continue;
                }
                Err(error) => {
                    drop(source_file);
                    let _ = unlinkat(directory, source.as_str(), AtFlags::empty());
                    return Err(SnapshotFileError::WriteFailed(
                        io::Error::from(error).kind(),
                    ));
                }
            };
            let probes = ProbeFiles {
                directory,
                source,
                destination,
            };
            let result = renameat_with(
                directory,
                probes.source.as_str(),
                directory,
                probes.destination.as_str(),
                RenameFlags::NOREPLACE,
            );
            drop((source_file, destination_file, probes));
            return match result {
                Err(Errno::EXIST) => Ok(()),
                Err(Errno::INVAL | Errno::NOSYS | Errno::NOTSUP) => {
                    Err(SnapshotFileError::Unsupported)
                }
                Err(error) => Err(SnapshotFileError::WriteFailed(
                    io::Error::from(error).kind(),
                )),
                Ok(()) => Err(SnapshotFileError::Unsupported),
            };
        }
        Err(SnapshotFileError::WriteFailed(io::ErrorKind::AlreadyExists))
    }

    fn create_probe(directory: &File, name: &str) -> Result<File, Errno> {
        let descriptor = openat(
            directory,
            name,
            OFlags::RDWR
                .union(OFlags::CREATE)
                .union(OFlags::EXCL)
                .union(OFlags::CLOEXEC)
                .union(OFlags::NOFOLLOW),
            PRIVATE_FILE,
        )?;
        Ok(File::from(descriptor))
    }
}

#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
#[cfg(test)]
mod tests {
    use super::*;
    use du_core_contracts::{Id, snapshot_file::SnapshotFileSystem};
    use std::{fs, io::Write};

    fn id(value: u32) -> Result<Id, String> {
        Id::parse(&format!("018f0000-0000-7000-8000-{value:012x}"))
    }

    fn temp_path(directory: &Path, value: &Id) -> PathBuf {
        directory.join(format!(".tmp-{}.dusnap.part", value.as_str()))
    }

    #[test]
    fn native_publish_collision_and_drop_behave_atomically()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let data = temp.path().join("data");
        fs::create_dir(&data)?;
        let filesystem = NativeSnapshotFileSystem::for_data_directory(&data);
        let snapshots = data.join("snapshots");
        let directory = filesystem.open_or_create_snapshots()?;
        directory.check_no_replace_register()?;
        assert!(fs::metadata(&snapshots)?.is_dir());
        assert_eq!(fs::read_dir(&snapshots)?.count(), 0);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&snapshots)?.permissions().mode() & 0o777,
                0o700
            );
        }

        let temporary_id = id(1)?;
        let final_id = id(2)?;
        let mut staged = directory.create_exclusive_temp(&temporary_id)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(temp_path(&snapshots, &temporary_id))?
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        staged.write_all(b"published")?;
        staged.sync_file()?;
        staged.register(&final_id)?;
        directory.sync_folder()?;
        drop(staged);
        assert!(!temp_path(&snapshots, &temporary_id).exists());
        assert_eq!(
            fs::read(snapshots.join(format!("{}.dusnap", final_id.as_str())))?,
            b"published"
        );

        let collision = id(3)?;
        let original = snapshots.join(format!("{}.dusnap", collision.as_str()));
        fs::write(&original, b"original")?;
        let collision_temp = id(4)?;
        let mut staged = directory.create_exclusive_temp(&collision_temp)?;
        staged.write_all(b"replacement")?;
        staged.sync_file()?;
        assert_eq!(
            staged.register(&collision).err().map(|error| error.kind()),
            Some(std::io::ErrorKind::AlreadyExists)
        );
        drop(staged);
        assert_eq!(fs::read(original)?, b"original");
        assert!(!temp_path(&snapshots, &collision_temp).exists());

        let abandoned = id(5)?;
        let mut staged = directory.create_exclusive_temp(&abandoned)?;
        staged.write_all(b"partial")?;
        assert!(temp_path(&snapshots, &abandoned).exists());
        drop(staged);
        assert!(!temp_path(&snapshots, &abandoned).exists());
        Ok(())
    }
}
