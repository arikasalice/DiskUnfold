//! Shared location for persistent application data.

use std::path::PathBuf;

/// File name shared by GUI writers and CLI readers in the application directory.
pub const DATABASE_FILE: &str = "diskunfold.db";

/// Returns the explicit data override or this platform's per-user application directory.
pub fn directory() -> Option<PathBuf> {
    std::env::var_os("DISKUNFOLD_DATA_DIR")
        .map(PathBuf::from)
        .or_else(platform_directory)
}

#[cfg(windows)]
fn platform_directory() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|path| PathBuf::from(path).join("DiskUnfold"))
}

#[cfg(target_os = "linux")]
fn platform_directory() -> Option<PathBuf> {
    linux_directory(
        std::env::var_os("XDG_DATA_HOME").map(PathBuf::from),
        std::env::var_os("HOME").map(PathBuf::from),
    )
}

#[cfg(not(any(windows, target_os = "linux")))]
fn platform_directory() -> Option<PathBuf> {
    None
}

#[cfg(target_os = "linux")]
fn linux_directory(xdg_data_home: Option<PathBuf>, home: Option<PathBuf>) -> Option<PathBuf> {
    let base = xdg_data_home
        .filter(|path| path.is_absolute())
        .or_else(|| {
            home.filter(|path| path.is_absolute())
                .map(|path| path.join(".local/share"))
        })?;
    Some(base.join("DiskUnfold"))
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::linux_directory;
    use std::path::PathBuf;

    #[test]
    fn absolute_xdg_data_home_selects_the_application_subdirectory() {
        assert_eq!(
            linux_directory(
                Some(PathBuf::from("/srv/user-data")),
                Some(PathBuf::from("/home/user")),
            ),
            Some(PathBuf::from("/srv/user-data/DiskUnfold"))
        );
    }

    #[test]
    fn invalid_xdg_data_home_falls_back_to_absolute_home() {
        assert_eq!(
            linux_directory(
                Some(PathBuf::from("relative/data")),
                Some(PathBuf::from("/home/user")),
            ),
            Some(PathBuf::from("/home/user/.local/share/DiskUnfold"))
        );
    }

    #[test]
    fn missing_or_relative_home_fails_closed() {
        assert_eq!(linux_directory(None, None), None);
        assert_eq!(
            linux_directory(Some(PathBuf::from("")), Some(PathBuf::from("user"))),
            None
        );
    }
}
