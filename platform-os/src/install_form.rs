//! Install-form detection from evidence the operating system keeps.
//!
//! Only forms that leave positive evidence are reported. Debian and MSIX leave
//! OS records. The tar.gz, portable and MSI packages ship a one-line `install-form`
//! marker next to the executable; it is read without following symbolic links,
//! capped at [`MARKER_LIMIT`] bytes, and any problem yields `Unknown`. The MSI
//! installs the marker with the executable, so uninstalling removes both.
use du_core_contracts::{product_ipc::InstallForm, product_ports::InstallFormPort};
#[cfg(any(target_os = "linux", target_os = "macos", windows, test))]
use std::path::Path;

/// Production adapter for the running executable's install form.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemInstallForm;

impl InstallFormPort for SystemInstallForm {
    fn install_form(&self) -> InstallForm {
        detect()
    }
}

/// A packaged process is an MSIX install; an unpackaged one is portable or an
/// MSI install only when the marker beside the executable says so.
#[cfg(windows)]
fn detect() -> InstallForm {
    let identity = crate::windows_package_identity::has_package_identity();
    let marker = std::env::current_exe()
        .ok()
        .and_then(|executable| read_marker(executable.parent()?));
    classify_windows(identity, marker.as_deref())
}

#[cfg(target_os = "linux")]
fn detect() -> InstallForm {
    let Ok(executable) = std::env::current_exe() else {
        return InstallForm::Unknown;
    };
    let list = std::fs::read_to_string(DPKG_FILE_LIST).ok();
    let marker = executable.parent().and_then(read_marker);
    classify_linux(&executable, list.as_deref(), marker.as_deref())
}

#[cfg(target_os = "macos")]
fn detect() -> InstallForm {
    std::env::current_exe()
        .map(|executable| classify_macos(&executable))
        .unwrap_or(InstallForm::Unknown)
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
fn detect() -> InstallForm {
    InstallForm::Unknown
}

#[cfg(target_os = "linux")]
const DPKG_FILE_LIST: &str = "/var/lib/dpkg/info/diskunfold.list";
#[cfg(any(target_os = "linux", test))]
const DEB_EXECUTABLE: &str = "/usr/bin/diskunfold";
#[cfg(any(target_os = "linux", test))]
const DEB_DIRECTORY: &str = "/usr/bin";
#[cfg(any(target_os = "linux", windows, test))]
const MARKER_NAME: &str = "install-form";
/// Largest marker prefix that is read.
#[cfg(any(target_os = "linux", windows, test))]
const MARKER_LIMIT: u64 = 64;

/// Reads the trimmed marker in `directory`, or `None` if it is absent, not a
/// regular file, a symbolic link, unreadable or not UTF-8 within the cap.
#[cfg(any(target_os = "linux", windows, test))]
pub(crate) fn read_marker(directory: &Path) -> Option<String> {
    use std::io::Read;
    let path = directory.join(MARKER_NAME);
    if !std::fs::symlink_metadata(&path).ok()?.file_type().is_file() {
        return None;
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32);
    }
    let file = options.open(&path).ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(MARKER_LIMIT).read_to_end(&mut bytes).ok()?;
    Some(String::from_utf8(bytes).ok()?.trim().to_owned())
}

/// A packaged process is MSIX; otherwise the marker `portable` selects the portable
/// form and the marker `msi` the Windows Installer form.
#[cfg(any(windows, test))]
pub(crate) fn classify_windows(
    package_identity: Option<bool>,
    marker: Option<&str>,
) -> InstallForm {
    match (package_identity, marker) {
        (Some(true), _) => InstallForm::Msix,
        (Some(false), Some("portable")) => InstallForm::Portable,
        (Some(false), Some("msi")) => InstallForm::Msi,
        _ => InstallForm::Unknown,
    }
}

/// A Debian install needs both the packaged path and dpkg's record of owning it.
/// A tar.gz extraction outside `/usr/bin` is recognized by its marker.
#[cfg(any(target_os = "linux", test))]
pub(crate) fn classify_linux(
    executable: &Path,
    dpkg_file_list: Option<&str>,
    marker: Option<&str>,
) -> InstallForm {
    let owned = dpkg_file_list.is_some_and(|list| list.lines().any(|line| line == DEB_EXECUTABLE));
    if executable == Path::new(DEB_EXECUTABLE) && owned {
        InstallForm::Deb
    } else if !executable.starts_with(DEB_DIRECTORY) && marker == Some("tar_gz") {
        InstallForm::TarGz
    } else {
        InstallForm::Unknown
    }
}

