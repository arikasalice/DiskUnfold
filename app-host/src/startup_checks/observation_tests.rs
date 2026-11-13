//! Tests for the real-observation startup rows 1, 4, 5 and 6 (F15 FR4.HD1.app-host.DD1).
use super::*;
use du_core_contracts::product_support::{
    BuildKind, EmbeddedProductInfo, ProductBuildInfo, ProductNotices, SecuritySupportBasis,
    SecuritySupportKind, SupportCapabilityObservation, SupportCapabilityState, SupportCombination,
    SupportCombinationIdentity, SupportCpu, SupportEnvironmentIdentity, SupportEnvironmentPort,
    SupportMatrix, SupportOsFamily, SupportStatus,
};

const STORE: &[u8] = b"<html></html>body{}";
const MANIFEST: &str = "/index.html\ttext/html\t0\t13\n/a.css\ttext/css\t13\t19\n";

fn hex(manifest: &str, store: &[u8]) -> String {
    asset_digest::assets_digest_hex(manifest, store)
}

#[test]
fn consistent_assets_pass_row_one() -> Result<(), Box<dyn std::error::Error>> {
    let row = resource_integrity_observation(MANIFEST, STORE, &hex(MANIFEST, STORE))?;
    assert_eq!(row.check, StartupCheck::ResourceIntegrity);
    assert_eq!(row.result, StartupCheckOutcome::Pass);
    Ok(())
}

#[test]
fn damaged_or_inconsistent_assets_fail_row_one_with_the_published_reason() {
    let digest = hex(MANIFEST, STORE);
    let mut flipped = STORE.to_vec();
    flipped[3] ^= 1;
    let cases: [(&str, &[u8], String); 10] = [
        (MANIFEST, &flipped, digest.clone()),
        (MANIFEST, STORE, "0".repeat(64)),
        (MANIFEST, STORE, String::new()),
        // same tiling, different content type: only the manifest changed
        (
            "/index.html\ttext/plain\t0\t13\n/a.css\ttext/css\t13\t19\n",
            STORE,
            digest.clone(),
        ),
        // a digest over the byte store alone does not authenticate the manifest
        (MANIFEST, STORE, {
            use sha2::{Digest, Sha256};
            Sha256::digest(STORE)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect()
        }),
        // gap between entries
        (
            "/index.html\ttext/html\t0\t13\n/a.css\ttext/css\t14\t19\n",
            STORE,
            digest.clone(),
        ),
        // entries stop before the end of the byte store
        ("/index.html\ttext/html\t0\t13\n", STORE, digest.clone()),
        // range past the byte store
        ("/index.html\ttext/html\t0\t99\n", STORE, digest.clone()),
        // entry page missing
        ("/a.css\ttext/css\t0\t19\n", STORE, digest.clone()),
        // duplicate name and malformed line
        (
            "/index.html\ttext/html\t0\t13\n/index.html\ttext/html\t13\t19\n",
            STORE,
            digest,
        ),
    ];
    for (manifest, store, digest) in cases {
        let error = resource_integrity_observation(manifest, store, &digest).err();
        assert_eq!(
            error.map(|error| error.to_string()).as_deref(),
            Some("startup.resource_integrity_failed")
        );
    }
    assert!(
        resource_integrity_observation("/index.html\ttext/html\t0\n", STORE, &hex(MANIFEST, STORE))
            .is_err()
    );
    assert!(resource_integrity_observation("", &[], &hex("", &[])).is_err());
}

#[test]
fn the_assets_embedded_in_this_build_pass_row_one() -> Result<(), Box<dyn std::error::Error>> {
    let manifest = include_str!(concat!(env!("OUT_DIR"), "/assets.txt"));
    let assets = include_bytes!(concat!(env!("OUT_DIR"), "/assets.bin"));
    let digest = include_str!(concat!(env!("OUT_DIR"), "/assets.sha256"));
    let row = resource_integrity_observation(manifest, assets, digest)?;
    assert_eq!(row.result, StartupCheckOutcome::Pass);
    Ok(())
}

#[test]
fn row_four_reports_only_product_temporary_files() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let clean = temporary_files::temporary_files_observation(dir.path());
    assert_eq!(clean.result, StartupCheckOutcome::Pass);
    assert_eq!(clean.check, StartupCheck::TemporaryFiles);

    std::fs::write(dir.path().join("notes.tmp"), b"user file")?;
    std::fs::write(dir.path().join(".diskunfold-create.db"), b"")?;
    assert_eq!(
        temporary_files::temporary_files_observation(dir.path()).result,
        StartupCheckOutcome::Pass
    );

    let leftover = dir.path().join(".diskunfold-create-7-1.tmp");
    std::fs::write(&leftover, b"half written")?;
    let found = temporary_files::temporary_files_observation(dir.path());
    assert_eq!(found.result, StartupCheckOutcome::Limited);
    assert_eq!(found.reason_key.as_deref(), Some("export.temp_file_left"));
    // The check observes only; the interrupted file is still there for its owner.
    assert!(leftover.exists());
    Ok(())
}

