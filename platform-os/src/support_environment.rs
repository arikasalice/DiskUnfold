//! Read-only support environment observations for local product information.
use du_core_contracts::product_support::{
    SupportCapabilityObservation, SupportCapabilityState, SupportCpu, SupportEnvironmentIdentity,
    SupportEnvironmentPort, SupportOsFamily,
};
use du_core_contracts::startup::StartupCapabilitiesPort;

/// Production adapter for platform identity and currently observable capabilities.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemSupportEnvironment;

impl SupportEnvironmentPort for SystemSupportEnvironment {
    fn identity(&self) -> SupportEnvironmentIdentity {
        let os_family = match std::env::consts::OS {
            "windows" => SupportOsFamily::Windows,
            "macos" => SupportOsFamily::Macos,
            "linux" => SupportOsFamily::Linux,
            _ => SupportOsFamily::Unknown,
        };
        let cpu = match std::env::consts::ARCH {
            "x86_64" => SupportCpu::X64,
            "aarch64" => SupportCpu::Arm64,
            "x86" | "arm" => SupportCpu::Other,
            _ => SupportCpu::Unknown,
        };
        let (release, edition) = release_and_edition(os_family);
        SupportEnvironmentIdentity {
            os_family,
            release,
            edition,
            cpu,
        }
    }

    fn capabilities(&self) -> Vec<SupportCapabilityObservation> {
        let access = super::startup::SystemStartupCapabilities.full_disk_access_status();
        let (access_state, access_reason_key) = match access {
            super::startup::FullDiskAccessStatus::Granted => {
                (SupportCapabilityState::Supported, None)
            }
            super::startup::FullDiskAccessStatus::Denied => (
                SupportCapabilityState::Limited,
                Some("startup.full_disk_access_required".to_owned()),
            ),
            super::startup::FullDiskAccessStatus::NotApplicable => {
                (SupportCapabilityState::NotApplicable, None)
            }
            super::startup::FullDiskAccessStatus::Unknown => (
                SupportCapabilityState::Unknown,
                Some("support.capability_unknown".to_owned()),
            ),
        };
        let webview = super::startup::SystemStartupCapabilities.webview_availability();
        let (webview_state, webview_reason_key) = match webview {
            super::startup::WebViewAvailability::Available => {
                (SupportCapabilityState::Supported, None)
            }
            super::startup::WebViewAvailability::Unavailable => {
                (SupportCapabilityState::Unsupported, None)
            }
            super::startup::WebViewAvailability::Unknown => (
                SupportCapabilityState::Unknown,
                Some("support.capability_unknown".to_owned()),
            ),
        };
        vec![
            SupportCapabilityObservation {
                area: "full_disk_access".to_owned(),
                state: access_state,
                reason_key: access_reason_key,
            },
            SupportCapabilityObservation {
                area: "webview".to_owned(),
                state: webview_state,
                reason_key: webview_reason_key,
            },
        ]
    }
}

fn release_and_edition(os_family: SupportOsFamily) -> (Option<String>, Option<String>) {
    match os_family {
        SupportOsFamily::Linux => linux_release_and_edition(),
        // No native release and edition adapter has been verified for these
        // platforms yet, so a family and process CPU alone cannot match a row.
        SupportOsFamily::Windows | SupportOsFamily::Macos | SupportOsFamily::Unknown => {
            (None, None)
        }
    }
}

#[cfg(target_os = "linux")]
fn linux_release_and_edition() -> (Option<String>, Option<String>) {
    let Ok(contents) = std::fs::read_to_string("/etc/os-release") else {
        return (None, None);
    };
    let values = parse_os_release(&contents);
    (values.get("VERSION_ID").cloned(), values.get("ID").cloned())
}

#[cfg(not(target_os = "linux"))]
fn linux_release_and_edition() -> (Option<String>, Option<String>) {
    (None, None)
}

#[cfg(any(target_os = "linux", test))]
fn parse_os_release(contents: &str) -> std::collections::BTreeMap<String, String> {
    contents
        .lines()
        .filter_map(|line| {
            let (key, value) = line.split_once('=')?;
            if !matches!(key, "ID" | "VERSION_ID") {
                return None;
            }
            let value = value.trim();
            let value = value
                .strip_prefix('"')
                .and_then(|value| value.strip_suffix('"'))
                .or_else(|| {
                    value
                        .strip_prefix('\'')
                        .and_then(|value| value.strip_suffix('\''))
                })
                .unwrap_or(value)
                .trim();
            if value.is_empty() {
                return None;
            }
            Some((key.to_owned(), value.to_owned()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn parses_only_exact_os_release_identity_keys() {
        let values =
            parse_os_release("ID=ubuntu\nVERSION_ID=\"24.04\"\nID_LIKE=debian\nNAME=Ubuntu\n");
        assert_eq!(
            values,
            BTreeMap::from([
                ("ID".to_owned(), "ubuntu".to_owned()),
                ("VERSION_ID".to_owned(), "24.04".to_owned()),
            ])
        );
    }

    #[test]
    fn malformed_or_empty_identity_values_are_not_reported() {
        let values = parse_os_release("ID=\nVERSION_ID=\"\"\nID_LIKE=ubuntu\n");
        assert!(values.is_empty());
    }

    #[test]
    fn system_identity_never_invents_windows_or_macos_release_data() {
        for family in [SupportOsFamily::Windows, SupportOsFamily::Macos] {
            assert_eq!(release_and_edition(family), (None, None));
        }
    }
}