/// The DMG delivers `DiskUnfold.app`; running from that bundle layout is a DMG install.
///
/// This is a heuristic on the executable's directory layout. A copied or
/// extracted `DiskUnfold.app` is classified as a DMG install as well, and an app
/// moved out of the bundle layout is `Unknown`; no installer record is consulted.
#[cfg(any(target_os = "macos", test))]
pub(crate) fn classify_macos(executable: &Path) -> InstallForm {
    let in_bundle = executable
        .parent()
        .is_some_and(|directory| directory.ends_with("DiskUnfold.app/Contents/MacOS"));
    if in_bundle {
        InstallForm::Dmg
    } else {
        InstallForm::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debian_needs_the_packaged_path_and_dpkg_ownership() {
        let list = "/usr\n/usr/bin\n/usr/bin/diskunfold\n";
        let packaged = Path::new("/usr/bin/diskunfold");
        assert_eq!(classify_linux(packaged, Some(list), None), InstallForm::Deb);
        assert_eq!(classify_linux(packaged, None, None), InstallForm::Unknown);
        assert_eq!(
            classify_linux(packaged, Some("/usr/bin\n"), None),
            InstallForm::Unknown
        );
        assert_eq!(
            classify_linux(Path::new("/home/u/diskunfold/diskunfold"), Some(list), None),
            InstallForm::Unknown
        );
    }

    #[test]
    fn tar_gz_needs_the_marker_and_a_location_outside_usr_bin() {
        let home = Path::new("/home/u/diskunfold/diskunfold");
        assert_eq!(
            classify_linux(home, None, Some("tar_gz")),
            InstallForm::TarGz
        );
        assert_eq!(
            classify_linux(home, None, Some("portable")),
            InstallForm::Unknown
        );
        assert_eq!(classify_linux(home, None, None), InstallForm::Unknown);
        assert_eq!(
            classify_linux(Path::new("/usr/bin/diskunfold"), None, Some("tar_gz")),
            InstallForm::Unknown
        );
    }

    #[test]
    fn windows_marker_is_portable_unless_the_process_is_packaged() {
        assert_eq!(
            classify_windows(Some(false), Some("portable")),
            InstallForm::Portable
        );
        assert_eq!(classify_windows(Some(false), Some("msi")), InstallForm::Msi);
        assert_eq!(classify_windows(Some(true), Some("msi")), InstallForm::Msix);
        assert_eq!(classify_windows(None, Some("msi")), InstallForm::Unknown);
        assert_eq!(
            classify_windows(Some(true), Some("portable")),
            InstallForm::Msix
        );
        assert_eq!(
            classify_windows(Some(false), Some("tar_gz")),
            InstallForm::Unknown
        );
        assert_eq!(
            classify_windows(None, Some("portable")),
            InstallForm::Unknown
        );
        assert_eq!(classify_windows(Some(false), None), InstallForm::Unknown);
    }

    #[test]
    fn marker_reader_trims_regular_files_and_rejects_everything_else() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        assert_eq!(read_marker(dir.path()), None);
        let marker = dir.path().join(MARKER_NAME);
        std::fs::write(&marker, "tar_gz\n")?;
        assert_eq!(read_marker(dir.path()).as_deref(), Some("tar_gz"));
        std::fs::write(&marker, "tar_gz\n".repeat(20))?;
        assert_ne!(read_marker(dir.path()).as_deref(), Some("tar_gz"));
        std::fs::write(&marker, [0xff, 0xfe])?;
        assert_eq!(read_marker(dir.path()), None);
        std::fs::remove_file(&marker)?;
        std::fs::create_dir(&marker)?;
        assert_eq!(read_marker(dir.path()), None);
        Ok(())
    }

    #[test]
    fn marker_read_is_capped_at_the_limit() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        let content = format!("tar_gz{}", " ".repeat(200));
        std::fs::write(dir.path().join(MARKER_NAME), content)?;
        assert_eq!(read_marker(dir.path()).as_deref(), Some("tar_gz"));
        std::fs::write(
            dir.path().join(MARKER_NAME),
            format!("{}tar_gz", " ".repeat(64)),
        )?;
        assert_eq!(read_marker(dir.path()).as_deref(), Some(""));
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn marker_symlink_is_not_followed() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        let target = dir.path().join("elsewhere");
        std::fs::write(&target, "tar_gz\n")?;
        std::os::unix::fs::symlink(&target, dir.path().join(MARKER_NAME))?;
        assert_eq!(read_marker(dir.path()), None);
        Ok(())
    }

    #[test]
    fn mac_bundle_layout_is_a_dmg_install_and_other_paths_are_unknown() {
        assert_eq!(
            classify_macos(Path::new(
                "/Applications/DiskUnfold.app/Contents/MacOS/diskunfold"
            )),
            InstallForm::Dmg
        );
        assert_eq!(
            classify_macos(Path::new("/Users/u/bin/diskunfold")),
            InstallForm::Unknown
        );
        assert_eq!(
            classify_macos(Path::new(
                "/Applications/Other.app/Contents/MacOS/diskunfold"
            )),
            InstallForm::Unknown
        );
    }
}
