use super::*;

/// Renders the normalized inventory as a Markdown table and channel-reference list.
pub fn render_markdown(inventory: &ReleaseInventory) -> String {
    let mut output = format!(
        "# {} {} distribution inventory\n\n",
        inventory.product_name, inventory.version
    );
    output.push_str(
        "> Internal inventory output. Staged paths are rehashed, but package contents, executable membership, signatures, and build provenance are not verified by this tool.\n\n",
    );
    output.push_str("| File | Version | OS range | CPU | Format | Channel | Signature | SHA-256 | Run instructions |\n");
    output.push_str("| --- | --- | --- | --- | --- | --- | --- | --- | --- |\n");
    for row in &inventory.rows {
        let max = row
            .os
            .maximum
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_else(|| "open".to_owned());
        output.push_str(&format!(
            "| {} | {} | {} {}–{} | {} | {} | {} | {} | {} | {} |\n",
            markdown_cell(&row.file_name),
            markdown_cell(&row.version),
            row.os.family.sort_key(),
            row.os.minimum,
            max,
            row.cpu.sort_key(),
            row.format.sort_key(),
            row.channel.as_str(),
            row.signature.as_str(),
            markdown_cell(row.sha256.as_str()),
            markdown_cell(&row.run_instructions),
        ));
    }
    output.push_str("\n## Channel references\n\n");
    output.push_str("| Channel | Version | CPU | Artifact row | Update method |\n| --- | --- | --- | --- | --- |\n");
    for reference in &inventory.channel_references {
        output.push_str(&format!(
            "| {} | {} | {} | {} | {} |\n",
            reference.channel.as_str(),
            markdown_cell(&reference.version),
            reference.cpu.sort_key(),
            markdown_cell(&reference.artifact_file_name),
            reference.update_method.as_str(),
        ));
    }
    output
}

/// Serializes stable inventory JSON without local staged-file paths.
pub fn render_json(inventory: &ReleaseInventory) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(inventory)
}

/// Renders verified package and executable hashes in standard `sha256sum` line format.
pub fn render_checksums(verified: &[VerifiedFile]) -> String {
    let mut files = verified.to_vec();
    files.sort_by(|left, right| {
        left.file_name
            .cmp(&right.file_name)
            .then_with(|| left.kind.cmp(&right.kind))
            .then_with(|| left.artifact_file_name.cmp(&right.artifact_file_name))
    });
    let mut output = String::new();
    for file in files {
        output.push_str(&format!("{}  {}\n", file.sha256, file.file_name));
    }
    output
}

fn markdown_cell(value: &str) -> String {
    value
        .replace('|', "\\|")
        .replace(['\r', '\n'], " ")
        .trim()
        .to_owned()
}
