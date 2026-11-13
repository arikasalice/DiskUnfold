use super::*;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const EMPTY_REGISTRY: &str = "{\"targets\": []}";
const ONE_TARGET: &str = "{\"targets\": [{\"name\": \"a\"}]}";
const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";
// 2026-11-13T03:23:00Z
const NOW: i64 = 1_794_540_180;

fn run(conclusion: &str, created: &str, sha: &str) -> String {
    format!(
        "[{{\"databaseId\": 9, \"url\": \"https://example/run/9\", \"conclusion\": \"{conclusion}\", \"createdAt\": \"{created}\", \"headSha\": \"{sha}\"}}]"
    )
}

fn gate(
    issues: &str,
    registry: &str,
    runs: &str,
    require_run: bool,
) -> Result<FuzzGateRecord, GateFailure> {
    check_fuzz_gate(&FuzzGateInput {
        issues_json: issues,
        registry_text: registry,
        runs_json: runs,
        now_unix: NOW,
        release_commit: COMMIT,
        require_run,
        max_age_days: 3,
    })
}

#[test]
fn timestamps_parse_to_unix_seconds() {
    assert_eq!(parse_utc_timestamp("1970-01-01T00:00:00Z"), Some(0));
    assert_eq!(parse_utc_timestamp("2026-11-13T03:23:00Z"), Some(NOW));
    assert_eq!(
        parse_utc_timestamp("2024-02-29T12:00:00Z"),
        Some(1_709_208_000)
    );
    for bad in [
        "",
        "2026-11-13",
        "2026-13-01T00:00:00Z",
        "2026-11-13T25:00:00Z",
        "2026-11-13T00:00:00",
    ] {
        assert_eq!(parse_utc_timestamp(bad), None, "{bad}");
    }
}

#[test]
fn an_empty_registry_passes_but_the_record_says_there_is_no_fuzz_evidence() -> TestResult {
    let ok = run("success", "2026-11-13T00:00:00Z", COMMIT);
    let record = gate("[]", EMPTY_REGISTRY, &ok, true)?;
    assert_eq!(record.outcome, NO_TARGETS);
    assert_eq!(record.registered_targets, 0);
    assert!(
        record
            .notes
            .iter()
            .any(|note| note.contains("not a fuzz pass")),
        "{record:?}"
    );
    assert_eq!(record.latest_run.map(|run| run.same_commit), Some(true));
    Ok(())
}

#[test]
fn registered_targets_with_a_recent_success_pass() -> TestResult {
    let ok = run("success", "2026-11-12T03:23:00Z", "f".repeat(40).as_str());
    let record = gate("[]", ONE_TARGET, &ok, true)?;
    assert_eq!(record.outcome, TARGETS_REGISTERED);
    assert_eq!(record.latest_run.map(|run| run.same_commit), Some(false));
    Ok(())
}

#[test]
fn an_open_crash_stops_the_release_and_is_named() -> TestResult {
    let ok = run("success", "2026-11-13T00:00:00Z", COMMIT);
    let issues = "[{\"number\": 7, \"title\": \"Fuzz crash: snapshot open\"}]";
    let failed = gate(issues, EMPTY_REGISTRY, &ok, true)
        .err()
        .ok_or("an open crash must fail")?;
    assert_eq!(failed.reason_key, "release.verification_failed");
    assert!(
        failed.detail.contains("#7 Fuzz crash: snapshot open"),
        "{failed}"
    );
    Ok(())
}

#[test]
fn a_failed_timed_out_or_cancelled_latest_run_blocks_even_without_a_crash_file() -> TestResult {
    for conclusion in ["failure", "timed_out", "cancelled", ""] {
        for require in [true, false] {
            let runs = run(conclusion, "2026-11-13T00:00:00Z", COMMIT);
            let failed = gate("[]", ONE_TARGET, &runs, require)
                .err()
                .ok_or("must fail")?;
            assert_eq!(failed.reason_key, VERIFICATION_FAILED, "{conclusion}");
            assert!(failed.detail.contains("not success"), "{failed}");
        }
    }
    Ok(())
}

#[test]
fn a_missing_or_stale_run_blocks_an_official_build_and_only_warns_a_developer_build() -> TestResult
{
    let failed = gate("[]", ONE_TARGET, "[]", true)
        .err()
        .ok_or("missing run must fail")?;
    assert!(
        failed.detail.contains("no completed Fuzz workflow run"),
        "{failed}"
    );
    let record = gate("[]", ONE_TARGET, "[]", false)?;
    assert!(record.latest_run.is_none());
    assert!(
        record
            .notes
            .iter()
            .any(|note| note.contains("not required for a developer build"))
    );

    let stale = run("success", "2026-11-01T00:00:00Z", COMMIT);
    let failed = gate("[]", ONE_TARGET, &stale, true)
        .err()
        .ok_or("stale run must fail")?;
    assert!(failed.detail.contains("days old"), "{failed}");
    let record = gate("[]", ONE_TARGET, &stale, false)?;
    assert!(record.notes.iter().any(|note| note.contains("days old")));
    Ok(())
}

#[test]
fn unreadable_input_fails_closed() -> TestResult {
    let ok = run("success", "2026-11-13T00:00:00Z", COMMIT);
    for (issues, registry, runs) in [
        ("not json", EMPTY_REGISTRY, ok.as_str()),
        ("{}", EMPTY_REGISTRY, ok.as_str()),
        ("", EMPTY_REGISTRY, ok.as_str()),
        ("[]", "not json", ok.as_str()),
        ("[]", "{}", ok.as_str()),
        ("[]", EMPTY_REGISTRY, "not json"),
        ("[]", EMPTY_REGISTRY, "{}"),
        (
            "[]",
            EMPTY_REGISTRY,
            "[{\"conclusion\": \"success\", \"createdAt\": \"yesterday\"}]",
        ),
    ] {
        let failed = gate(issues, registry, runs, true)
            .err()
            .ok_or("unreadable input must fail")?;
        assert_eq!(failed.reason_key, VERIFICATION_FAILED);
    }
    Ok(())
}
