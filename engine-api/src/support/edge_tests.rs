use super::*;
use du_core_contracts::product_support::{
    SecuritySupportBasis, SecuritySupportKind, SupportCombinationIdentity, SupportCpu,
    SupportEnvironmentIdentity, SupportFeatureRow, SupportOsFamily,
};

fn identity() -> SupportEnvironmentIdentity {
    SupportEnvironmentIdentity {
        os_family: SupportOsFamily::Linux,
        release: Some("24.04".to_owned()),
        edition: Some("ubuntu".to_owned()),
        cpu: SupportCpu::X64,
    }
}

fn row() -> SupportCombination {
    SupportCombination {
        identity: SupportCombinationIdentity {
            os_family: SupportOsFamily::Linux,
            release: Some("24.04".to_owned()),
            edition: Some("ubuntu".to_owned()),
            cpu: SupportCpu::X64,
        },
        status: SupportStatus::OfficiallyVerified,
        security_support: Some(SecuritySupportBasis {
            source: Some("https://ubuntu.com/about/release-cycle".to_owned()),
            checked_on: Some("2026-11-12".to_owned()),
            kind: Some(SecuritySupportKind::General),
        }),
        verified_details: vec!["native-cli".to_owned()],
        verification_records: vec!["native-test-record-1".to_owned()],
        features: vec![SupportFeatureRow {
            area: "scan".to_owned(),
            status: SupportStatus::OfficiallyVerified,
            conditions: vec!["local filesystems".to_owned()],
        }],
    }
}

fn matrix(rows: Vec<SupportCombination>) -> SupportMatrix {
    SupportMatrix {
        revision: Some("1".to_owned()),
        as_of: Some("2026-11-12".to_owned()),
        combinations: rows,
    }
}

#[test]
fn duplicate_capabilities_are_unknown_without_mislabeling_the_matrix() {
    let report = evaluate(
        &matrix(vec![row()]),
        identity(),
        &[
            SupportCapabilityObservation {
                area: "scan".to_owned(),
                state: SupportCapabilityState::Supported,
                reason_key: None,
            },
            SupportCapabilityObservation {
                area: "scan".to_owned(),
                state: SupportCapabilityState::Unsupported,
                reason_key: None,
            },
        ],
    );

    assert_eq!(report.combination_status, SupportStatus::OfficiallyVerified);
    assert_eq!(
        report.features[0].support_table_status,
        SupportStatus::OfficiallyVerified
    );
    assert_eq!(
        report.features[0].current_capability,
        SupportCapabilityState::Unknown
    );
    assert_eq!(
        report.features[0].reason_key.as_deref(),
        Some(CAPABILITY_UNKNOWN)
    );
}

#[test]
fn invalid_matrix_does_not_erase_current_capability_observations() {
    let mut invalid = row();
    invalid.security_support = None;
    let report = evaluate(
        &matrix(vec![invalid]),
        identity(),
        &[SupportCapabilityObservation {
            area: "scan".to_owned(),
            state: SupportCapabilityState::Limited,
            reason_key: Some("support.capability_limited".to_owned()),
        }],
    );

    assert_eq!(report.combination_status, SupportStatus::Unknown);
    assert_eq!(report.reason_key.as_deref(), Some(MATRIX_INVALID));
    assert_eq!(
        report.features[0].support_table_status,
        SupportStatus::Unknown
    );
    assert_eq!(
        report.features[0].current_capability,
        SupportCapabilityState::Limited
    );
    assert_eq!(
        report.features[0].reason_key.as_deref(),
        Some("support.capability_limited")
    );
}
