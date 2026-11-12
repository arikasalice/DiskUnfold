use std::{
    ffi::OsString,
    process::ExitCode,
    sync::atomic::{AtomicUsize, Ordering},
};

use du_core_contracts::product_support::{
    BuildKind, EmbeddedProductInfo, ProductBuildInfo, ProductNotices, SupportCapabilityObservation,
    SupportCapabilityState, SupportCpu, SupportEnvironmentIdentity, SupportEnvironmentPort,
    SupportOsFamily,
};

use crate::run_with_io_and_product_info;

struct FixedEnvironment;

impl SupportEnvironmentPort for FixedEnvironment {
    fn identity(&self) -> SupportEnvironmentIdentity {
        SupportEnvironmentIdentity {
            os_family: SupportOsFamily::Linux,
            release: Some("24.04".to_owned()),
            edition: Some("ubuntu".to_owned()),
            cpu: SupportCpu::X64,
        }
    }

    fn capabilities(&self) -> Vec<SupportCapabilityObservation> {
        vec![SupportCapabilityObservation {
            area: "full_disk_access".to_owned(),
            state: SupportCapabilityState::NotApplicable,
            reason_key: None,
        }]
    }
}

fn info() -> EmbeddedProductInfo {
    EmbeddedProductInfo {
        build: ProductBuildInfo {
            product_version: Some("0.1.0-test".to_owned()),
            build_kind: BuildKind::Developer,
            commit: None,
            built_at_utc: None,
            target_os: Some("linux".to_owned()),
            target_cpu: Some("x86_64".to_owned()),
            app_db_format: Some(1),
            snapshot_format: Some(1),
            support_matrix_revision: None,
        },
        notices: ProductNotices {
            license: Some("embedded license\n".to_owned()),
            third_party: None,
            source_commit: None,
        },
        support_matrix: None,
    }
}

fn execute(
    input: &[&str],
    product_info: EmbeddedProductInfo,
) -> (ExitCode, Vec<u8>, Vec<u8>, usize) {
    static STORED_READS: AtomicUsize = AtomicUsize::new(0);
    STORED_READS.store(0, Ordering::Relaxed);
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let exit = run_with_io_and_product_info(
        input.iter().map(OsString::from),
        product_info,
        &FixedEnvironment,
        || {
            STORED_READS.fetch_add(1, Ordering::Relaxed);
            Err(())
        },
        || Some(vec!["en-US".to_owned()]),
        &mut stdout,
        &mut stderr,
    );
    (exit, stdout, stderr, STORED_READS.load(Ordering::Relaxed))
}

#[test]
fn support_json_is_language_independent_unknown_without_embedded_records_and_skips_settings() {
    let (exit, stdout, stderr, stored_reads) =
        execute(&["--lang", "en", "support", "--format", "json"], info());
    assert_eq!(exit, ExitCode::SUCCESS);
    assert!(stderr.is_empty());
    assert_eq!(stored_reads, 0);
    let json: serde_json::Value =
        serde_json::from_slice(&stdout).unwrap_or(serde_json::Value::Null);
    assert!(!json.is_null());
    assert_eq!(json["schema"], "diskunfold.cli");
    assert_eq!(json["schemaVersion"], 1);
    assert_eq!(json["command"], "support");
    assert_eq!(json["result"]["combinationStatus"], "unknown");
    assert_eq!(json["result"]["reasonKey"], "support.matrix_unavailable");
    assert_eq!(json["result"]["environment"]["release"], "24.04");
    assert_eq!(json["result"]["features"][0]["area"], "full_disk_access");
    assert_eq!(
        json["result"]["features"][0]["currentCapability"],
        "not_applicable"
    );
}

#[test]
fn support_text_localizes_both_languages_and_reports_unknown_evidence() {
    for (language, expected_title, expected_reason) in [
        (
            "en",
            "Product support",
            "No support matrix is embedded in this build.",
        ),
        (
            "ko",
            "제품 지원 정보",
            "이 빌드에 지원표가 포함되지 않았습니다.",
        ),
    ] {
        let (exit, stdout, stderr, stored_reads) =
            execute(&["--lang", language, "support", "--format", "text"], info());
        let output = String::from_utf8_lossy(&stdout);
        assert_eq!(exit, ExitCode::SUCCESS);
        assert!(stderr.is_empty());
        assert_eq!(stored_reads, 0);
        assert!(output.contains(expected_title), "{output}");
        assert!(output.contains(expected_reason), "{output}");
        assert!(
            output.contains("Unknown") || output.contains("확인 불가"),
            "{output}"
        );
    }
}

#[test]
fn notices_print_only_embedded_license_and_never_read_settings() {
    let (exit, stdout, stderr, stored_reads) =
        execute(&["--lang", "en", "notices", "LICENSE"], info());
    assert_eq!(exit, ExitCode::SUCCESS);
    assert_eq!(stdout, b"embedded license\n");
    assert!(stderr.is_empty());
    assert_eq!(stored_reads, 0);
}

#[test]
fn absent_third_party_notices_are_reported_as_unavailable() {
    for (language, expected) in [
        ("en", "This notice document is not embedded in this build."),
        ("ko", "이 빌드에는 해당 고지 문서가 내장되지 않았습니다."),
    ] {
        let (exit, stdout, stderr, stored_reads) = execute(
            &["--lang", language, "notices", "THIRD-PARTY-NOTICES"],
            info(),
        );
        assert_eq!(exit, ExitCode::from(1));
        assert!(stdout.is_empty());
        assert!(String::from_utf8_lossy(&stderr).contains(expected));
        assert_eq!(stored_reads, 0);
    }
}

#[test]
fn version_prints_build_provenance_without_opening_settings() {
    for (language, expected, expected_commit) in [
        ("en", "Developer build", "Source commit: Unknown"),
        ("ko", "개발자 빌드", "소스 커밋: 확인 불가"),
    ] {
        let (exit, stdout, stderr, stored_reads) =
            execute(&["--lang", language, "--version"], info());
        let output = String::from_utf8_lossy(&stdout);
        assert_eq!(exit, ExitCode::SUCCESS);
        assert!(stderr.is_empty());
        assert_eq!(stored_reads, 0);
        assert!(output.contains(expected), "{output}");
        assert!(output.contains(expected_commit), "{output}");
        assert!(output.contains("0.1.0-test"), "{output}");
    }
}
