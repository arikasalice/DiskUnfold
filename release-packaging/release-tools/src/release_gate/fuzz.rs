//! Fuzz gate of the release verification list (ANFR4): an open fuzz crash blocks the release, and
//! so does a failed latest Fuzz run.
//!
//! The architecture DLD tests and fuzz section says an open crash issue makes the release gate stop
//! the release. The fuzz workflow files one issue labelled `fuzz-crash` per crashing run (and when
//! the job fails), and the issue stays open until a person closes it. A fuzz job that fails or times
//! out without writing a crash file leaves no issue, so the gate also reads the latest completed Fuzz
//! workflow run and stops on any conclusion other than `success`. Missing or unreadable input fails
//! the gate. The outcome is returned as a record that the inventory publishes, so a release with no
//! registered fuzz target says so instead of looking like a fuzz pass.

use super::GateFailure;
use serde::Serialize;
use serde_json::Value;

/// Reason key: a release verification item failed (ANFR4), here a fuzz crash or a failed fuzz run.
pub const VERIFICATION_FAILED: &str = "release.verification_failed";

/// Outcome recorded when the registry has no target: the gate had no fuzz results to consult.
pub const NO_TARGETS: &str = "no_fuzz_targets_registered";
/// Outcome recorded when targets are registered and the gate passed.
pub const TARGETS_REGISTERED: &str = "fuzz_targets_registered";

/// The latest completed Fuzz workflow run the gate looked at.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct FuzzRunRecord {
    /// Workflow run id.
    pub run_id: u64,
    /// Run page.
    pub url: String,
    /// Conclusion reported by GitHub.
    pub conclusion: String,
    /// Creation time, UTC.
    pub created_at: String,
    /// Commit the run tested.
    pub head_sha: String,
    /// Whether that commit is the release commit. Fuzz runs are scheduled on the default branch,
    /// so a tag build is usually later than the run.
    pub same_commit: bool,
}

/// What the fuzz gate saw. It is written into the release inventory and DISTRIBUTION.md.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct FuzzGateRecord {
    /// `no_fuzz_targets_registered` or `fuzz_targets_registered`.
    pub outcome: &'static str,
    /// Number of fuzz targets in the registry.
    pub registered_targets: usize,
    /// Open `fuzz-crash` issues; always zero in a passing record.
    pub open_crashes: usize,
    /// The latest completed Fuzz run, when one exists.
    pub latest_run: Option<FuzzRunRecord>,
    /// Statements a reader must not miss, for example that no fuzz evidence exists.
    pub notes: Vec<String>,
}

/// The inputs of the fuzz gate.
#[derive(Clone, Copy, Debug)]
pub struct FuzzGateInput<'a> {
    /// `gh issue list --label fuzz-crash --state open --json number,title` output.
    pub issues_json: &'a str,
    /// Contents of `release-packaging/quality/fuzz-targets.json`.
    pub registry_text: &'a str,
    /// `gh run list --workflow fuzz.yml --status completed --json ...` output, newest first.
    pub runs_json: &'a str,
    /// Current time as Unix seconds.
    pub now_unix: i64,
    /// The release commit.
    pub release_commit: &'a str,
    /// Official builds require a recent successful run; developer builds only record its absence.
    pub require_run: bool,
    /// Oldest acceptable run age in days when a run is required.
    pub max_age_days: i64,
}

fn failure(detail: impl Into<String>) -> GateFailure {
    GateFailure::new(VERIFICATION_FAILED, detail)
}

/// Counts the targets in `release-packaging/quality/fuzz-targets.json`.
///
/// # Errors
/// Returns `release.verification_failed` when the registry is not JSON with a `targets` array.
pub fn registered_fuzz_targets(registry_text: &str) -> Result<usize, GateFailure> {
    let value: Value = serde_json::from_str(registry_text)
        .map_err(|error| failure(format!("fuzz target registry is not JSON: {error}")))?;
    value["targets"]
        .as_array()
        .map(Vec::len)
        .ok_or_else(|| failure("fuzz target registry has no targets list"))
}

