//! Reads the WebKitGTK version this executable is linked against.
use webkit6::ffi;

/// The runtime library's `major.minor.micro` version.
///
/// The version functions return constants of the loaded library and need no
/// GTK initialization, so they are safe to call before the window exists.
pub(super) fn linked_version() -> Option<String> {
    // SAFETY: the three functions take no arguments, read no caller memory and
    // return the loaded library's version constants.
    let (major, minor, micro) = unsafe {
        (
            ffi::webkit_get_major_version(),
            ffi::webkit_get_minor_version(),
            ffi::webkit_get_micro_version(),
        )
    };
    Some(format!("{major}.{minor}.{micro}"))
}

#[cfg(test)]
mod tests {
    use super::super::{WebViewAvailability, webview_runtime::linked_runtime_availability};

    #[test]
    fn the_linked_library_reports_a_usable_version() {
        let version = super::linked_version();
        assert_eq!(
            linked_runtime_availability(version.as_deref()),
            WebViewAvailability::Available,
            "{version:?}"
        );
        let numbers: Vec<u32> = version
            .as_deref()
            .unwrap_or_default()
            .split('.')
            .filter_map(|part| part.parse().ok())
            .collect();
        // webkit6 0.6 binds WebKitGTK 6.0, which starts at 2.40.
        assert!(
            matches!(numbers.as_slice(), [major, minor, _] if (*major, *minor) >= (2, 40)),
            "{version:?}"
        );
    }
}
