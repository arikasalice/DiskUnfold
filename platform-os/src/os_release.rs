//! Operating-system release and edition evidence for support-matrix matching.
//!
//! Every reader returns `None` for a value it cannot establish. Encodings are
//! documented on [`OsReleaseEvidence`].
use du_core_contracts::product_ports::{OsReleaseEvidence, OsReleasePort};

/// Production adapter that reads the evidence kept by the running operating system.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemOsRelease;

impl OsReleasePort for SystemOsRelease {
    fn release_and_edition(&self) -> OsReleaseEvidence {
        read_system()
    }
}

#[cfg(windows)]
fn read_system() -> OsReleaseEvidence {
    crate::windows_os_release::read()
        .map(|facts| {
            windows_evidence(
                facts.major,
                facts.build,
                facts.display_version.as_deref(),
                facts.edition_id.as_deref(),
            )
        })
        .unwrap_or_default()
}

/// Reads `os-release` using the documented `/etc` then `/usr/lib` search order.
#[cfg(target_os = "linux")]
fn read_system() -> OsReleaseEvidence {
    ["/etc/os-release", "/usr/lib/os-release"]
        .iter()
        .find_map(|path| std::fs::read_to_string(path).ok())
        .map(|contents| linux_evidence(&contents))
        .unwrap_or_default()
}

/// Reads the product name and version through `sw_vers`.
#[cfg(target_os = "macos")]
fn read_system() -> OsReleaseEvidence {
    let query = |argument: &str| -> Option<String> {
        let output = std::process::Command::new("/usr/bin/sw_vers")
            .arg(argument)
            .output()
            .ok()?;
        output
            .status
            .success()
            .then(|| String::from_utf8(output.stdout).ok())
            .flatten()
    };
    match (query("-productName"), query("-productVersion")) {
        (Some(name), Some(version)) => macos_evidence(&name, &version),
        (Some(name), None) => macos_evidence(&name, ""),
        (None, Some(version)) => macos_evidence("", &version),
        (None, None) => OsReleaseEvidence::default(),
    }
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
fn read_system() -> OsReleaseEvidence {
    OsReleaseEvidence::default()
}

fn clean(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty() && value.chars().all(|c| !c.is_control())).then(|| value.to_owned())
}

/// Maps `os-release` text to release (`VERSION_ID`) and edition (`ID`).
#[cfg(any(target_os = "linux", test))]
pub(crate) fn linux_evidence(contents: &str) -> OsReleaseEvidence {
    let mut evidence = OsReleaseEvidence::default();
    for line in contents.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        let value = value
            .strip_prefix('"')
            .and_then(|value| value.strip_suffix('"'))
            .or_else(|| {
                value
                    .strip_prefix('\'')
                    .and_then(|value| value.strip_suffix('\''))
            })
            .unwrap_or(value);
        match key {
            "VERSION_ID" => evidence.release = clean(value),
            "ID" => evidence.edition = clean(value),
            _ => {}
        }
    }
    evidence
}

/// Maps `sw_vers` output to the macOS major release and lowercase product name.
///
/// The release is the major component, except for the legacy `10.x` series where
/// the minor component is part of the release identity.
#[cfg(any(target_os = "macos", test))]
pub(crate) fn macos_evidence(product_name: &str, product_version: &str) -> OsReleaseEvidence {
    let mut parts = product_version.trim().split('.');
    let number = |part: Option<&str>| {
        part.filter(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
            .map(str::to_owned)
    };
    let release = match number(parts.next()) {
        Some(major) if major == "10" => number(parts.next()).map(|minor| format!("10.{minor}")),
        other => other,
    };
    OsReleaseEvidence {
        release,
        edition: clean(product_name).map(|name| name.to_lowercase()),
    }
}

/// Maps Windows version facts to `<10|11>-<DisplayVersion>` and the `EditionID`.
///
/// Windows 10 and 11 share major version 10 and overlapping feature-update
/// names, so the build number selects the family. Any other major version, or a
/// missing `DisplayVersion`, leaves the release unknown.
#[cfg(any(windows, test))]
pub(crate) fn windows_evidence(
    major: u32,
    build: u32,
    display_version: Option<&str>,
    edition_id: Option<&str>,
) -> OsReleaseEvidence {
    let release = (major == 10)
        .then(|| display_version.and_then(clean))
        .flatten()
        .map(|version| format!("{}-{version}", if build >= 22_000 { 11 } else { 10 }));
    OsReleaseEvidence {
        release,
        edition: edition_id.and_then(clean),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evidence(release: Option<&str>, edition: Option<&str>) -> OsReleaseEvidence {
        OsReleaseEvidence {
            release: release.map(str::to_owned),
            edition: edition.map(str::to_owned),
        }
    }

    #[test]
    fn linux_uses_exact_version_id_and_id_only() {
        let parsed =
            linux_evidence("ID=ubuntu\nVERSION_ID=\"24.04\"\nID_LIKE=debian\nNAME=Ubuntu\n");
        assert_eq!(parsed, evidence(Some("24.04"), Some("ubuntu")));
        assert_eq!(
            linux_evidence("ID='debian'\nVERSION_ID='12'"),
            evidence(Some("12"), Some("debian"))
        );
    }

    #[test]
    fn linux_empty_malformed_or_missing_values_stay_unknown() {
        assert_eq!(
            linux_evidence("ID=\nVERSION_ID=\"\"\nID_LIKE=ubuntu\n"),
            evidence(None, None)
        );
        assert_eq!(linux_evidence("ID=arch\n"), evidence(None, Some("arch")));
        assert_eq!(linux_evidence("garbage"), evidence(None, None));
    }

    #[test]
    fn macos_release_is_the_major_component_and_edition_the_product_name() {
        assert_eq!(
            macos_evidence("macOS\n", "27.2\n"),
            evidence(Some("27"), Some("macos"))
        );
        assert_eq!(
            macos_evidence("macOS", "15"),
            evidence(Some("15"), Some("macos"))
        );
        assert_eq!(
            macos_evidence("Mac OS X", "10.15.7"),
            evidence(Some("10.15"), Some("mac os x"))
        );
    }

    #[test]
    fn macos_unparseable_versions_and_names_stay_unknown() {
        for version in ["", "beta", ".5", "x.2", "10", "10.x"] {
            assert_eq!(macos_evidence("macOS", version).release, None, "{version}");
        }
        assert_eq!(macos_evidence("  ", "27.2"), evidence(Some("27"), None));
    }

    #[test]
    fn windows_build_number_selects_the_family_and_both_parts_are_required() {
        assert_eq!(
            windows_evidence(10, 26_200, Some("25H2"), Some("Professional")),
            evidence(Some("11-25H2"), Some("Professional"))
        );
        assert_eq!(
            windows_evidence(10, 19_045, Some("22H2"), Some("Core")),
            evidence(Some("10-22H2"), Some("Core"))
        );
        assert_eq!(
            windows_evidence(10, 26_200, None, Some("Core")),
            evidence(None, Some("Core"))
        );
        assert_eq!(
            windows_evidence(6, 9_600, Some("x"), Some("")),
            evidence(None, None)
        );
    }

    #[cfg(windows)]
    #[test]
    fn real_windows_host_reports_a_release_and_edition() {
        let observed = SystemOsRelease.release_and_edition();
        let release = observed.release.unwrap_or_default();
        assert!(
            release.starts_with("11-") || release.starts_with("10-"),
            "unexpected Windows release {release:?}"
        );
        assert!(observed.edition.is_some_and(|edition| !edition.is_empty()));
    }
}
