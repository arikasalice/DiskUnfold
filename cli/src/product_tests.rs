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
use du_platform_os::startup::{
    FullDiskAccessStatus, PrivacySettingsOpenOutcome, StartupCapabilitiesPort,
    StartupDialogOutcome, WebViewAvailability,
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
            product_version: Some("0.0.1-test".to_owned()),
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
    execute_with_access(input, product_info, FullDiskAccessStatus::NotApplicable)
}

/// Reports a fixed full disk access status and refuses every other startup action.
struct FixedAccess(FullDiskAccessStatus);

impl StartupCapabilitiesPort for FixedAccess {
    fn full_disk_access_status(&self) -> FullDiskAccessStatus {
        self.0
    }

    fn webview_availability(&self) -> WebViewAvailability {
        WebViewAvailability::Unknown
    }

    fn open_privacy_settings(&self) -> PrivacySettingsOpenOutcome {
        PrivacySettingsOpenOutcome::Unsupported
    }

    fn show_pre_window_error_dialog(&self, _: &str, _: &str) -> StartupDialogOutcome {
        StartupDialogOutcome::Unsupported
    }
}

fn execute_with_access(
    input: &[&str],
    product_info: EmbeddedProductInfo,
    access: FullDiskAccessStatus,
) -> (ExitCode, Vec<u8>, Vec<u8>, usize) {
    static STORED_READS: AtomicUsize = AtomicUsize::new(0);
    STORED_READS.store(0, Ordering::Relaxed);
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let exit = run_with_io_and_product_info(
        input.iter().map(OsString::from),
        product_info,
        &crate::OsPorts {
            environment: &FixedEnvironment,
            access: &FixedAccess(access),
        },
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
        assert!(output.contains("0.0.1-test"), "{output}");
    }
}

#[test]
fn confirmed_full_disk_access_denial_stops_scan_with_exit_two_and_no_stdout() {
    let (exit, stdout, stderr, _) = execute_with_access(
        &["--lang", "en", "scan", "root"],
        info(),
        FullDiskAccessStatus::Denied,
    );
    assert_eq!(exit, ExitCode::from(2));
    assert!(stdout.is_empty());
    let line = String::from_utf8_lossy(&stderr);
    assert_eq!(line.lines().count(), 1, "{line}");
    assert!(
        line.starts_with(
            "DiskUnfold: startup.full_disk_access_required: Full Disk Access is required"
        ),
        "{line}"
    );
    assert!(line.ends_with(" [INVALID_ARGUMENT]\n"), "{line}");
}

#[test]
fn full_disk_access_denial_is_explained_in_the_selected_language() {
    let (exit, _, stderr, _) = execute_with_access(
        &["--lang", "ko", "scan", "root"],
        info(),
        FullDiskAccessStatus::Denied,
    );
    assert_eq!(exit, ExitCode::from(2));
    let line = String::from_utf8_lossy(&stderr);
    assert!(
        line.starts_with("DiskUnfold: startup.full_disk_access_required: 폴더를 분석하려면"),
        "{line}"
    );
}

#[test]
fn option_without_a_value_is_an_unsupported_argument_not_an_unsupported_language() {
    for args in [
        &["--lang", "en", "support", "--format"][..],
        &["--lang"][..],
        &["--lang", "en", "support", "--format", "--lang"][..],
    ] {
        let (exit, stdout, stderr, _) = execute(args, info());
        assert_eq!(exit, ExitCode::from(2), "{args:?}");
        assert!(stdout.is_empty(), "{args:?}");
        let line = String::from_utf8_lossy(&stderr);
        assert!(!line.contains("locale.unsupported"), "{args:?}: {line}");
        assert_eq!(line.lines().count(), 1, "{args:?}: {line}");
    }
    let (_, _, stderr, _) = execute(&["--lang", "en", "support", "--format"], info());
    assert_eq!(
        String::from_utf8_lossy(&stderr),
        "DiskUnfold: unsupported arguments. See --help.\n"
    );
}

#[test]
fn unknown_or_granted_full_disk_access_does_not_report_a_denial() {
    for status in [
        FullDiskAccessStatus::Unknown,
        FullDiskAccessStatus::Granted,
        FullDiskAccessStatus::NotApplicable,
    ] {
        let (exit, _, stderr, _) =
            execute_with_access(&["--lang", "en", "scan", "root"], info(), status);
        assert_eq!(exit, ExitCode::from(2), "{status:?}");
        assert!(
            !String::from_utf8_lossy(&stderr).contains("full_disk_access_required"),
            "{status:?}"
        );
    }
}

#[test]
fn information_commands_ignore_a_denied_full_disk_access_status() {
    for args in [
        &["--version"][..],
        &["notices", "LICENSE"][..],
        &["support", "--format", "json"][..],
    ] {
        let (exit, stdout, stderr, _) =
            execute_with_access(args, info(), FullDiskAccessStatus::Denied);
        assert_eq!(exit, ExitCode::SUCCESS, "{args:?}");
        assert!(!stdout.is_empty(), "{args:?}");
        assert!(stderr.is_empty(), "{args:?}");
    }
}

#[test]
fn blank_embedded_notice_is_reported_as_unavailable() {
    let mut product = info();
    product.notices.license = Some(" \n".to_owned());
    let (exit, stdout, _, _) = execute(&["--lang", "en", "notices", "LICENSE"], product);
    assert_eq!(exit, ExitCode::from(1));
    assert!(stdout.is_empty());
}

fn matrix(revision: &str) -> du_core_contracts::product_support::SupportMatrix {
    du_core_contracts::product_support::SupportMatrix {
        revision: Some(revision.to_owned()),
        as_of: Some("2026-01-01".to_owned()),
        combinations: Vec::new(),
    }
}

#[test]
fn support_ignores_a_matrix_whose_revision_differs_from_the_build_record() {
    let mut product = info();
    product.build.support_matrix_revision = Some("r2".to_owned());
    product.support_matrix = Some(matrix("r1"));
    let (exit, stdout, _, _) = execute(&["support", "--format", "json"], product);
    assert_eq!(exit, ExitCode::SUCCESS);
    let json: serde_json::Value =
        serde_json::from_slice(&stdout).unwrap_or(serde_json::Value::Null);
    assert_eq!(json["result"]["reasonKey"], "support.matrix_unavailable");
}

#[test]
fn support_json_equals_the_evaluation_behind_the_support_screen() {
    let mut product = info();
    product.build.support_matrix_revision = Some("r1".to_owned());
    product.support_matrix = Some(matrix("r1"));
    let expected = du_engine_api::support::evaluate(
        &matrix("r1"),
        FixedEnvironment.identity(),
        &FixedEnvironment.capabilities(),
    );
    let (exit, stdout, _, _) = execute(&["support", "--format", "json"], product);
    assert_eq!(exit, ExitCode::SUCCESS);
    let json: serde_json::Value =
        serde_json::from_slice(&stdout).unwrap_or(serde_json::Value::Null);
    assert_eq!(
        json["result"],
        serde_json::to_value(expected).unwrap_or_default()
    );
}

#[test]
fn invalid_embedded_product_metadata_exits_one() {
    let exit = crate::run_with_product_info_json(
        ["--version"].into_iter().map(OsString::from),
        "not json",
    );
    assert_eq!(exit, ExitCode::from(1));
}
