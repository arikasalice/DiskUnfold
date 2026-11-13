//! Command-line entry for the macOS disk image install (F15 FR9.HD1: dmg row).
//!
//! The published design registers the `diskunfold` command for a disk image install
//! through the application menu. This module is the menu action's logic: it links
//! the bundle's single executable into a folder the user can put on `PATH`. It needs no
//! administrator rights and never overwrites a file or a link that it did not create.
use std::{
    ffi::OsStr,
    fmt, fs, io,
    os::unix::fs::symlink,
    path::{Component, Path, PathBuf},
};

/// Name of the command the link provides.
pub const LINK_NAME: &str = "diskunfold";

/// Folder that receives the link when the user picks the menu item.
///
/// It lives under the user's home directory, so no elevation is needed.
pub const DEFAULT_LINK_DIRECTORY: &str = ".local/bin";

/// What a successful install did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkOutcome {
    /// A new link was created.
    Created,
    /// The link already pointed at this executable; nothing changed.
    AlreadyLinked,
    /// A link made for an earlier install location now points at this executable.
    Updated,
}

/// Why the link was not installed. No variant leaves the target folder changed.
#[derive(Debug)]
pub enum LinkError {
    /// The running executable is not inside an installed application bundle, or the
    /// bundle sits on a mounted disk image or a quarantine copy that goes away.
    NotInstalledBundle,
    /// A file or link that DiskUnfold did not create already uses the command name.
    TargetOccupied(PathBuf),
    /// The folder or link could not be created.
    Io(io::Error),
}

impl fmt::Display for LinkError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotInstalledBundle => formatter.write_str("DiskUnfold is not installed"),
            Self::TargetOccupied(path) => write!(formatter, "{} is in use", path.display()),
            Self::Io(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for LinkError {}

/// True when `executable` is `<Name>.app/Contents/MacOS/diskunfold` on a volume that
/// stays after the disk image is ejected.
fn is_installed_bundle_executable(executable: &Path) -> bool {
    let mut parts = executable.components().rev();
    let name = parts.next();
    let mac_os = parts.next();
    let contents = parts.next();
    let bundle = parts.next();
    let named = |part: Option<Component<'_>>, expected: &str| {
        part.is_some_and(|part| part.as_os_str() == OsStr::new(expected))
    };
    let bundle_is_app = bundle
        .is_some_and(|part| Path::new(part.as_os_str()).extension() == Some(OsStr::new("app")));
    let mounted_image = executable.starts_with("/Volumes");
    let translocated = executable
        .components()
        .any(|part| part.as_os_str() == OsStr::new("AppTranslocation"));
    executable.is_absolute()
        && named(name, LINK_NAME)
        && named(mac_os, "MacOS")
        && named(contents, "Contents")
        && bundle_is_app
        && !mounted_image
        && !translocated
}

/// True for a link target that an earlier DiskUnfold install could have created.
fn is_bundle_executable_shape(target: &Path) -> bool {
    target.ends_with("Contents/MacOS/diskunfold")
}

/// Links `directory/diskunfold` to the bundle `executable`.
///
/// The folder is created when missing. An existing link that points at another
/// DiskUnfold bundle executable is replaced atomically; any other existing entry
/// is left alone and reported as `TargetOccupied`.
///
/// # Errors
///
/// `NotInstalledBundle` when `executable` is not an installed bundle's executable,
/// `TargetOccupied` when the command name is taken, and `Io` for file system failures.
pub fn install_link(executable: &Path, directory: &Path) -> Result<LinkOutcome, LinkError> {
    if !is_installed_bundle_executable(executable)
        || !fs::metadata(executable).is_ok_and(|meta| meta.is_file())
    {
        return Err(LinkError::NotInstalledBundle);
    }
    fs::create_dir_all(directory).map_err(LinkError::Io)?;
    let link = directory.join(LINK_NAME);
    match fs::symlink_metadata(&link) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            symlink(executable, &link).map_err(|error| occupied_or_io(error, &link))?;
            Ok(LinkOutcome::Created)
        }
        Err(error) => Err(LinkError::Io(error)),
        Ok(meta) if meta.is_symlink() => {
            let current = fs::read_link(&link).map_err(LinkError::Io)?;
            if current == executable {
                Ok(LinkOutcome::AlreadyLinked)
            } else if is_bundle_executable_shape(&current) {
                replace_link(executable, directory, &link)?;
                Ok(LinkOutcome::Updated)
            } else {
                Err(LinkError::TargetOccupied(link))
            }
        }
        Ok(_) => Err(LinkError::TargetOccupied(link)),
    }
}

fn occupied_or_io(error: io::Error, link: &Path) -> LinkError {
    if error.kind() == io::ErrorKind::AlreadyExists {
        LinkError::TargetOccupied(link.to_path_buf())
    } else {
        LinkError::Io(error)
    }
}

/// Replaces `link` by renaming a new link over it, so the command never disappears.
fn replace_link(executable: &Path, directory: &Path, link: &Path) -> Result<(), LinkError> {
    let staging = directory.join(format!(".{LINK_NAME}.link.{}", std::process::id()));
    symlink(executable, &staging).map_err(|error| occupied_or_io(error, &staging))?;
    fs::rename(&staging, link).map_err(|error| {
        // Remove only the link created above; nothing else is touched.
        let _ = fs::remove_file(&staging);
        LinkError::Io(error)
    })
}

/// The default link folder for a home directory.
#[must_use]
pub fn default_directory(home: &Path) -> PathBuf {
    home.join(DEFAULT_LINK_DIRECTORY)
}

/// Dialog title and detail text for the menu action's result.
#[must_use]
pub fn describe(result: &Result<LinkOutcome, LinkError>, directory: &Path) -> (String, String) {
    let link = directory.join(LINK_NAME);
    match result {
        Ok(outcome) => {
            let title = match outcome {
                LinkOutcome::AlreadyLinked => "The command line tool is already installed",
                LinkOutcome::Created | LinkOutcome::Updated => "Command line tool installed",
            };
            (
                title.to_owned(),
                format!(
                    "`{}` now runs DiskUnfold from Terminal. If the shell does not find `{LINK_NAME}`, add {} to PATH, for example: export PATH=\"{}:$PATH\"",
                    link.display(),
                    directory.display(),
                    directory.display(),
                ),
            )
        }
        Err(LinkError::NotInstalledBundle) => (
            "Install DiskUnfold first".to_owned(),
            "Copy DiskUnfold.app to the Applications folder, open it from there, and choose this menu item again."
                .to_owned(),
        ),
        Err(LinkError::TargetOccupied(path)) => (
            "The command name is already in use".to_owned(),
            format!(
                "{} already exists and was not created by DiskUnfold. It was not changed.",
                path.display()
            ),
        ),
        Err(LinkError::Io(error)) => (
            "The command line tool could not be installed".to_owned(),
            format!("{}: {error}", directory.display()),
        ),
    }
}

#[cfg(test)]
mod tests;
