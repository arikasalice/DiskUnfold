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

/// Reads the product name and version through `sw_vers`, once per process.
#[cfg(target_os = "macos")]
fn read_system() -> OsReleaseEvidence {
    static CACHE: std::sync::OnceLock<OsReleaseEvidence> = std::sync::OnceLock::new();
    CACHE
        .get_or_init(
            || match (sw_vers("-productName"), sw_vers("-productVersion")) {
                (Some(name), Some(version)) => macos_evidence(&name, &version),
                (Some(name), None) => macos_evidence(&name, ""),
                (None, Some(version)) => macos_evidence("", &version),
                (None, None) => OsReleaseEvidence::default(),
            },
        )
        .clone()
}

/// Longest time one `sw_vers` query may run before it is killed and treated as unknown.
#[cfg(target_os = "macos")]
const SW_VERS_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

/// Runs one `sw_vers` query with the compatibility shim disabled and a bounded wait.
///
/// `SYSTEM_VERSION_COMPAT` makes `sw_vers` report a legacy `10.16` version, so it
/// is removed from the child's environment.
#[cfg(target_os = "macos")]
fn sw_vers(argument: &str) -> Option<String> {
    use std::{io::Read, process::Stdio, time::Instant};

    let mut child = std::process::Command::new("/usr/bin/sw_vers")
        .arg(argument)
        .env_remove("SYSTEM_VERSION_COMPAT")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + SW_VERS_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Ok(None) | Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    };
    let mut text = String::new();
    child.stdout.take()?.read_to_string(&mut text).ok()?;
    status.success().then_some(text)
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
fn read_system() -> OsReleaseEvidence {
    OsReleaseEvidence::default()
}

#[cfg(any(windows, target_os = "macos", test))]
fn clean(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty() && value.chars().all(|c| !c.is_control())).then(|| value.to_owned())
}

/// Maps `os-release` text to release (`VERSION_ID`) and edition (`ID`).
///
/// The parser follows the shell-like syntax of `os-release(5)`: keys are trimmed,
/// `#` lines and trailing comments are ignored, single quotes are literal and
/// double quotes honor backslash escapes. A value that is malformed, unmatched or
/// outside `[A-Za-z0-9._-]+` after unquoting leaves that field unknown, and a later
/// assignment replaces an earlier one as the shell would.
#[cfg(any(target_os = "linux", test))]
pub(crate) fn linux_evidence(contents: &str) -> OsReleaseEvidence {
    let mut evidence = OsReleaseEvidence::default();
    for line in contents.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key.trim() {
            "VERSION_ID" => evidence.release = os_release_token(value),
            "ID" => evidence.edition = os_release_token(value),
            _ => {}
        }
    }
    evidence
}

/// Unquotes one `os-release` value and accepts only `[A-Za-z0-9._-]+`.
#[cfg(any(target_os = "linux", test))]
fn os_release_token(raw: &str) -> Option<String> {
    let raw = raw.trim_start();
    let (value, rest) = match raw.chars().next() {
        Some('"') => {
            let mut value = String::new();
            let mut chars = raw.char_indices().skip(1);
            let end = loop {
                let (index, c) = chars.next()?;
                match c {
                    '"' => break index + 1,
                    '\\' => {
                        let (_, escaped) = chars.next()?;
                        if !matches!(escaped, '$' | '`' | '"' | '\\') {
                            value.push('\\');
                        }
                        value.push(escaped);
                    }
                    other => value.push(other),
                }
            };
            (value, &raw[end..])
        }
        Some('\'') => {
            let length = raw[1..].find('\'')?;
            (raw[1..=length].to_owned(), &raw[length + 2..])
        }
        _ => {
            let end = raw.find(char::is_whitespace).unwrap_or(raw.len());
            (raw[..end].to_owned(), &raw[end..])
        }
    };
    let rest = rest.trim_start();
    let valid = !value.is_empty()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
    ((rest.is_empty() || rest.starts_with('#')) && valid).then_some(value)
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
/// names, so the build number selects the family. Any other major version, a
/// missing `DisplayVersion`, or a Server edition (whose `DisplayVersion` does not
/// follow the client feature-update naming and has no support-matrix encoding)
/// leaves the release unknown.
#[cfg(any(windows, test))]
pub(crate) fn windows_evidence(
    major: u32,
    build: u32,
    display_version: Option<&str>,
    edition_id: Option<&str>,
) -> OsReleaseEvidence {
    let server = edition_id.is_some_and(|edition| {
        edition
            .trim()
            .get(..6)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("server"))
    });
    let release = (major == 10 && !server)
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
    fn linux_comments_keys_and_trailing_comments_are_handled() {
        let text =
            "# ID=fake\n  ID = debian # base\nVERSION_ID=\"12\"   # bookworm\n  # VERSION_ID=99\n";
        assert_eq!(linux_evidence(text), evidence(Some("12"), Some("debian")));
        assert_eq!(
            linux_evidence("ID='arch' #x\nVERSION_ID=rolling.1_a-b"),
            evidence(Some("rolling.1_a-b"), Some("arch"))
        );
    }

    #[test]
    fn linux_unmatched_quotes_and_trailing_garbage_are_unknown() {
        for text in [
            "ID=\"ubuntu\nVERSION_ID=\"24.04",
            "ID='ubuntu\nVERSION_ID='24.04",
            "ID=\"ubuntu\\\"\n",
            "ID=\"ubuntu\" junk\nVERSION_ID=24.04 extra",
            "ID=ubuntu#note\nVERSION_ID=\"24.04\"x",
        ] {
            assert_eq!(linux_evidence(text), evidence(None, None), "{text}");
        }
    }

    #[test]
    fn linux_escapes_and_characters_outside_the_token_set_are_unknown() {
        for text in [
            "ID=\"ub\\\"untu\"\nVERSION_ID=\"2\\4.04\"",
            "ID=\"a\\\\b\"\nVERSION_ID=\"24\\.04\"",
            "ID=\"u buntu\"\nVERSION_ID=\"$x\"",
            "ID=ub/untu\nVERSION_ID=\"24,04\"",
        ] {
            assert_eq!(linux_evidence(text), evidence(None, None), "{text}");
        }
        // A later invalid assignment replaces an earlier valid one.
        assert_eq!(
            linux_evidence("ID=ubuntu\nID=\"bad\\\"id\""),
            evidence(None, None)
        );
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

    #[test]
    fn windows_server_editions_have_an_unknown_release() {
        for edition in ["ServerStandard", "ServerDatacenterEval", "serverrdsh"] {
            assert_eq!(
                windows_evidence(10, 26_100, Some("24H2"), Some(edition)),
                evidence(None, Some(edition)),
                "{edition}"
            );
        }
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
