//! Markdown rendering of the support table.

use std::fmt::Write;

use super::{Finding, SupportTable};

fn cell(value: &str) -> String {
    value.replace('|', "/").replace('\n', " ")
}

fn basis_cell(row: &super::Row) -> String {
    match &row.basis {
        None => "missing".to_owned(),
        Some(basis) => {
            let mut parts = vec![
                basis.kind.clone().unwrap_or_default(),
                basis.source.clone().unwrap_or_default(),
                format!(
                    "checked {}",
                    basis.checked_on.as_deref().unwrap_or_default()
                ),
            ];
            if let Some(end) = &basis.vendor_end_date {
                parts.push(format!("vendor end {end}"));
            }
            if !basis.confirmed {
                parts.push("NEEDS CONFIRMATION".to_owned());
            }
            cell(&parts.join("; "))
        }
    }
}

fn write_findings(out: &mut String, title: &str, items: &[Finding]) {
    let _ = writeln!(out, "\n## {title}\n");
    if items.is_empty() {
        out.push_str("None.\n");
    }
    for item in items {
        let _ = writeln!(
            out,
            "- {} {}: {}",
            item.reason_key, item.subject, item.detail
        );
    }
}

/// Renders the published support table. Output is deterministic.
pub fn render_markdown(table: &SupportTable, revision: &str) -> String {
    let mut out = String::from("# DiskUnfold support table\n\n");
    let _ = writeln!(
        out,
        "Revision: {revision}  \nAs of: {}  \nMode: {}\n",
        table.as_of,
        table.mode.name()
    );
    out.push_str(
        "A status comes only from test records for the exact row. A row without a record is\n\
         not verified. The security-support basis is a separate column and is not used at run time.\n\n",
    );
    out.push_str("| OS | Release | Edition | CPU | Status | Security-support basis | Evidence |\n| --- | --- | --- | --- | --- | --- | --- |\n");
    for row in &table.rows {
        let evidence = if row.records.is_empty() {
            "none".to_owned()
        } else {
            cell(&format!(
                "{} ({})",
                row.records.join(", "),
                row.details.join("; ")
            ))
        };
        let _ = writeln!(
            out,
            "| {} | {} | {} | {} | {} | {} | {} |",
            row.os,
            row.release,
            row.edition,
            row.cpu,
            row.status,
            basis_cell(row),
            evidence
        );
    }
    write_findings(&mut out, "Release gate blockers", &table.blockers);
    write_findings(&mut out, "Warnings", &table.warnings);
    out
}
