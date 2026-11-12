//! Shared location for persistent application data.

use std::path::PathBuf;

/// File name shared by GUI writers and CLI readers in the application directory.
pub const DATABASE_FILE: &str = "diskunfold.db";

/// Returns the explicit data override or this platform's per-user application directory.
pub fn directory() -> Option<PathBuf> {
    explicit_or_platform_directory(
        std::env::var_os("DISKUNFOLD_DATA_DIR").map(PathBuf::from),
        platform_directory,
    )
}

fn explicit_or_platform_directory(
    explicit: Option<PathBuf>,
    platform: impl FnOnce() -> Option<PathBuf>,
) -> Option<PathBuf> {
    match explicit {
        Some(path) if !path.as_os_str().is_empty() => Some(path),
        Some(_) => None,
        None => platform(),
    }
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

#[cfg(target_os = "macos")]
fn platform_directory() -> Option<PathBuf> {
    macos_directory(std::env::var_os("HOME").map(PathBuf::from))
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
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

#[cfg(any(target_os = "macos", test))]
fn macos_directory(home: Option<PathBuf>) -> Option<PathBuf> {
    let home = home.filter(|path| path.is_absolute())?;
    Some(home.join("Library/Application Support/DiskUnfold"))
}

#[cfg(test)]
mod tests {
    use super::explicit_or_platform_directory;
    use std::path::PathBuf;

    #[cfg(target_os = "linux")]
    use super::linux_directory;
    #[cfg(target_os = "macos")]
    use super::macos_directory;

    #[cfg(target_os = "linux")]
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

    #[cfg(target_os = "linux")]
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

    #[cfg(target_os = "linux")]
    #[test]
    fn missing_or_relative_home_fails_closed() {
        assert_eq!(linux_directory(None, None), None);
        assert_eq!(
            linux_directory(Some(PathBuf::from("")), Some(PathBuf::from("user"))),
            None
        );
    }

    #[test]
    fn explicit_data_override_is_used_and_empty_override_fails_closed() {
        let fallback = PathBuf::from("/home/user/.local/share/DiskUnfold");
        assert_eq!(
            explicit_or_platform_directory(Some(PathBuf::from("relative/test-data")), || Some(
                fallback.clone()
            ),),
            Some(PathBuf::from("relative/test-data"))
        );
        assert_eq!(
            explicit_or_platform_directory(Some(PathBuf::new()), || Some(fallback)),
            None
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_uses_the_user_application_support_directory() {
        assert_eq!(
            macos_directory(Some(PathBuf::from("/Users/example"))),
            Some(PathBuf::from(
                "/Users/example/Library/Application Support/DiskUnfold"
            ))
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_requires_an_absolute_home_directory() {
        assert_eq!(macos_directory(None), None);
        assert_eq!(macos_directory(Some(PathBuf::from("relative/home"))), None);
    }
}
