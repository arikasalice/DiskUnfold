//! Shared location for persistent Windows application data.

use std::path::PathBuf;

/// File name shared by GUI writers and CLI readers in the application directory.
pub const DATABASE_FILE: &str = "diskunfold.db";

/// Returns the existing desktop data override or the per-user data directory.
pub fn directory() -> Option<PathBuf> {
    std::env::var_os("DISKUNFOLD_DATA_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("LOCALAPPDATA").map(|path| PathBuf::from(path).join("DiskUnfold"))
        })
}
