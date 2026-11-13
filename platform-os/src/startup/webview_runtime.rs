//! Classifies webview runtime evidence into a startup availability value.
//!
//! The classification is pure so every combination of observations is testable
//! on any host; the platform adapters supply the observations.
use super::WebViewAvailability;

/// What one registry probe of a WebView2 runtime version value established.
#[cfg(any(target_os = "windows", test))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum RuntimeProbe {
    /// The value exists; its text is the runtime version.
    Version(String),
    /// The key or value is definitely absent.
    Missing,
    /// The probe failed for another reason, so absence is not established.
    Unreadable,
}

/// Splits dotted decimal text into its numbers; anything else is not a version.
fn version_numbers(version: &str) -> Option<Vec<&str>> {
    let parts: Vec<&str> = version.trim().split('.').collect();
    let well_formed = parts.len() >= 2
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()));
    well_formed.then_some(parts)
}

/// A usable runtime version is dotted decimal numbers that are not all zero.
fn is_installed_version(version: &str) -> bool {
    version_numbers(version).is_some_and(|parts| {
        parts
            .iter()
            .any(|part| part.bytes().any(|byte| byte != b'0'))
    })
}

/// The updater writes an all-zero version (`0.0.0.0`) when the runtime is uninstalled.
#[cfg(any(target_os = "windows", test))]
fn is_uninstalled_marker(version: &str) -> bool {
    version_numbers(version).is_some_and(|parts| {
        parts
            .iter()
            .all(|part| part.bytes().all(|byte| byte == b'0'))
    })
}

#[cfg(any(target_os = "windows", test))]
impl RuntimeProbe {
    /// Whether this probe is direct evidence that no runtime is installed there.
    fn shows_absence(&self) -> bool {
        match self {
            Self::Missing => true,
            Self::Version(version) => is_uninstalled_marker(version),
            Self::Unreadable => false,
        }
    }
}

/// Returns the first usable runtime version among the probes.
#[cfg(any(target_os = "windows", test))]
pub(super) fn installed_version(probes: &[RuntimeProbe]) -> Option<&str> {
    probes.iter().find_map(|probe| match probe {
        RuntimeProbe::Version(version) if is_installed_version(version) => Some(version.trim()),
        _ => None,
    })
}

/// Combines the probes with the fixed-runtime override into one availability.
///
/// A usable version is reliable evidence. Absence is reported only when every
/// probe definitely found nothing (no value, or the uninstalled `0.0.0.0`
/// marker) and no override points at a fixed runtime; any unreadable probe,
/// malformed version or override leaves the result unknown.
#[cfg(any(target_os = "windows", test))]
pub(super) fn availability(probes: &[RuntimeProbe], override_present: bool) -> WebViewAvailability {
    if installed_version(probes).is_some() {
        WebViewAvailability::Available
    } else if override_present
        || probes.is_empty()
        || !probes.iter().all(RuntimeProbe::shows_absence)
    {
        WebViewAvailability::Unknown
    } else {
        WebViewAvailability::Unavailable
    }
}

/// Availability from the version a linked webview library reports about itself.
///
/// A library that reports a usable version is present and loaded. A missing or
/// unusable report is unknown, never unavailable: the library may still work.
#[cfg(any(
    all(any(target_os = "linux", target_os = "macos"), feature = "webview"),
    test
))]
pub(super) fn linked_runtime_availability(version: Option<&str>) -> WebViewAvailability {
    if version.is_some_and(is_installed_version) {
        WebViewAvailability::Available
    } else {
        WebViewAvailability::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(text: &str) -> RuntimeProbe {
        RuntimeProbe::Version(text.to_owned())
    }

    #[test]
    fn a_usable_version_is_available_even_when_other_probes_failed() {
        let probes = [RuntimeProbe::Unreadable, version("131.0.2903.51")];
        assert_eq!(availability(&probes, false), WebViewAvailability::Available);
        assert_eq!(installed_version(&probes), Some("131.0.2903.51"));
    }

    #[test]
    fn empty_or_malformed_versions_are_not_evidence_either_way() {
        for text in ["", "  ", "abc", "1", "1..2", "1.2.x", "0.0.x"] {
            let probes = [version(text)];
            assert_eq!(installed_version(&probes), None, "{text}");
            assert_eq!(availability(&probes, false), WebViewAvailability::Unknown);
        }
    }

    #[test]
    fn the_uninstalled_marker_counts_as_absence() {
        for text in ["0.0.0.0", " 0.0.0.0 ", "0.0"] {
            let probes = [version(text)];
            assert_eq!(installed_version(&probes), None, "{text}");
            assert_eq!(
                availability(&probes, false),
                WebViewAvailability::Unavailable,
                "{text}"
            );
        }
        // Registry keys survive an uninstall: the marker next to missing keys is still absence.
        let all_absent = [
            version("0.0.0.0"),
            RuntimeProbe::Missing,
            RuntimeProbe::Missing,
        ];
        assert_eq!(
            availability(&all_absent, false),
            WebViewAvailability::Unavailable
        );
        // ...but not when another location is unreadable, and a real version still wins.
        let unreadable = [version("0.0.0.0"), RuntimeProbe::Unreadable];
        assert_eq!(
            availability(&unreadable, false),
            WebViewAvailability::Unknown
        );
        let user_install = [version("0.0.0.0"), version("131.0.2903.51")];
        assert_eq!(
            availability(&user_install, false),
            WebViewAvailability::Available
        );
    }

    #[test]
    fn absence_is_reported_only_when_every_probe_found_nothing() {
        let missing = [RuntimeProbe::Missing, RuntimeProbe::Missing];
        assert_eq!(
            availability(&missing, false),
            WebViewAvailability::Unavailable
        );
        let mixed = [RuntimeProbe::Missing, RuntimeProbe::Unreadable];
        assert_eq!(availability(&mixed, false), WebViewAvailability::Unknown);
        assert_eq!(availability(&[], false), WebViewAvailability::Unknown);
    }

    #[test]
    fn a_fixed_runtime_override_keeps_absence_unknown() {
        let missing = [RuntimeProbe::Missing];
        assert_eq!(availability(&missing, true), WebViewAvailability::Unknown);
        let marker = [version("0.0.0.0")];
        assert_eq!(availability(&marker, true), WebViewAvailability::Unknown);
    }

    #[test]
    fn a_linked_library_version_is_available_and_anything_else_is_unknown() {
        for text in ["2.44.0", "21622.1.22.11.15", "19.0"] {
            assert_eq!(
                linked_runtime_availability(Some(text)),
                WebViewAvailability::Available,
                "{text}"
            );
        }
        for text in [None, Some(""), Some("0.0.0"), Some("2"), Some("x.y.z")] {
            assert_eq!(
                linked_runtime_availability(text),
                WebViewAvailability::Unknown,
                "{text:?}"
            );
        }
    }
}