#[test]
fn row_four_cannot_list_a_missing_folder_so_it_stays_unverifiable()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let missing = dir.path().join("absent");
    assert_eq!(
        temporary_files::temporary_files_observation(&missing).result,
        StartupCheckOutcome::Unverifiable
    );
    // A file where the folder should be is also not a listable folder.
    let file = dir.path().join("file");
    std::fs::write(&file, b"")?;
    assert_eq!(
        temporary_files::temporary_files_observation(&file).result,
        StartupCheckOutcome::Unverifiable
    );
    Ok(())
}

struct FakeEnvironment {
    identity: SupportEnvironmentIdentity,
    capabilities: Vec<SupportCapabilityObservation>,
}

impl SupportEnvironmentPort for FakeEnvironment {
    fn identity(&self) -> SupportEnvironmentIdentity {
        self.identity.clone()
    }
    fn capabilities(&self) -> Vec<SupportCapabilityObservation> {
        self.capabilities.clone()
    }
}

fn windows_identity() -> SupportEnvironmentIdentity {
    SupportEnvironmentIdentity {
        os_family: SupportOsFamily::Windows,
        release: Some("25H2".into()),
        edition: Some("Professional".into()),
        cpu: SupportCpu::X64,
    }
}

fn capability(area: &str, state: SupportCapabilityState) -> SupportCapabilityObservation {
    SupportCapabilityObservation {
        area: area.into(),
        state,
        reason_key: None,
    }
}

fn product(status: Option<SupportStatus>, revision: Option<&str>) -> EmbeddedProductInfo {
    let identity = SupportCombinationIdentity {
        os_family: SupportOsFamily::Windows,
        release: Some("25H2".into()),
        edition: Some("Professional".into()),
        cpu: SupportCpu::X64,
    };
    EmbeddedProductInfo {
        build: ProductBuildInfo {
            product_version: None,
            build_kind: BuildKind::Unknown,
            commit: None,
            built_at_utc: None,
            target_os: None,
            target_cpu: None,
            app_db_format: None,
            snapshot_format: None,
            support_matrix_revision: revision.map(str::to_owned),
        },
        notices: ProductNotices {
            license: None,
            third_party: None,
            source_commit: None,
        },
        support_matrix: status.map(|status| SupportMatrix {
            revision: Some("m1".into()),
            as_of: Some("2026-11-01".into()),
            combinations: vec![SupportCombination {
                identity,
                status,
                security_support: Some(SecuritySupportBasis {
                    source: Some("https://vendor.example/lifecycle".into()),
                    checked_on: Some("2026-11-01".into()),
                    kind: Some(SecuritySupportKind::General),
                }),
                verified_details: Vec::new(),
                verification_records: vec!["record-1".into()],
                features: Vec::new(),
            }],
        }),
    }
}

fn row_five(
    info: &EmbeddedProductInfo,
    identity: SupportEnvironmentIdentity,
) -> StartupCheckResult {
    support_matrix_observation(
        info,
        &FakeEnvironment {
            identity,
            capabilities: Vec::new(),
        },
    )
}

#[test]
fn row_five_maps_the_matching_combination_status() {
    let cases = [
        (
            SupportStatus::OfficiallyVerified,
            StartupCheckOutcome::Pass,
            None,
        ),
        (
            SupportStatus::Conditional,
            StartupCheckOutcome::Limited,
            None,
        ),
        (
            SupportStatus::NotVerified,
            StartupCheckOutcome::OutsideVerified,
            Some("support.outside_verified"),
        ),
        (
            SupportStatus::UnsupportedFound,
            StartupCheckOutcome::OutsideVerified,
            Some("support.outside_verified"),
        ),
        (
            SupportStatus::Unknown,
            StartupCheckOutcome::Unverifiable,
            Some("support.env_unknown"),
        ),
    ];
    for (status, outcome, reason) in cases {
        let row = row_five(&product(Some(status), Some("m1")), windows_identity());
        assert_eq!(
            (row.result, row.reason_key.as_deref()),
            (outcome, reason),
            "{status:?}"
        );
    }
}

#[test]
fn row_five_is_outside_verified_for_an_environment_absent_from_the_matrix() {
    let mut identity = windows_identity();
    identity.release = Some("24H2".into());
    let row = row_five(
        &product(Some(SupportStatus::OfficiallyVerified), None),
        identity,
    );
    assert_eq!(row.result, StartupCheckOutcome::OutsideVerified);
    assert_eq!(row.reason_key.as_deref(), Some("support.outside_verified"));
}

