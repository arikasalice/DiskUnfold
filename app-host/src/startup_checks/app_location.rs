//! Where the running application bundle is installed, for the Full Disk Access guidance.
use du_core_contracts::startup_ipc::StartupAppLocation;

/// Classifies the running executable with the same rule as the command line link (macOS).
///
/// Returns `None` on other platforms and when the executable or the home folder cannot be
/// determined, so the screen shows no location guidance instead of a wrong one.
#[cfg(target_os = "macos")]
pub(super) fn app_location() -> Option<StartupAppLocation> {
    use du_platform_os::cli_link::is_in_application_folder;
    use std::path::PathBuf;

    let executable = std::env::current_exe().ok()?;
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|home| home.is_absolute())?;
    Some(if is_in_application_folder(&executable, &home) {
        StartupAppLocation::Applications
    } else {
        StartupAppLocation::Elsewhere
    })
}

/// Classifies the running executable; only macOS reports a location.
#[cfg(not(target_os = "macos"))]
pub(super) fn app_location() -> Option<StartupAppLocation> {
    None
}
