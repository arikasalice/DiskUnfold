//! Pure support-matrix evaluation used by the CLI and application shell.
use std::collections::{BTreeMap, BTreeSet};

use du_core_contracts::product_support::{
    SecuritySupportBasis, SecuritySupportKind, SupportCapabilityObservation,
    SupportCapabilityState, SupportCombination, SupportEnvironmentIdentity,
    SupportFeatureAssessment, SupportMatrix, SupportReport, SupportStatus,
};

mod embedded;

pub use embedded::embedded_matrix;

const ENVIRONMENT_UNKNOWN: &str = "support.env_unknown";
const OUTSIDE_VERIFIED: &str = "support.outside_verified";
const MATRIX_UNAVAILABLE: &str = "support.matrix_unavailable";
const MATRIX_INVALID: &str = "support.matrix_invalid";
const CAPABILITY_UNKNOWN: &str = "support.capability_unknown";

/// Evaluates the exact environment against an embedded support matrix.
///
/// Matching uses all four published identity keys: operating-system family,
/// release, edition, and process CPU. Missing values never act as wildcards.
/// Matrix metadata that omits required security-support evidence fails closed
/// and cannot produce an officially verified result.
pub fn evaluate(
    matrix: &SupportMatrix,
    environment: SupportEnvironmentIdentity,
    capabilities: &[SupportCapabilityObservation],
) -> SupportReport {
    let matrix_valid = validate_matrix(matrix);
    let duplicate_areas = duplicate_capability_areas(capabilities);
    let match_rows: Vec<_> = if environment.is_complete() {
        matrix
            .combinations
            .iter()
            .filter(|row| row.identity.matches(&environment))
            .collect()
    } else {
        Vec::new()
    };

    let (matching, combination_status, reason_key) = if !environment.is_complete() {
        (None, SupportStatus::Unknown, Some(ENVIRONMENT_UNKNOWN))
    } else if !matrix_valid {
        (None, SupportStatus::Unknown, Some(MATRIX_INVALID))
    } else if matrix.combinations.is_empty() {
        (None, SupportStatus::Unknown, Some(MATRIX_UNAVAILABLE))
    } else if match_rows.len() > 1 {
        (None, SupportStatus::Unknown, Some(MATRIX_INVALID))
    } else if let Some(row) = match_rows.first().copied() {
        (Some(row), row.status, None)
    } else {
        (None, SupportStatus::NotVerified, Some(OUTSIDE_VERIFIED))
    };

    let features = feature_assessments(
        &matrix.combinations,
        matching,
        capabilities,
        &duplicate_areas,
        if matrix_valid {
            None
        } else {
            Some(MATRIX_INVALID)
        },
        combination_status,
    );
    let security_support = matching.and_then(|row| row.security_support.clone());

    SupportReport {
        matrix_revision: matrix.revision.clone(),
        as_of: matrix.as_of.clone(),
        environment,
        combination_status,
        security_support,
        features,
        combinations: safe_combinations(&matrix.combinations, matrix_valid),
        reason_key: reason_key.map(str::to_owned),
    }
}

fn feature_assessments(
    combinations: &[SupportCombination],
    matching: Option<&SupportCombination>,
    capabilities: &[SupportCapabilityObservation],
    duplicate_areas: &BTreeSet<&str>,
    invalid_reason: Option<&str>,
    combination_status: SupportStatus,
) -> Vec<SupportFeatureAssessment> {
    let mut table_rows = Vec::new();
    let mut seen_areas = BTreeSet::new();
    for combination in combinations {
        for feature in &combination.features {
            if seen_areas.insert(feature.area.as_str()) {
                table_rows.push(feature.area.as_str());
            }
        }
    }
    for capability in capabilities {
        if seen_areas.insert(capability.area.as_str()) {
            table_rows.push(capability.area.as_str());
        }
    }

    let matched_rows: BTreeMap<_, _> = matching
        .into_iter()
        .flat_map(|row| row.features.iter())
        .map(|feature| (feature.area.as_str(), feature))
        .collect();
    let observed: BTreeMap<_, _> = capabilities
        .iter()
        .map(|capability| (capability.area.as_str(), capability))
        .collect();

    table_rows
        .into_iter()
        .map(|area| {
            let feature = matched_rows.get(area).copied();
            let capability = observed.get(area).copied();
            let duplicate = duplicate_areas.contains(area);
            let current_capability = if duplicate {
                SupportCapabilityState::Unknown
            } else {
                capability.map_or(SupportCapabilityState::Unknown, |value| value.state)
            };
            let support_table_status = if invalid_reason.is_some() {
                SupportStatus::Unknown
            } else if matching.is_some() {
                feature.map_or(SupportStatus::Unknown, |value| value.status)
            } else if matches!(combination_status, SupportStatus::NotVerified) {
                SupportStatus::NotVerified
            } else {
                SupportStatus::Unknown
            };
            let reason_key = if duplicate {
                Some(CAPABILITY_UNKNOWN)
            } else if capability.is_none() || current_capability == SupportCapabilityState::Unknown
            {
                capability
                    .and_then(|value| value.reason_key.as_deref())
                    .or(Some(CAPABILITY_UNKNOWN))
            } else {
                capability.and_then(|value| value.reason_key.as_deref())
            };
            SupportFeatureAssessment {
                area: area.to_owned(),
                support_table_status,
                current_capability,
                conditions: feature.map_or_else(Vec::new, |value| value.conditions.clone()),
                reason_key: reason_key.map(str::to_owned),
            }
        })
        .collect()
}