#[test]
fn row_five_fails_closed_without_identity_or_a_usable_matrix() {
    let verified = product(Some(SupportStatus::OfficiallyVerified), Some("m1"));
    let mut unidentified = windows_identity();
    unidentified.edition = None;
    let row = row_five(&verified, unidentified);
    assert_eq!(row.result, StartupCheckOutcome::Unverifiable);
    assert_eq!(row.reason_key.as_deref(), Some("support.env_unknown"));

    let absent = row_five(&product(None, None), windows_identity());
    assert_eq!(absent.result, StartupCheckOutcome::Unverifiable);
    assert_eq!(
        absent.reason_key.as_deref(),
        Some("support.matrix_unavailable")
    );

    // A build that records another matrix revision than the embedded one is not trusted.
    let mismatched = row_five(
        &product(Some(SupportStatus::OfficiallyVerified), Some("m0")),
        windows_identity(),
    );
    assert_eq!(mismatched.result, StartupCheckOutcome::Unverifiable);
}

fn row_six(capabilities: Vec<SupportCapabilityObservation>) -> StartupCheckResult {
    capability_observation(&FakeEnvironment {
        identity: windows_identity(),
        capabilities,
    })
}

#[test]
fn row_six_limits_on_any_limited_capability_and_ignores_the_access_row() {
    use SupportCapabilityState::{Limited, NotApplicable, Supported, Unknown, Unsupported};
    let pass = row_six(vec![
        capability("webview", Supported),
        capability("full_disk_access", NotApplicable),
    ]);
    assert_eq!(
        (pass.result, pass.reason_key),
        (StartupCheckOutcome::Pass, None)
    );

    for state in [Limited, Unsupported] {
        let row = row_six(vec![
            capability("webview", Supported),
            capability("other", state),
        ]);
        assert_eq!(row.result, StartupCheckOutcome::Limited);
        assert_eq!(
            row.reason_key.as_deref(),
            Some("support.capability_limited")
        );
    }
    // Denied full disk access is row seven's block and must not also limit row six.
    let access_only = row_six(vec![
        capability("webview", Supported),
        capability("full_disk_access", Limited),
    ]);
    assert_eq!(access_only.result, StartupCheckOutcome::Pass);

    // Limited evidence outranks unknown evidence.
    let mixed = row_six(vec![capability("a", Unknown), capability("b", Limited)]);
    assert_eq!(mixed.result, StartupCheckOutcome::Limited);
}

#[test]
fn row_six_never_passes_on_unknown_or_missing_capability_evidence() {
    let unknown = row_six(vec![
        capability("webview", SupportCapabilityState::Supported),
        capability("gpu", SupportCapabilityState::Unknown),
    ]);
    assert_eq!(unknown.result, StartupCheckOutcome::Unverifiable);
    assert_eq!(
        unknown.reason_key.as_deref(),
        Some("support.capability_unknown")
    );
    assert_eq!(
        row_six(Vec::new()).result,
        StartupCheckOutcome::Unverifiable
    );
    assert_eq!(
        row_six(vec![capability(
            "full_disk_access",
            SupportCapabilityState::Supported
        )])
        .result,
        StartupCheckOutcome::Unverifiable
    );
}

#[test]
fn real_rows_drive_the_report_grade() -> Result<(), Box<dyn std::error::Error>> {
    let environment = FakeEnvironment {
        identity: windows_identity(),
        capabilities: vec![capability("webview", SupportCapabilityState::Supported)],
    };
    let all_pass = vec![
        resource_integrity_observation(MANIFEST, STORE, &hex(MANIFEST, STORE))?,
        StartupCheckResult {
            check: StartupCheck::Webview,
            result: StartupCheckOutcome::Pass,
            reason_key: None,
        },
        data_format_and_open_observation(true),
        temporary_files::temporary_files_observation(tempfile::tempdir()?.path()),
        support_matrix_observation(
            &product(Some(SupportStatus::OfficiallyVerified), Some("m1")),
            &environment,
        ),
        capability_observation(&environment),
    ];
    let report = status(true, StartupAccessState::NotApplicable, &all_pass)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(report.grade, StartupGrade::Pass);

    let outside = environment_with_other_release();
    let mut rows = all_pass;
    rows[4] = support_matrix_observation(
        &product(Some(SupportStatus::OfficiallyVerified), Some("m1")),
        &outside,
    );
    let report = status(true, StartupAccessState::NotApplicable, &rows)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(report.grade, StartupGrade::OutsideVerified);
    assert!(report.analysis_allowed);
    Ok(())
}

fn environment_with_other_release() -> FakeEnvironment {
    let mut identity = windows_identity();
    identity.release = Some("26H1".into());
    FakeEnvironment {
        identity,
        capabilities: Vec::new(),
    }
}
