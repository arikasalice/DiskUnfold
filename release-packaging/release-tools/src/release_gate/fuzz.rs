//! Fuzz gate of the release verification list (ANFR4): an open fuzz crash blocks the release.
//!
//! The architecture DLD tests and fuzz section says an open crash issue makes the release gate
//! stop the release. The fuzz workflow files one issue labelled `fuzz-crash` per crashing run and
//! the issue stays open until a person closes it, so the gate reads the open issues with that
//! label. An unreadable list fails the gate rather than passing it.

use super::GateFailure;

/// Reason key: a release verification item failed (ANFR4), here an open fuzz crash.
pub const VERIFICATION_FAILED: &str = "release.verification_failed";

/// What the fuzz gate saw when it passed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FuzzGateReport {
    /// Number of fuzz targets in the registry. Zero means the gate had no fuzz results to
    /// consult, which is not a fuzz pass.
    pub registered_targets: usize,
}

fn failure(detail: impl Into<String>) -> GateFailure {
    GateFailure::new(VERIFICATION_FAILED, detail)
}

/// Counts the targets in `release-packaging/quality/fuzz-targets.json`.
///
/// # Errors
/// Returns `release.verification_failed` when the registry is not JSON with a `targets` array.
pub fn registered_fuzz_targets(registry_text: &str) -> Result<usize, GateFailure> {
    let value: serde_json::Value = serde_json::from_str(registry_text)
        .map_err(|error| failure(format!("fuzz target registry is not JSON: {error}")))?;
    value["targets"]
        .as_array()
        .map(Vec::len)
        .ok_or_else(|| failure("fuzz target registry has no targets list"))
}

/// Requires zero open fuzz crash issues.
///
/// `issues_json` is the output of
/// `gh issue list --label fuzz-crash --state open --json number,title`.
///
/// # Errors
/// Returns `release.verification_failed` naming the open issues, or when the issue list or the
/// registry cannot be read.
pub fn check_fuzz_crashes(
    issues_json: &str,
    registry_text: &str,
) -> Result<FuzzGateReport, GateFailure> {
    let registered_targets = registered_fuzz_targets(registry_text)?;
    let issues: serde_json::Value = serde_json::from_str(issues_json)
        .map_err(|error| failure(format!("open fuzz crash list is not JSON: {error}")))?;
    let issues = issues
        .as_array()
        .ok_or_else(|| failure("open fuzz crash list is not a JSON array"))?;
    if issues.is_empty() {
        return Ok(FuzzGateReport { registered_targets });
    }
    let listed: Vec<String> = issues
        .iter()
        .map(|issue| {
            let number = issue["number"]
                .as_u64()
                .map_or_else(|| "?".to_owned(), |n| n.to_string());
            format!(
                "#{number} {}",
                issue["title"].as_str().unwrap_or("untitled")
            )
        })
        .collect();
    Err(failure(format!(
        "{} open fuzz crash issue(s): {}",
        issues.len(),
        listed.join("; ")
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;
    const EMPTY_REGISTRY: &str = "{\"targets\": []}";

    #[test]
    fn no_open_crash_passes_and_reports_the_registry_size() -> TestResult {
        assert_eq!(
            check_fuzz_crashes("[]", EMPTY_REGISTRY)?,
            FuzzGateReport {
                registered_targets: 0
            }
        );
        let registry = "{\"targets\": [{\"name\": \"a\"}, {\"name\": \"b\"}]}";
        assert_eq!(check_fuzz_crashes("[]", registry)?.registered_targets, 2);
        Ok(())
    }

    #[test]
    fn an_open_crash_stops_the_release_and_is_named() -> TestResult {
        let issues = "[{\"number\": 7, \"title\": \"Fuzz crash: snapshot open\"}]";
        let failed = check_fuzz_crashes(issues, EMPTY_REGISTRY)
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
    fn unreadable_input_fails_closed() -> TestResult {
        for (issues, registry) in [
            ("not json", EMPTY_REGISTRY),
            ("{}", EMPTY_REGISTRY),
            ("", EMPTY_REGISTRY),
            ("[]", "not json"),
            ("[]", "{}"),
        ] {
            let failed = check_fuzz_crashes(issues, registry)
                .err()
                .ok_or("unreadable input must fail")?;
            assert_eq!(failed.reason_key, VERIFICATION_FAILED);
        }
        Ok(())
    }
}
