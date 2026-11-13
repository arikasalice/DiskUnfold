use sha2::{Digest, Sha256};
use std::{
    env, fs,
    fs::{File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT_DIRECTORY_ID: AtomicU64 = AtomicU64::new(0);

/// Private snapshot passed to an external verifier so it sees the exact bytes hashed here.
pub(super) struct StagedFile {
    path: PathBuf,
    sha256: String,
    _handle: File,
    _directory: PrivateDirectory,
}

impl StagedFile {
    pub(super) fn from_path(path: &Path, file_name: &str, executable: bool) -> io::Result<Self> {
        let mut source = File::open(path)?;
        let directory = PrivateDirectory::new()?;
        let staged_path = directory.0.join(file_name);
        let mut staged = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staged_path)?;
        let mut buffer = [0; 64 * 1024];
        loop {
            let read = source.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            staged.write_all(&buffer[..read])?;
        }
        staged.sync_all()?;
        drop(staged);
        set_private_file_permissions(&staged_path, executable)?;
        let mut handle = open_locked_read(&staged_path)?;
        let sha256 = hash_open_file(&mut handle)?;

        Ok(Self {
            path: staged_path,
            sha256,
            _handle: handle,
            _directory: directory,
        })
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    pub(super) fn sha256(&self) -> &str {
        &self.sha256
    }
}

struct PrivateDirectory(PathBuf);

impl PrivateDirectory {
    fn new() -> io::Result<Self> {
        let base = fs::canonicalize(env::temp_dir())?;
        for _ in 0..64 {
            let id = NEXT_DIRECTORY_ID.fetch_add(1, Ordering::Relaxed);
            let path = base.join(format!("diskunfold-provenance-{}-{id}", process::id()));
            let mut builder = fs::DirBuilder::new();
            set_private_directory_mode(&mut builder);
            match builder.create(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not create a unique private verification directory",
        ))
    }
}

impl Drop for PrivateDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_file(self.0.join("artifact"));
        let _ = fs::remove_file(self.0.join("gh.exe"));
        let _ = fs::remove_file(self.0.join("gh"));
        let _ = fs::remove_dir(&self.0);
    }
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

fn hash_open_file(file: &mut File) -> io::Result<String> {
    use std::io::{Seek, SeekFrom};

    file.seek(SeekFrom::Start(0))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(encode_hex(&hasher.finalize()))
}

#[cfg(unix)]
fn set_private_directory_mode(builder: &mut fs::DirBuilder) {
    use std::os::unix::fs::DirBuilderExt;
    builder.mode(0o700);
}

#[cfg(not(unix))]
fn set_private_directory_mode(_builder: &mut fs::DirBuilder) {}

#[cfg(unix)]
fn set_private_file_permissions(path: &Path, executable: bool) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mode = if executable { 0o500 } else { 0o400 };
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
fn set_private_file_permissions(_path: &Path, _executable: bool) -> io::Result<()> {
    Ok(())
}

#[cfg(windows)]
fn open_locked_read(path: &Path) -> io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_SHARE_READ: u32 = 0x0000_0001;
    let mut options = OpenOptions::new();
    options.read(true).share_mode(FILE_SHARE_READ);
    options.open(path)
}

#[cfg(not(windows))]
fn open_locked_read(path: &Path) -> io::Result<File> {
    File::open(path)
}

#[cfg(test)]
mod tests {
    use super::StagedFile;
    use std::{
        fs,
        io::{Seek, SeekFrom, Write},
    };
    use tempfile::NamedTempFile;

    #[test]
    fn staged_snapshot_is_unchanged_when_source_bytes_change()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut source = NamedTempFile::new()?;
        source.write_all(b"abc")?;
        source.flush()?;
        let staged = StagedFile::from_path(source.path(), "artifact", false)?;

        source.as_file_mut().set_len(0)?;
        source.as_file_mut().seek(SeekFrom::Start(0))?;
        source.write_all(b"def")?;
        source.flush()?;

        assert_eq!(fs::read(staged.path())?, b"abc");
        assert_eq!(
            staged.sha256(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(fs::read(source.path())?, b"def");
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn staged_executable_retains_execute_permission() -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::fs::PermissionsExt;

        let source = NamedTempFile::new()?;
        let staged = StagedFile::from_path(source.path(), "gh", true)?;
        let mode = fs::metadata(staged.path())?.permissions().mode();
        assert_ne!(mode & 0o111, 0);
        assert_eq!(mode & 0o222, 0);
        Ok(())
    }
}
