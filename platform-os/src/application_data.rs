//! Shared location for persistent Windows application data.

use std::path::PathBuf;

/// Returns the existing desktop data override or the per-user data directory.
pub fn directory() -> Option<PathBuf> {
    std::env::var_os("DISKUNFOLD_DATA_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("LOCALAPPDATA").map(|path| PathBuf::from(path).join("DiskUnfold"))
        })
}
