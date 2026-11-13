//! Deterministic THIRD-PARTY-NOTICES text.

use std::{collections::BTreeMap, fmt::Write};

use sha2::{Digest, Sha256};

use super::NoticePackage;

fn normalize(text: &str) -> String {
    let lines: Vec<&str> = text.lines().map(str::trim_end).collect();
    let mut joined = lines.join("\n");
    joined.truncate(joined.trim_end().len());
    let start = joined.len() - joined.trim_start_matches('\n').len();
    format!("{}\n", &joined[start..])
}

fn digest(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Renders the package list followed by each distinct license text once.
/// Output depends only on the package set and texts: it is sorted, uses LF
/// line endings and contains no timestamps or paths.
pub fn render_notices(packages: &[NoticePackage]) -> String {
    let mut sorted: Vec<&NoticePackage> = packages.iter().collect();
    sorted.sort_by(|a, b| {
        (a.ecosystem, &a.name, &a.version).cmp(&(b.ecosystem, &b.name, &b.version))
    });

    let mut out = String::from("DiskUnfold THIRD-PARTY-NOTICES\n\n");
    out.push_str(
        "DiskUnfold includes the third-party packages listed below. They come from\n\
         Cargo.lock (cargo metadata --locked, normal dependencies of the launcher)\n\
         and the production packages of gui-web/package-lock.json. Each package is\n\
         listed with its SPDX license expression; license texts follow, one copy per\n\
         distinct text.\n\n",
    );
    let _ = writeln!(out, "Packages: {}\n", sorted.len());

    let mut groups: BTreeMap<String, (String, Vec<String>)> = BTreeMap::new();
    let mut without_text = Vec::new();
    for package in &sorted {
        let label = format!(
            "{} {} {}",
            package.ecosystem.label(),
            package.name,
            package.version
        );
        let expression = package.license.as_deref().unwrap_or("UNKNOWN");
        let _ = writeln!(out, "{label}: {expression}");
        let mut any = false;
        for license in &package.texts {
            let text = normalize(&license.text);
            if text.trim().is_empty() {
                continue;
            }
            any = true;
            let entry = groups
                .entry(digest(&text))
                .or_insert_with(|| (text, Vec::new()));
            if !entry.1.contains(&label) {
                entry.1.push(label.clone());
            }
        }
        if !any {
            without_text.push(format!("{label}: {expression}"));
        }
    }
    if !without_text.is_empty() {
        out.push_str("\nPackages without a bundled license file (SPDX expression only):\n");
        for line in &without_text {
            let _ = writeln!(out, "{line}");
        }
    }
    for (index, (hash, (text, users))) in groups.iter().enumerate() {
        let _ = writeln!(
            out,
            "\n{}\nLicense text {} (sha256 {hash})",
            "=".repeat(72),
            index + 1
        );
        out.push_str("Used by:\n");
        for user in users {
            let _ = writeln!(out, "  {user}");
        }
        out.push('\n');
        out.push_str(text);
    }
    out
}
