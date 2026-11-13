//! Support-table generation from targets.json and test records
//! (F15 FR6.HD1, NFR1.HD1, NFR2.HD1).

mod keys;
mod model;
mod render;
#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use keys::{is_date, validate_key};
pub use model::*;
pub use render::render_markdown;

/// Generation mode. Release mode also blocks unconfirmed security bases.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Mode {
    /// Local development: unconfirmed bases are warnings and marked in the text.
    Development,
    /// Release pipeline: every basis must be present and confirmed.
    Release,
}

impl Mode {
    /// Stable name written into the release gate.
    pub fn name(self) -> &'static str {
        match self {
            Self::Development => "development",
            Self::Release => "release",
        }
    }
}

/// A release-gate finding.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Finding {
    /// Stable release reason key.
    pub reason_key: &'static str,
    /// Row or record the finding is about.
    pub subject: String,
    /// Human-readable detail.
    pub detail: String,
}

/// One generated combination row.
#[derive(Clone, Debug)]
pub struct Row {
    /// OS family key.
    pub os: String,
    /// Encoded release key.
    pub release: String,
    /// Edition identifier.
    pub edition: String,
    /// CPU key.
    pub cpu: String,
    /// Derived status: officially_verified, conditional, unsupported_found or not_verified.
    pub status: &'static str,
    /// Security basis, when complete.
    pub basis: Option<BasisSpec>,
    /// Test record ids behind the status.
    pub records: Vec<String>,
    /// Verified configuration details and limits.
    pub details: Vec<String>,
    /// Feature rows from the records.
    pub features: Vec<RecordFeature>,
}

impl Row {
    fn subject(&self) -> String {
        format!("{}/{}/{}/{}", self.os, self.release, self.edition, self.cpu)
    }
}

/// The generated table and its gate findings.
#[derive(Clone, Debug)]
pub struct SupportTable {
    /// Preparation date.
    pub as_of: String,
    /// Generation mode.
    pub mode: Mode,
    /// All rows in published order.
    pub rows: Vec<Row>,
    /// Findings that stop a release.
    pub blockers: Vec<Finding>,
    /// Findings that do not stop development use.
    pub warnings: Vec<Finding>,
}

fn finding(
    reason_key: &'static str,
    subject: impl Into<String>,
    detail: impl Into<String>,
) -> Finding {
    Finding {
        reason_key,
        subject: subject.into(),
        detail: detail.into(),
    }
}

fn expand_rows(spec: &SupportTableSpec, blockers: &mut Vec<Finding>) -> Vec<Row> {
    let mut rows = BTreeMap::new();
    for group in &spec.groups {
        for release in &group.releases {
            for edition in &group.editions {
                for cpu in &group.cpus {
                    let row = Row {
                        os: group.os.clone(),
                        release: release.release.clone(),
                        edition: edition.clone(),
                        cpu: cpu.clone(),
                        status: "not_verified",
                        basis: release.security_basis.clone(),
                        records: Vec::new(),
                        details: Vec::new(),
                        features: Vec::new(),
                    };
                    let subject = row.subject();
                    if let Err(error) = validate_key(&row.os, &row.release, &row.edition, &row.cpu)
                    {
                        blockers.push(finding(SUPPORT_EVIDENCE_INVALID, &subject, error));
                    } else if rows.insert(subject.clone(), row).is_some() {
                        blockers.push(finding(SUPPORT_EVIDENCE_INVALID, subject, "duplicate row"));
                    }
                }
            }
        }
    }
    rows.into_values().collect()
}

fn basis_complete(basis: Option<&BasisSpec>) -> bool {
    basis.is_some_and(|b| {
        matches!(b.kind.as_deref(), Some("general" | "extended"))
            && b.source.as_deref().is_some_and(|s| !s.trim().is_empty())
            && b.checked_on.as_deref().is_some_and(is_date)
    })
}

fn check_bases(
    rows: &mut [Row],
    mode: Mode,
    blockers: &mut Vec<Finding>,
    warnings: &mut Vec<Finding>,
) {
    for row in rows {
        if !basis_complete(row.basis.as_ref()) {
            row.basis = None;
            blockers.push(finding(
                SUPPORT_BASIS_MISSING,
                row.subject(),
                "security-support source, kind or check date is missing",
            ));
        } else if row.basis.as_ref().is_some_and(|b| !b.confirmed) {
            let item = finding(
                SUPPORT_BASIS_UNCONFIRMED,
                row.subject(),
                "security-support basis needs confirmation against the vendor source",
            );
            if mode == Mode::Release {
                blockers.push(item)
            } else {
                warnings.push(item)
            }
        }
    }
}

fn derive_status(records: &[&TestRecord]) -> &'static str {
    let merged = |name: &str| -> &str {
        let values: Vec<&str> = records
            .iter()
            .filter_map(|r| r.checks.get(name).map(String::as_str))
            .collect();
        ["failed", "passed", "limited"]
            .into_iter()
            .find(|state| values.contains(state))
            .unwrap_or("not_run")
    };
    let states: Vec<&str> = REQUIRED_CHECKS.iter().map(|n| merged(n)).collect();
    if states.contains(&"failed") {
        "unsupported_found"
    } else if states.iter().all(|s| *s == "passed") {
        "officially_verified"
    } else if states.iter().any(|s| matches!(*s, "passed" | "limited")) {
        "conditional"
    } else {
        "not_verified"
    }
}

