//! Native handle-relative storage for snapshot files.

#[cfg(windows)]
use du_core_contracts::snapshot_file::SnapshotFileDirectory;
use du_core_contracts::snapshot_file::SnapshotFileError;
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

fn check_capture_space(
    available_bytes: Option<u64>,
    required_bytes: Option<u64>,
) -> Result<(), SnapshotFileError> {
    let required = required_bytes.unwrap_or(1);
    if available_bytes.is_some_and(|available| available < required) {
        Err(SnapshotFileError::InsufficientSpace)
    } else {
        Ok(())
    }
}

#[cfg(windows)]
mod windows;

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod unix;

#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
#[cfg(test)]
mod tests;
