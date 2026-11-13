//! Read-only support environment observations for local product information.
use du_core_contracts::product_ports::OsReleasePort;
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
        identity_from(
            std::env::consts::OS,
            std::env::consts::ARCH,
            &super::os_release::SystemOsRelease,
        )
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

/// Builds the exact identity from Rust's OS and architecture names plus release evidence.
///
/// An unrecognized OS yields `Unknown` and never queries release evidence, so an
/// unfamiliar platform cannot borrow another platform's values.
pub fn identity_from(
    os: &str,
    arch: &str,
    releases: &dyn OsReleasePort,
) -> SupportEnvironmentIdentity {
    let os_family = match os {
        "windows" => SupportOsFamily::Windows,
        "macos" => SupportOsFamily::Macos,
        "linux" => SupportOsFamily::Linux,
        _ => SupportOsFamily::Unknown,
    };
    let cpu = match arch {
        "x86_64" => SupportCpu::X64,
        "aarch64" => SupportCpu::Arm64,
        "x86" | "arm" => SupportCpu::Other,
        _ => SupportCpu::Unknown,
    };
    let evidence = if os_family == SupportOsFamily::Unknown {
        Default::default()
    } else {
        releases.release_and_edition()
    };
    SupportEnvironmentIdentity {
        os_family,
        release: evidence.release,
        edition: evidence.edition,
        cpu,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use du_core_contracts::product_ports::OsReleaseEvidence;

    struct Fixed(Option<&'static str>, Option<&'static str>);

    impl OsReleasePort for Fixed {
        fn release_and_edition(&self) -> OsReleaseEvidence {
            OsReleaseEvidence {
                release: self.0.map(str::to_owned),
                edition: self.1.map(str::to_owned),
            }
        }
    }

    #[test]
    fn complete_evidence_yields_a_complete_identity() {
        let identity = identity_from("linux", "x86_64", &Fixed(Some("24.04"), Some("ubuntu")));
        assert_eq!(identity.os_family, SupportOsFamily::Linux);
        assert_eq!(identity.cpu, SupportCpu::X64);
        assert!(identity.is_complete());
    }

    #[test]
    fn missing_release_or_edition_keeps_the_identity_incomplete() {
        for fixed in [
            Fixed(None, Some("ubuntu")),
            Fixed(Some("24.04"), None),
            Fixed(None, None),
        ] {
            assert!(!identity_from("macos", "aarch64", &fixed).is_complete());
        }
    }

    #[test]
    fn unrecognized_platform_never_reads_release_evidence() {
        let identity = identity_from("freebsd", "riscv64", &Fixed(Some("14"), Some("bsd")));
        assert_eq!(identity.os_family, SupportOsFamily::Unknown);
        assert_eq!(identity.cpu, SupportCpu::Unknown);
        assert_eq!((identity.release, identity.edition), (None, None));
    }

    #[test]
    fn unrecognized_architecture_is_incomplete_even_with_release_evidence() {
        let identity = identity_from("windows", "riscv64", &Fixed(Some("11-25H2"), Some("Core")));
        assert!(!identity.is_complete());
    }
}
