//! Command-line entry for the macOS disk image install (F15 FR9.HD1: dmg row).
//!
//! The published design registers the `diskunfold` command for a disk image install
//! through the application menu. This module is the menu action's logic: it links
//! the bundle's single executable into a folder the user can put on `PATH`. It needs no
//! administrator rights and never overwrites a file or a link that it did not create.
//!
//! Only a bundle that sits in `/Applications` or `~/Applications` (after symlinks are
//! resolved) may install the link. Any other location, such as a mounted disk image, a
//! quarantine copy or a scratch folder, is refused because the link would dangle once
//! that location goes away.
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

/// System-wide folder for installed applications.
pub const SYSTEM_APPLICATIONS: &str = "/Applications";

/// Per-user application folder, relative to the home directory.
pub const USER_APPLICATIONS: &str = "Applications";

/// Language of the dialog text. Any language the build does not translate is English.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Language {
    /// English text.
    #[default]
    English,
    /// Korean text.
    Korean,
}

impl Language {
    /// Picks the dialog language from a BCP 47 tag such as `ko`, `ko-KR` or `en_US`.
    #[must_use]
    pub fn from_tag(tag: &str) -> Self {
        let primary = tag.split(['-', '_']).next().unwrap_or_default();
        if primary.eq_ignore_ascii_case("ko") {
            Self::Korean
        } else {
            Self::English
        }
    }
}

/// The folders an app may be installed in for the link: `/Applications` and `~/Applications`.
#[must_use]
pub fn application_folders(home: &Path) -> Vec<PathBuf> {
    vec![
        PathBuf::from(SYSTEM_APPLICATIONS),
        home.join(USER_APPLICATIONS),
    ]
}

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
    /// The running executable is not inside an application bundle that sits in an
    /// allowed application folder (see [`application_folders`]).
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

/// True when `executable` is `<Name>.app/Contents/MacOS/diskunfold` inside one of
/// `folders`. The caller passes a path with symlinks resolved, and each folder is
/// resolved here, so a link placed inside an allowed folder cannot lead outside it.
fn is_installed_bundle_executable(executable: &Path, folders: &[PathBuf]) -> bool {
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
    let in_application_folder = folders
        .iter()
        .filter_map(|folder| fs::canonicalize(folder).ok())
        .any(|folder| executable.starts_with(folder));
    executable.is_absolute()
        && named(name, LINK_NAME)
        && named(mac_os, "MacOS")
        && named(contents, "Contents")
        && bundle_is_app
        && in_application_folder
}

/// True when the running `executable` sits in `/Applications` or `~/Applications` of `home`.
///
/// This is the location rule for the command line link and for the Full Disk Access
/// guidance: symlinks are resolved first, so a link into an Applications folder does not count.
#[must_use]
pub fn is_in_application_folder(executable: &Path, home: &Path) -> bool {
    fs::canonicalize(executable)
        .is_ok_and(|resolved| is_installed_bundle_executable(&resolved, &application_folders(home)))
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
/// `NotInstalledBundle` when `executable` is not an installed bundle's executable
/// inside one of `folders`, `TargetOccupied` when the command name is taken, and `Io`
/// for file system failures.
pub fn install_link(
    executable: &Path,
    directory: &Path,
    folders: &[PathBuf],
) -> Result<LinkOutcome, LinkError> {
    let installed = fs::canonicalize(executable).is_ok_and(|resolved| {
        is_installed_bundle_executable(&resolved, folders)
            && fs::metadata(&resolved).is_ok_and(|meta| meta.is_file())
    });
    if !installed {
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
pub fn describe(
    result: &Result<LinkOutcome, LinkError>,
    directory: &Path,
    language: Language,
) -> (String, String) {
    let link = directory.join(LINK_NAME);
    let korean = language == Language::Korean;
    match result {
        Ok(outcome) => {
            let installed_already = *outcome == LinkOutcome::AlreadyLinked;
            let title = match (korean, installed_already) {
                (false, true) => "The command line tool is already installed",
                (false, false) => "Command line tool installed",
                (true, true) => "명령행 도구가 이미 설치되어 있습니다",
                (true, false) => "명령행 도구를 설치했습니다",
            };
            let detail = if korean {
                format!(
                    "`{}` 명령으로 터미널에서 DiskUnfold를 실행할 수 있습니다. 셸이 `{LINK_NAME}`을(를) 찾지 못하면 {}을(를) PATH에 추가하세요. 예: export PATH=\"{}:$PATH\"",
                    link.display(),
                    directory.display(),
                    directory.display(),
                )
            } else {
                format!(
                    "`{}` now runs DiskUnfold from Terminal. If the shell does not find `{LINK_NAME}`, add {} to PATH, for example: export PATH=\"{}:$PATH\"",
                    link.display(),
                    directory.display(),
                    directory.display(),
                )
            };
            (title.to_owned(), detail)
        }
        Err(LinkError::NotInstalledBundle) => {
            if korean {
                (
                    "먼저 DiskUnfold를 설치하세요".to_owned(),
                    "이 위치에서 실행 중인 DiskUnfold로는 명령행 도구를 설치할 수 없습니다. DiskUnfold.app을 응용 프로그램(Applications) 폴더로 옮기고, 그곳에서 열어 이 메뉴를 다시 선택하세요."
                        .to_owned(),
                )
            } else {
                (
                    "Move DiskUnfold to Applications first".to_owned(),
                    "The command line tool cannot be installed from where DiskUnfold is running now. Move DiskUnfold.app to the Applications folder, open it from there, and choose this menu item again."
                        .to_owned(),
                )
            }
        }
        Err(LinkError::TargetOccupied(path)) => {
            if korean {
                (
                    "이미 사용 중인 명령 이름입니다".to_owned(),
                    format!(
                        "{}이(가) 이미 있으며 DiskUnfold가 만든 것이 아니므로 바꾸지 않았습니다.",
                        path.display()
                    ),
                )
            } else {
                (
                    "The command name is already in use".to_owned(),
                    format!(
                        "{} already exists and was not created by DiskUnfold. It was not changed.",
                        path.display()
                    ),
                )
            }
        }
        Err(LinkError::Io(error)) => (
            if korean {
                "명령행 도구를 설치하지 못했습니다"
            } else {
                "The command line tool could not be installed"
            }
            .to_owned(),
            format!("{}: {error}", directory.display()),
        ),
    }
}

/// Dialog text for a home folder that could not be found.
#[must_use]
pub fn describe_missing_home(language: Language) -> (String, String) {
    match language {
        Language::English => (
            "The command line tool could not be installed".to_owned(),
            "The home folder could not be found.".to_owned(),
        ),
        Language::Korean => (
            "명령행 도구를 설치하지 못했습니다".to_owned(),
            "홈 폴더를 찾을 수 없습니다.".to_owned(),
        ),
    }
}

#[cfg(test)]
mod tests;
