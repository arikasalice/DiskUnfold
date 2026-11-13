//! Release resources: notices and support matrix for the embedded build info
//! (F15 FR5.HD1.app-host.DD1, FR6.HD1.app-host.DD1, NFR2.HD1.app-host.DD1).

use std::{fs, path::Path, process::Command};

use serde_json::{Value, json};

use crate::{
    notices::{self, NoticeReport},
    support_table::{self, Finding, Mode, SupportTable, TargetsFile, TestRecord},
};

/// Returns true for a lower-case 40 digit hexadecimal commit id.
fn is_full_commit(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Reason key: a release run has no usable full source commit.
pub const SOURCE_COMMIT_MISSING: &str = "release.source_commit_missing";

#[cfg(test)]
mod tests;

/// Everything produced for one build.
#[derive(Clone, Debug)]
pub struct Resources {
    /// The product-resources JSON that launcher/build.rs reads.
    pub json: Value,
    /// THIRD-PARTY-NOTICES text.
    pub notices: String,
    /// Published support table.
    pub support_markdown: String,
    /// Embedded matrix.
    pub support_matrix: Value,
    /// Gate blockers; the release stops when this is not empty.
    pub blockers: Vec<Finding>,
}

fn notice_findings(report: &NoticeReport) -> Vec<Finding> {
    report
        .violations
        .iter()
        .map(|v| Finding {
            reason_key: v.reason_key,
            subject: v.package.clone(),
            detail: v
                .license
                .clone()
                .unwrap_or_else(|| "no license expression".to_owned()),
        })
        .collect()
}

fn finding_json(finding: &Finding) -> Value {
    json!({"reasonKey": finding.reason_key, "subject": finding.subject, "detail": finding.detail})
}

/// Combines notices and the support table into the resources document.
pub fn compose(
    source_commit: Option<&str>,
    report: &NoticeReport,
    table: &SupportTable,
) -> Resources {
    let mut blockers = table.blockers.clone();
    blockers.extend(notice_findings(report));
    if table.mode == Mode::Release && !source_commit.is_some_and(is_full_commit) {
        blockers.push(Finding {
            reason_key: SOURCE_COMMIT_MISSING,
            subject: "sourceCommit".to_owned(),
            detail: "release mode needs a full 40 digit source commit".to_owned(),
        });
    }
    let matrix = table.matrix_json();
    let revision = matrix["revision"].as_str().unwrap_or_default().to_owned();
    let document = json!({
        "sourceCommit": source_commit,
        "releaseGate": {
            "passed": blockers.is_empty(),
            "mode": table.mode.name(),
            "blockers": blockers.iter().map(finding_json).collect::<Vec<_>>(),
            "warnings": table.warnings.iter().map(finding_json).collect::<Vec<_>>()
        },
        "thirdPartyNotices": report.text,
        "supportMatrix": matrix
    });
    Resources {
        json: document,
        notices: report.text.clone(),
        support_markdown: support_table::render_markdown(table, &revision),
        support_matrix: matrix,
        blockers,
    }
}

fn load_records(dir: &Path) -> Result<Vec<TestRecord>, String> {
    let mut paths: Vec<_> = fs::read_dir(dir)
        .map_err(|e| format!("read {}: {e}", dir.display()))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    paths.sort();
    paths
        .iter()
        .map(|path| {
            let text =
                fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
            serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
        })
        .collect()
}

fn git_head(repo: &Path) -> Option<String> {
    let output = Command::new("git")
        .current_dir(repo)
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()?;
    let text = String::from_utf8(output.stdout).ok()?;
    Some(text.trim().to_owned()).filter(|sha| is_full_commit(sha))
}

/// Generates the resources for a checkout. The source commit defaults to HEAD.
///
/// # Errors
/// Returns a message when an input document cannot be read or parsed.
pub fn generate(
    repo: &Path,
    source_commit: Option<String>,
    mode: Mode,
) -> Result<Resources, String> {
    let targets_path = repo.join("release-packaging/quality/targets.json");
    let targets_text =
        fs::read_to_string(&targets_path).map_err(|e| format!("read targets.json: {e}"))?;
    let targets: TargetsFile =
        serde_json::from_str(&targets_text).map_err(|e| format!("targets.json: {e}"))?;
    let records = load_records(&repo.join("release-packaging/release-tools/support-records"))?;
    let table = support_table::build(&targets.support_table, &records, mode);
    let report = notices::generate(repo)?;
    let commit = source_commit.or_else(|| git_head(repo));
    Ok(compose(commit.as_deref(), &report, &table))
}
