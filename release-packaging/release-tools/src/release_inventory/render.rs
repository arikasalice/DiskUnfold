use super::*;

/// Renders the normalized inventory as a Markdown table and channel-reference list.
pub fn render_markdown(inventory: &ReleaseInventory) -> String {
    let mut output = format!(
        "# {} {} distribution inventory\n\n",
        inventory.product_name, inventory.version
    );
    output.push_str(
        "> Internal inventory output. Staged paths are rehashed, but package contents, executable membership, signatures, and build provenance are not verified or created by this tool. This output is not a release gate.\n\n",
    );
    output.push_str("| File | Version | OS support | CPU | Format | Channel | Signature | Signature scope | SHA-256 | SBOM | Run instructions |\n");
    output.push_str("| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |\n");
    for row in &inventory.rows {
        let os_support = match &row.os.versions {
            OsVersionScope::Range { minimum, maximum } => format!(
                "{} {}–{}",
                row.os.family.sort_key(),
                minimum,
                maximum
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| "open".to_owned())
            ),
            OsVersionScope::Exact { supported_versions } => format!(
                "{} {}",
                row.os.family.sort_key(),
                supported_versions
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        };
        output.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
            markdown_cell(&row.file_name),
            markdown_cell(&row.version),
            markdown_cell(&os_support),
            row.cpu.sort_key(),
            row.format.sort_key(),
            row.channel.as_str(),
            row.signature.as_str(),
            markdown_cell(&row.signature_scope),
            markdown_cell(row.sha256.as_str()),
            markdown_cell(row.sbom_file_name.as_deref().unwrap_or("not generated")),
            markdown_cell(&row.run_instructions),
        ));
    }
    output.push_str("\n## Runtime requirements\n\n");
    output.push_str(
        "- Windows 11: the Microsoft Edge WebView2 Runtime ships with Windows. DiskUnfold does not bundle it. If it is missing, DiskUnfold says so at startup and offers to open Microsoft's WebView2 page.\n- macOS: WebKit is part of the system.\n- Ubuntu: the .deb installs GTK 4 and WebKitGTK 6.0 automatically. The tar.gz does not; its README.txt lists the packages and the `apt` command.\n",
    );
    output.push_str("\n## Executable checksums\n\n");
    output.push_str("| Package | Executable | SHA-256 |\n| --- | --- | --- |\n");
    for row in &inventory.rows {
        output.push_str(&format!(
            "| {} | {} | {} |\n",
            markdown_cell(&row.file_name),
            markdown_cell(&row.executable_file_name),
            markdown_cell(row.executable_sha256.as_str()),
        ));
    }
    output.push_str(
        "\nAfter extracting or installing a package, hash its executable and compare it with the row for that package in `EXECUTABLE-SHA256.json`. The package-to-executable relationship is supplied by the manifest; this tool does not inspect package contents to verify membership.\n\n",
    );
    output.push_str("## Check package checksums\n\n");
    output.push_str(
        "Place `SHA256SUMS` beside the downloadable package files. To check one downloaded package, run Windows PowerShell `Get-FileHash -LiteralPath '<package-file>' -Algorithm SHA256`, macOS `shasum -a 256 '<package-file>'`, or Ubuntu `sha256sum '<package-file>'`, then compare the result with that filename's entry in `SHA256SUMS`. When all files listed in `SHA256SUMS` are present in the same directory, macOS can run `shasum -a 256 -c SHA256SUMS` and Ubuntu can run `sha256sum -c SHA256SUMS`.\n\n",
    );
    output.push_str(
        "For an executable after extraction or installation, use Windows PowerShell `Get-FileHash -LiteralPath '<executable-path>' -Algorithm SHA256`, macOS `shasum -a 256 '<executable-path>'`, or Ubuntu `sha256sum '<executable-path>'`; compare the result with the entry keyed by the downloaded package in `EXECUTABLE-SHA256.json`. Match means the bytes equal the published digest; check any separately published signature and provenance evidence because a digest match alone does not establish authenticity. Mismatch means stop using the file and download it again from the official release page. If the published digest or a hash tool is unavailable, treat verification as unavailable and do not treat the file as verified.\n\n",
    );
    output.push_str("## Channel references\n\n");
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
    if let Some(fuzz) = &inventory.fuzz_gate {
        output.push_str(&render_fuzz_gate(fuzz));
    }
    if let Some(bundle) = &inventory.sample_bundle {
        output.push_str(&format!(
            "\n## Sample data bundle\n\n- Bundle version: {}\n- Format version: {}\n- Declared items: {}\n- BLAKE3: {}\n",
            markdown_cell(&bundle.bundle_version),
            bundle.format_version,
            bundle.declared_item_count,
            bundle.blake3,
        ));
    }
    output
}