fn duplicate_capability_areas(capabilities: &[SupportCapabilityObservation]) -> BTreeSet<&str> {
    let mut seen = BTreeSet::new();
    let mut duplicates = BTreeSet::new();
    for capability in capabilities {
        if !seen.insert(capability.area.as_str()) {
            duplicates.insert(capability.area.as_str());
        }
    }
    duplicates
}

fn validate_matrix(matrix: &SupportMatrix) -> bool {
    if matrix.combinations.is_empty() {
        return true;
    }
    if !present(&matrix.revision) || !matrix.as_of.as_deref().is_some_and(is_iso_date) {
        return false;
    }

    let mut identities = BTreeSet::new();
    for row in &matrix.combinations {
        if !row_identity_complete(row) || !security_basis_complete(row.security_support.as_ref()) {
            return false;
        }
        let row_has_records = !row.verification_records.is_empty()
            && row
                .verification_records
                .iter()
                .all(|record| !record.trim().is_empty());
        if row.status != SupportStatus::NotVerified
            && row.status != SupportStatus::Unknown
            && !row_has_records
        {
            return false;
        }
        if row.features.iter().any(|feature| {
            feature.status != SupportStatus::NotVerified
                && feature.status != SupportStatus::Unknown
                && !row_has_records
        }) {
            return false;
        }
        let identity_key = format!(
            "{:?}|{}|{}|{:?}",
            row.identity.os_family,
            row.identity.release.as_deref().unwrap_or_default(),
            row.identity.edition.as_deref().unwrap_or_default(),
            row.identity.cpu
        );
        if !identities.insert(identity_key) {
            return false;
        }
        let mut areas = BTreeSet::new();
        if row
            .features
            .iter()
            .any(|feature| feature.area.trim().is_empty() || !areas.insert(feature.area.as_str()))
        {
            return false;
        }
    }
    true
}

fn row_identity_complete(row: &SupportCombination) -> bool {
    row.identity.os_family != du_core_contracts::product_support::SupportOsFamily::Unknown
        && row
            .identity
            .release
            .as_ref()
            .is_some_and(|value| !value.trim().is_empty())
        && row
            .identity
            .edition
            .as_ref()
            .is_some_and(|value| !value.trim().is_empty())
        && !matches!(
            row.identity.cpu,
            du_core_contracts::product_support::SupportCpu::Unknown
                | du_core_contracts::product_support::SupportCpu::Other
        )
}

fn security_basis_complete(basis: Option<&SecuritySupportBasis>) -> bool {
    basis.is_some_and(|value| {
        present(&value.source)
            && value.checked_on.as_deref().is_some_and(is_iso_date)
            && value
                .kind
                .is_some_and(|kind| kind != SecuritySupportKind::Unknown)
    })
}

fn is_iso_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 10
        || !bytes[0..4].iter().all(u8::is_ascii_digit)
        || bytes[4] != b'-'
        || !bytes[5..7].iter().all(u8::is_ascii_digit)
        || bytes[7] != b'-'
        || !bytes[8..10].iter().all(u8::is_ascii_digit)
    {
        return false;
    }
    let year = u16::from(bytes[0] - b'0') * 1000
        + u16::from(bytes[1] - b'0') * 100
        + u16::from(bytes[2] - b'0') * 10
        + u16::from(bytes[3] - b'0');
    let month = u16::from(bytes[5] - b'0') * 10 + u16::from(bytes[6] - b'0');
    let day = u16::from(bytes[8] - b'0') * 10 + u16::from(bytes[9] - b'0');
    let days_in_month = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year % 400 == 0 || (year % 4 == 0 && year % 100 != 0) => 29,
        2 => 28,
        _ => 0,
    };
    year > 0 && (1..=days_in_month).contains(&day)
}

fn present(value: &Option<String>) -> bool {
    value.as_ref().is_some_and(|value| !value.trim().is_empty())
}

fn safe_combinations(rows: &[SupportCombination], valid: bool) -> Vec<SupportCombination> {
    if valid {
        return rows.to_vec();
    }
    rows.iter()
        .cloned()
        .map(|mut row| {
            row.status = SupportStatus::Unknown;
            if !security_basis_complete(row.security_support.as_ref()) {
                row.security_support = None;
            }
            for feature in &mut row.features {
                feature.status = SupportStatus::Unknown;
            }
            row
        })
        .collect()
}

#[cfg(test)]
mod tests {
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