fn record_valid(record: &TestRecord) -> Result<(), String> {
    validate_key(&record.os, &record.release, &record.edition, &record.cpu)?;
    if !is_date(&record.observed_on) {
        return Err("observedOn must be YYYY-MM-DD".to_owned());
    }
    for (name, value) in &record.checks {
        if !REQUIRED_CHECKS.contains(&name.as_str())
            || !matches!(value.as_str(), "passed" | "failed" | "limited" | "not_run")
        {
            return Err(format!("unknown check {name}={value}"));
        }
    }
    let texts = [
        &record.id,
        &record.os_detail,
        &record.device,
        &record.execution_form,
        &record.evidence,
    ];
    if texts.iter().any(|t| t.trim().is_empty()) {
        return Err("id, osDetail, device, executionForm and evidence are required".to_owned());
    }
    if let Some(feature) = record.features.iter().find(|f| {
        !matches!(
            f.status.as_str(),
            "officially_verified" | "conditional" | "unsupported_found" | "not_verified"
        )
    }) {
        return Err(format!(
            "feature {} has unknown status {}",
            feature.area, feature.status
        ));
    }
    Ok(())
}

fn apply_records(rows: &mut [Row], records: &[TestRecord], blockers: &mut Vec<Finding>) {
    let mut ids = BTreeSet::new();
    let mut by_row: BTreeMap<String, Vec<&TestRecord>> = BTreeMap::new();
    for record in records {
        if let Err(error) = record_valid(record) {
            blockers.push(finding(SUPPORT_EVIDENCE_INVALID, &record.id, error));
        } else if !ids.insert(record.id.as_str()) {
            blockers.push(finding(
                SUPPORT_EVIDENCE_INVALID,
                &record.id,
                "duplicate record id",
            ));
        } else {
            let key = format!(
                "{}/{}/{}/{}",
                record.os, record.release, record.edition, record.cpu
            );
            by_row.entry(key).or_default().push(record);
        }
    }
    let known: BTreeSet<String> = rows.iter().map(Row::subject).collect();
    for (key, group) in &by_row {
        if !known.contains(key) {
            let id = group.first().map_or("", |r| r.id.as_str());
            blockers.push(finding(
                SUPPORT_EVIDENCE_INVALID,
                id,
                format!("no table row for {key}"),
            ));
        }
    }
    for row in rows {
        let Some(group) = by_row.get(&row.subject()) else {
            continue;
        };
        row.status = derive_status(group);
        for record in group {
            row.records.push(record.id.clone());
            row.details.push(format!("OS: {}", record.os_detail));
            row.details.push(format!("device: {}", record.device));
            row.details
                .push(format!("execution form: {}", record.execution_form));
            let checks: Vec<String> = REQUIRED_CHECKS
                .iter()
                .map(|n| {
                    format!(
                        "{n}={}",
                        record.checks.get(*n).map_or("not_run", String::as_str)
                    )
                })
                .collect();
            row.details.push(format!("checks: {}", checks.join(", ")));
            row.details
                .extend(record.limits.iter().map(|l| format!("limit: {l}")));
            row.features.extend(record.features.iter().cloned());
        }
    }
}

/// Builds the table: expands rows, applies real test records, and runs the
/// NFR2 gate. A row without a record stays not_verified; no result is copied
/// from another combination.
pub fn build(spec: &SupportTableSpec, records: &[TestRecord], mode: Mode) -> SupportTable {
    let mut blockers = Vec::new();
    let mut warnings = Vec::new();
    if !is_date(&spec.as_of) {
        blockers.push(finding(
            SUPPORT_EVIDENCE_INVALID,
            "asOf",
            "asOf must be YYYY-MM-DD",
        ));
    }
    let mut rows = expand_rows(spec, &mut blockers);
    check_bases(&mut rows, mode, &mut blockers, &mut warnings);
    apply_records(&mut rows, records, &mut blockers);
    let order = |f: &Finding| (f.reason_key, f.subject.clone());
    blockers.sort_by_key(order);
    warnings.sort_by_key(order);
    SupportTable {
        as_of: spec.as_of.clone(),
        mode,
        rows,
        blockers,
        warnings,
    }
}

impl SupportTable {
    /// The matrix embedded into the executable (camelCase core-contracts shape).
    ///
    /// Rows without a complete security basis are left out, so the application
    /// reports them as outside the verified set; the release gate separately
    /// stops the release for them.
    pub fn matrix_json(&self) -> Value {
        let combinations: Vec<Value> = self
            .rows
            .iter()
            .filter_map(|row| {
                let basis = row.basis.as_ref()?;
                let suffix = if basis.confirmed { "" } else { " [unconfirmed]" };
                Some(json!({
                    "identity": {"osFamily": row.os, "release": row.release, "edition": row.edition, "cpu": row.cpu},
                    "status": row.status,
                    "securitySupport": {
                        "source": format!("{}{suffix}", basis.source.as_deref().unwrap_or_default()),
                        "checkedOn": basis.checked_on,
                        "kind": basis.kind
                    },
                    "verifiedDetails": row.details,
                    "verificationRecords": row.records,
                    "features": row.features.iter().map(|f| json!({"area": f.area, "status": f.status, "conditions": f.conditions})).collect::<Vec<_>>()
                }))
            })
            .collect();
        let digest = Sha256::digest(Value::Array(combinations.clone()).to_string().as_bytes());
        let short: String = digest.iter().take(6).map(|b| format!("{b:02x}")).collect();
        json!({
            "revision": format!("{}.{short}", self.as_of),
            "asOf": self.as_of,
            "combinations": combinations
        })
    }
}
