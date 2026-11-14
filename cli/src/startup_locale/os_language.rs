//! Reads the operating system's preferred display languages for CLI startup.

#[cfg(windows)]
pub(crate) fn preferred_ui_languages() -> Option<Vec<String>> {
    du_platform_os::windows_ui_language::preferred_ui_languages().ok()
}

#[cfg(target_os = "linux")]
pub(crate) fn preferred_ui_languages() -> Option<Vec<String>> {
    du_platform_os::linux_ui_language::preferred_ui_languages()
}

#[cfg(target_os = "macos")]
pub(crate) fn preferred_ui_languages() -> Option<Vec<String>> {
    du_platform_os::macos_ui_language::preferred_ui_languages()
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
pub(crate) fn preferred_ui_languages() -> Option<Vec<String>> {
    None
}
