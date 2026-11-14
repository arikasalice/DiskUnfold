//! Machine-readable (json) command output.
//!
//! This module deliberately imports neither the message resolver nor a format
//! profile: the renderers take raw values only, so two display languages
//! produce identical bytes (FR9.HD1.cli.DD1).

use std::io::{self, Write};

use du_core_contracts::product_support::SupportReport;

/// Writes the `support` result envelope as one JSON line.
pub(crate) fn write_support_json(
    stdout: &mut impl Write,
    report: &SupportReport,
) -> io::Result<()> {
    serde_json::to_writer(
        &mut *stdout,
        &serde_json::json!({
            "schema": "diskunfold.cli",
            "schemaVersion": 1,
            "command": "support",
            "result": report,
        }),
    )?;
    writeln!(stdout)
}
