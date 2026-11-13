//! Classifies WebView2 runtime registry evidence into a startup availability value.
//!
//! The classification is pure so every combination of registry outcomes is
//! testable on any host; the Windows adapter supplies the observations.
use super::WebViewAvailability;

/// What one registry probe of a WebView2 runtime version value established.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum RuntimeProbe {
    /// The value exists; its text is the runtime version.
    Version(String),
    /// The key or value is definitely absent.
    Missing,
    /// The probe failed for another reason, so absence is not established.
    Unreadable,
}

/// A usable runtime version is dotted decimal numbers that are not all zero.
fn is_installed_version(version: &str) -> bool {
    let parts: Vec<&str> = version.trim().split('.').collect();
    parts.len() >= 2
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
        && parts
            .iter()
            .any(|part| part.bytes().any(|byte| byte != b'0'))
}

/// Returns the first usable runtime version among the probes.
pub(super) fn installed_version(probes: &[RuntimeProbe]) -> Option<&str> {
    probes.iter().find_map(|probe| match probe {
        RuntimeProbe::Version(version) if is_installed_version(version) => Some(version.trim()),
        _ => None,
    })
}

/// Combines the probes with the fixed-runtime override into one availability.
///
/// A usable version is reliable evidence. Absence is reported only when every
/// probe definitely found nothing and no override points at a fixed runtime;
/// any unreadable probe or override leaves the result unknown.
pub(super) fn availability(probes: &[RuntimeProbe], override_present: bool) -> WebViewAvailability {
    if installed_version(probes).is_some() {
        WebViewAvailability::Available
    } else if override_present
        || probes.is_empty()
        || probes.iter().any(|probe| *probe != RuntimeProbe::Missing)
    {
        WebViewAvailability::Unknown
    } else {
        WebViewAvailability::Unavailable
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
    fn all_zero_empty_or_malformed_versions_are_not_evidence() {
        for text in ["0.0.0.0", "", "  ", "abc", "1", "1..2", "1.2.x"] {
            let probes = [version(text)];
            assert_eq!(installed_version(&probes), None, "{text}");
            assert_eq!(availability(&probes, false), WebViewAvailability::Unknown);
        }
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
    }
}
