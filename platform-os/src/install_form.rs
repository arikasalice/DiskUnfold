//! Install-form detection from evidence the operating system keeps.
//!
//! Only forms that leave positive evidence are reported. A portable executable
//! or an archive extraction leaves none, so those cases stay `Unknown`.
use du_core_contracts::{product_ipc::InstallForm, product_ports::InstallFormPort};
#[cfg(any(target_os = "linux", target_os = "macos", test))]
use std::path::Path;

/// Production adapter for the running executable's install form.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemInstallForm;

impl InstallFormPort for SystemInstallForm {
    fn install_form(&self) -> InstallForm {
        detect()
    }
}

/// A packaged process is an MSIX install; an unpackaged one is not classified.
#[cfg(windows)]
fn detect() -> InstallForm {
    match crate::windows_package_identity::has_package_identity() {
        Some(true) => InstallForm::Msix,
        Some(false) | None => InstallForm::Unknown,
    }
}

#[cfg(target_os = "linux")]
fn detect() -> InstallForm {
    let Ok(executable) = std::env::current_exe() else {
        return InstallForm::Unknown;
    };
    let list = std::fs::read_to_string(DPKG_FILE_LIST).ok();
    classify_linux(&executable, list.as_deref())
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

/// A Debian install needs both the packaged path and dpkg's record of owning it.
#[cfg(any(target_os = "linux", test))]
pub(crate) fn classify_linux(executable: &Path, dpkg_file_list: Option<&str>) -> InstallForm {
    let owned = dpkg_file_list.is_some_and(|list| list.lines().any(|line| line == DEB_EXECUTABLE));
    if executable == Path::new(DEB_EXECUTABLE) && owned {
        InstallForm::Deb
    } else {
        InstallForm::Unknown
    }
}

/// The DMG delivers `DiskUnfold.app`; running from that bundle layout is a DMG install.
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
        assert_eq!(classify_linux(packaged, Some(list)), InstallForm::Deb);
        assert_eq!(classify_linux(packaged, None), InstallForm::Unknown);
        assert_eq!(
            classify_linux(packaged, Some("/usr/bin\n")),
            InstallForm::Unknown
        );
        assert_eq!(
            classify_linux(Path::new("/home/u/diskunfold/diskunfold"), Some(list)),
            InstallForm::Unknown
        );
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

    #[cfg(windows)]
    #[test]
    fn an_unpackaged_windows_process_is_not_guessed() {
        assert_eq!(SystemInstallForm.install_form(), InstallForm::Unknown);
    }
}