    #[test]
    fn iso_check_dates_are_calendar_valid() {
        assert!(is_iso_date("2024-02-29"));
        assert!(!is_iso_date("2026-02-29"));
        assert!(!is_iso_date("2026-04-31"));
        assert!(!is_iso_date("0000-01-01"));
    }

    fn matrix(rows: Vec<SupportCombination>) -> SupportMatrix {
        SupportMatrix {
            revision: Some("1".to_owned()),
            as_of: Some("2026-11-12".to_owned()),
            combinations: rows,
        }
    }

    #[test]
    fn exact_identity_returns_matrix_status_and_current_capability_separately() {
        let report = evaluate(
            &matrix(vec![row()]),
            identity(),
            &[SupportCapabilityObservation {
                area: "scan".to_owned(),
                state: SupportCapabilityState::Limited,
                reason_key: Some("support.capability_limited".to_owned()),
            }],
        );
        assert_eq!(report.combination_status, SupportStatus::OfficiallyVerified);
        assert_eq!(
            report
                .security_support
                .as_ref()
                .and_then(|basis| basis.kind),
            Some(SecuritySupportKind::General)
        );
        assert_eq!(
            report.features[0].support_table_status,
            SupportStatus::OfficiallyVerified
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

    #[test]
    fn incomplete_identity_is_unknown_and_never_matches_a_wildcard_row() {
        let mut observed = identity();
        observed.release = None;
        let report = evaluate(&matrix(vec![row()]), observed, &[]);
        assert_eq!(report.combination_status, SupportStatus::Unknown);
        assert_eq!(report.reason_key.as_deref(), Some(ENVIRONMENT_UNKNOWN));
    }

    #[test]
    fn known_combination_without_a_row_is_not_verified() {
        let mut unsupported_release = row();
        unsupported_release.identity.release = Some("25.04".to_owned());
        let report = evaluate(&matrix(vec![unsupported_release]), identity(), &[]);
        assert_eq!(report.combination_status, SupportStatus::NotVerified);
        assert_eq!(report.reason_key.as_deref(), Some(OUTSIDE_VERIFIED));
    }

    #[test]
    fn missing_security_support_basis_prevents_an_official_status() {
        let mut invalid = row();
        invalid.security_support = None;
        let report = evaluate(&matrix(vec![invalid]), identity(), &[]);
        assert_eq!(report.combination_status, SupportStatus::Unknown);
        assert_eq!(report.reason_key.as_deref(), Some(MATRIX_INVALID));
        assert_eq!(
            report.features[0].support_table_status,
            SupportStatus::Unknown
        );
    }

    #[test]
    fn malformed_security_check_date_prevents_an_official_status() {
        let mut invalid = row();
        if let Some(basis) = invalid.security_support.as_mut() {
            basis.checked_on = Some("2026-1-2".to_owned());
        }
        let report = evaluate(&matrix(vec![invalid]), identity(), &[]);
        assert_eq!(report.combination_status, SupportStatus::Unknown);
        assert_eq!(report.reason_key.as_deref(), Some(MATRIX_INVALID));
        assert!(report.security_support.is_none());
    }

    #[test]
    fn feature_status_without_a_test_record_fails_closed() {
        let mut invalid = row();
        invalid.status = SupportStatus::Unknown;
        invalid.verification_records.clear();
        let report = evaluate(&matrix(vec![invalid]), identity(), &[]);
        assert_eq!(report.combination_status, SupportStatus::Unknown);
        assert_eq!(report.reason_key.as_deref(), Some(MATRIX_INVALID));
    }

    #[test]
    fn collapsed_other_cpu_class_does_not_match_exact_support_rows() {
        let mut observed = identity();
        observed.cpu = SupportCpu::Other;
        let report = evaluate(&matrix(vec![row()]), observed, &[]);
        assert_eq!(report.combination_status, SupportStatus::Unknown);
        assert_eq!(report.reason_key.as_deref(), Some(ENVIRONMENT_UNKNOWN));
    }

    #[test]
    fn duplicate_exact_rows_fail_closed() {
        let report = evaluate(&matrix(vec![row(), row()]), identity(), &[]);
        assert_eq!(report.combination_status, SupportStatus::Unknown);
        assert_eq!(report.reason_key.as_deref(), Some(MATRIX_INVALID));
    }

    #[test]
    fn absent_current_capability_is_unknown() {
        let report = evaluate(&matrix(vec![row()]), identity(), &[]);
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
    fn current_capabilities_are_reported_without_a_support_matrix() {
        let capability = SupportCapabilityObservation {
            area: "webview".to_owned(),
            state: SupportCapabilityState::Unsupported,
            reason_key: None,
        };
        let report = evaluate(&SupportMatrix::default(), identity(), &[capability]);
        assert_eq!(report.combination_status, SupportStatus::Unknown);
        assert_eq!(report.features.len(), 1);
        assert_eq!(report.features[0].area, "webview");
        assert_eq!(
            report.features[0].support_table_status,
            SupportStatus::Unknown
        );
        assert_eq!(
            report.features[0].current_capability,
            SupportCapabilityState::Unsupported
        );
    }
}

#[cfg(test)]
mod edge_tests;