/// The fuzz gate section: what was checked and, for an empty registry, that no fuzz evidence exists.
fn render_fuzz_gate(fuzz: &serde_json::Value) -> String {
    let text = |value: &serde_json::Value| {
        value
            .as_str()
            .map_or_else(|| value.to_string(), str::to_owned)
    };
    let mut output = String::from("\n## Fuzz gate\n\n");
    output.push_str(&format!(
        "- Outcome: {}\n- Registered fuzz targets: {}\n- Open fuzz crash issues: {}\n",
        text(&fuzz["outcome"]),
        text(&fuzz["registered_targets"]),
        text(&fuzz["open_crashes"]),
    ));
    let run = &fuzz["latest_run"];
    if run.is_null() {
        output.push_str("- Latest completed Fuzz run: none found\n");
    } else {
        output.push_str(&format!(
            "- Latest completed Fuzz run: {} ({}, {}, commit {}, same commit as the release: {})\n",
            markdown_cell(&text(&run["url"])),
            markdown_cell(&text(&run["conclusion"])),
            markdown_cell(&text(&run["created_at"])),
            markdown_cell(&text(&run["head_sha"])),
            text(&run["same_commit"]),
        ));
    }
    for note in fuzz["notes"].as_array().into_iter().flatten() {
        output.push_str(&format!("- Note: {}\n", markdown_cell(&text(note))));
    }
    output
}

/// Serializes stable inventory JSON without local staged-file paths.
pub fn render_json(inventory: &ReleaseInventory) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(inventory)
}

/// Renders verified downloadable package hashes in standard `sha256sum` line format.
///
/// Executable hashes are rendered separately because each release asset directory contains
/// packages, while each package may contain an executable with the same basename as other rows.
pub fn render_checksums(verified: &[VerifiedFile]) -> String {
    let mut files: Vec<_> = verified
        .iter()
        .filter(|file| file.kind == StagedFileKind::Package)
        .cloned()
        .collect();
    files.sort_by(|left, right| left.artifact_file_name.cmp(&right.artifact_file_name));
    let mut output = String::new();
    for file in files {
        output.push_str(&format!("{}  {}\n", file.sha256, file.artifact_file_name));
    }
    output
}

/// Renders executable digests keyed by the package associated with each staged executable.
///
/// The package association comes from the manifest; this renderer does not verify package
/// membership.
pub fn render_executable_checksums(
    inventory: &ReleaseInventory,
) -> Result<String, serde_json::Error> {
    #[derive(serde::Serialize)]
    struct ExecutableChecksum<'a> {
        artifact_file_name: &'a str,
        executable_file_name: &'a str,
        sha256: &'a str,
    }

    #[derive(serde::Serialize)]
    struct ExecutableChecksumManifest<'a> {
        schema_version: u8,
        product_name: &'a str,
        version: &'a str,
        executables: Vec<ExecutableChecksum<'a>>,
    }

    serde_json::to_string_pretty(&ExecutableChecksumManifest {
        schema_version: 1,
        product_name: &inventory.product_name,
        version: &inventory.version,
        executables: inventory
            .rows
            .iter()
            .map(|row| ExecutableChecksum {
                artifact_file_name: &row.file_name,
                executable_file_name: &row.executable_file_name,
                sha256: row.executable_sha256.as_str(),
            })
            .collect(),
    })
}

fn markdown_cell(value: &str) -> String {
    value
        .replace('|', "\\|")
        .replace(['\r', '\n'], " ")
        .trim()
        .to_owned()
}
