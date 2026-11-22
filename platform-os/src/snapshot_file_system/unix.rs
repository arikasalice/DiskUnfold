use super::*;
use du_core_contracts::{
    Id,
    snapshot_file::{
        SnapshotFileDirectory, SnapshotFileError, SnapshotFileSystem, SnapshotFileSystemTempFile,
    },
    snapshot_spool::{CaptureSpool, SealedSpool},
};
use rustix::{
    fs::{
        AtFlags, FileType, FlockOperation, Mode, OFlags, RenameFlags, fchmod, flock, fstat, fsync,
        mkdirat, openat, renameat_with, unlinkat,
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
        let data =
            File::open(&self.data_directory).map_err(|_| SnapshotFileError::StoreUnavailable)?;
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
        // The field widths differ by target (u32 on macOS, u64 on Linux), so the
        // conversion is needed on some targets and the identity on others.
        #[allow(clippy::useless_conversion)]
        let blocks = u64::from(status.f_bavail);
        blocks.checked_mul(status.f_frsize)
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

    fn open_final(&self, snapshot_id: &Id) -> io::Result<Box<dyn io::Read + Send>> {
        let name = format!("{}.dusnap", snapshot_id.as_str());
        let descriptor = openat(
            &self.directory,
            name.as_str(),
            OFlags::RDONLY
                .union(OFlags::CLOEXEC)
                .union(OFlags::NOFOLLOW)
                .union(OFlags::NONBLOCK),
            Mode::empty(),
        )
        .map_err(io::Error::from)?;
        let file = File::from(descriptor);
        let status = fstat(&file).map_err(io::Error::from)?;
        if FileType::from_raw_mode(status.st_mode) != FileType::RegularFile {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        flock(&file, FlockOperation::NonBlockingLockShared).map_err(io::Error::from)?;
        Ok(Box::new(file))
    }

    fn create_capture(
        &self,
        required_bytes: Option<u64>,
    ) -> Result<Box<dyn CaptureSpool<Sealed = Box<dyn SealedSpool>> + Send>, SnapshotFileError>
    {
        check_capture_space(self.available_bytes(), required_bytes)?;
        let mut random = [0; 16];
        getrandom::fill(&mut random)
            .map_err(|_| SnapshotFileError::WriteFailed(io::ErrorKind::Other))?;
        let name = format!(".capture-{}.part", Id::from_v7_bytes(random).as_str());
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
        let mut cleanup = UnixCaptureName {
            directory,
            name,
            linked: true,
        };
        fchmod(&file, PRIVATE_FILE)
            .map_err(|error| SnapshotFileError::WriteFailed(io::Error::from(error).kind()))?;
        flock(&file, FlockOperation::NonBlockingLockExclusive)
            .map_err(|error| SnapshotFileError::WriteFailed(io::Error::from(error).kind()))?;
        unlinkat(&cleanup.directory, cleanup.name.as_str(), AtFlags::empty())
            .map_err(|error| SnapshotFileError::WriteFailed(io::Error::from(error).kind()))?;
        cleanup.linked = false;
        Ok(Box::new(UnixCapture { file }))
    }

    fn sync_folder(&self) -> io::Result<()> {
        fsync(&self.directory).map_err(io::Error::from)
    }
}

struct UnixCaptureName {
    directory: File,
    name: String,
    linked: bool,
}

impl Drop for UnixCaptureName {
    fn drop(&mut self) {
        if self.linked {
            let _ = unlinkat(&self.directory, self.name.as_str(), AtFlags::empty());
        }
    }
}

struct UnixCapture {
    file: File,
}

impl CaptureSpool for UnixCapture {
    type Sealed = Box<dyn SealedSpool>;

    fn seal(mut self: Box<Self>) -> io::Result<Self::Sealed> {
        self.file.flush()?;
        self.file.rewind()?;
        Ok(Box::new(UnixSealedCapture { file: self.file }))
    }
}

impl Write for UnixCapture {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.file.write(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

struct UnixSealedCapture {
    file: File,
}

impl io::Read for UnixSealedCapture {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        io::Read::read(&mut self.file, bytes)
    }
}

impl io::Seek for UnixSealedCapture {
    fn seek(&mut self, position: io::SeekFrom) -> io::Result<u64> {
        io::Seek::seek(&mut self.file, position)
    }
}

impl SealedSpool for UnixSealedCapture {}

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
            Err(Errno::INVAL | Errno::NOSYS | Errno::NOTSUP) => Err(SnapshotFileError::Unsupported),
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