/// Parses `YYYY-MM-DDTHH:MM:SSZ` into Unix seconds.
pub fn parse_utc_timestamp(text: &str) -> Option<i64> {
    let (date, time) = text.strip_suffix('Z')?.split_once('T')?;
    let mut d = date.split('-').map(|part| part.parse::<i64>().ok());
    let (year, month, day) = (d.next()??, d.next()??, d.next()??);
    let mut t = time.split(':').map(|part| part.parse::<i64>().ok());
    let (hour, minute, second) = (t.next()??, t.next()??, t.next()??);
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }
    // Days from civil date (proleptic Gregorian), after Howard Hinnant.
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + hour * 3600 + minute * 60 + second)
}

fn open_crashes(issues_json: &str) -> Result<(), GateFailure> {
    let issues: Value = serde_json::from_str(issues_json)
        .map_err(|error| failure(format!("open fuzz crash list is not JSON: {error}")))?;
    let issues = issues
        .as_array()
        .ok_or_else(|| failure("open fuzz crash list is not a JSON array"))?;
    if issues.is_empty() {
        return Ok(());
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

fn latest_run(
    input: &FuzzGateInput<'_>,
    notes: &mut Vec<String>,
) -> Result<Option<FuzzRunRecord>, GateFailure> {
    let runs: Value = serde_json::from_str(input.runs_json)
        .map_err(|error| failure(format!("Fuzz run list is not JSON: {error}")))?;
    let runs = runs
        .as_array()
        .ok_or_else(|| failure("Fuzz run list is not a JSON array"))?;
    let Some(run) = runs.first() else {
        if input.require_run {
            return Err(failure(
                "no completed Fuzz workflow run exists; run the Fuzz workflow before releasing",
            ));
        }
        notes.push(
            "no completed Fuzz workflow run exists (not required for a developer build)".to_owned(),
        );
        return Ok(None);
    };
    let text = |key: &str| run[key].as_str().unwrap_or_default().to_owned();
    let record = FuzzRunRecord {
        run_id: run["databaseId"].as_u64().unwrap_or_default(),
        url: text("url"),
        conclusion: text("conclusion"),
        created_at: text("createdAt"),
        same_commit: text("headSha") == input.release_commit,
        head_sha: text("headSha"),
    };
    if record.conclusion != "success" {
        return Err(failure(format!(
            "the latest Fuzz run {} ended with {:?}, not success",
            record.url, record.conclusion
        )));
    }
    let created = parse_utc_timestamp(&record.created_at).ok_or_else(|| {
        failure(format!(
            "Fuzz run time {:?} is not a UTC timestamp",
            record.created_at
        ))
    })?;
    let age_days = (input.now_unix - created).div_euclid(86_400);
    if age_days > input.max_age_days {
        let message = format!(
            "the latest successful Fuzz run is {age_days} days old (limit {})",
            input.max_age_days
        );
        if input.require_run {
            return Err(failure(message));
        }
        notes.push(message);
    }
    Ok(Some(record))
}

/// Applies the fuzz gate.
///
/// # Errors
/// Returns `release.verification_failed` for an open fuzz crash issue, a latest Fuzz run that did
/// not succeed, a missing or stale run when one is required, or input that cannot be read.
pub fn check_fuzz_gate(input: &FuzzGateInput<'_>) -> Result<FuzzGateRecord, GateFailure> {
    let registered_targets = registered_fuzz_targets(input.registry_text)?;
    open_crashes(input.issues_json)?;
    let mut notes = Vec::new();
    let latest_run = latest_run(input, &mut notes)?;
    let outcome = if registered_targets == 0 {
        notes.push(
            "no fuzz targets are registered, so no fuzz evidence exists for this release; this is not a fuzz pass"
                .to_owned(),
        );
        NO_TARGETS
    } else {
        TARGETS_REGISTERED
    };
    Ok(FuzzGateRecord {
        outcome,
        registered_targets,
        open_crashes: 0,
        latest_run,
        notes,
    })
}

#[cfg(test)]
mod tests;
