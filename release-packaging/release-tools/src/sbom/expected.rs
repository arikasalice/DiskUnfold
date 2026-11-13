//! The expected component set for a target, derived from `cargo tree` and `npm ls`.
//!
//! The SBOM and THIRD-PARTY-NOTICES are both written from `cargo metadata` and the npm lockfile.
//! These commands walk the dependency graph with different code, so a package that one source
//! drops shows up as a difference instead of being missing from both documents.

use super::ComponentId;
use crate::notices::{Ecosystem, NoticePackage, shipped_packages_for};
use serde_json::Value;
use std::{collections::BTreeSet, path::Path, process::Command};

/// Reads registry crates from `cargo tree --prefix none --format {p}` output. Path and git
/// packages (first-party crates) and the repeat marker `(*)` are not components.
pub fn parse_cargo_tree(text: &str) -> BTreeSet<ComponentId> {
    let mut ids = BTreeSet::new();
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let (Some(name), Some(version)) = (parts.next(), parts.next()) else {
            continue;
        };
        let Some(version) = version.strip_prefix('v') else {
            continue;
        };
        let local =
            parts.any(|part| part.starts_with('(') && part != "(*)" && part != "(proc-macro)");
        if !local {
            ids.insert(("cargo".to_owned(), name.to_owned(), version.to_owned()));
        }
    }
    ids
}

fn collect_npm(node: &Value, ids: &mut BTreeSet<ComponentId>) {
    let Some(dependencies) = node["dependencies"].as_object() else {
        return;
    };
    for (name, child) in dependencies {
        if let Some(version) = child["version"].as_str() {
            ids.insert(("npm".to_owned(), name.clone(), version.to_owned()));
        }
        collect_npm(child, ids);
    }
}

/// Reads production packages from `npm ls --omit=dev --all --json` output.
///
/// # Errors
/// Returns a message when the text is not JSON.
pub fn parse_npm_ls(text: &str) -> Result<BTreeSet<ComponentId>, String> {
    let value: Value =
        serde_json::from_str(text).map_err(|error| format!("npm ls output: {error}"))?;
    let mut ids = BTreeSet::new();
    collect_npm(&value, &mut ids);
    Ok(ids)
}

fn run(mut command: Command, label: &str) -> Result<Vec<u8>, String> {
    let output = command
        .output()
        .map_err(|error| format!("start {label}: {error}"))?;
    // npm ls exits 1 for tree problems but still prints the tree, so only cargo's status counts.
    if label.starts_with("cargo") && !output.status.success() {
        return Err(format!(
            "{label} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(output.stdout)
}

/// Derives the expected components for a package built for `targets` (the union over the
/// targets): registry crates from `cargo tree --target` for the launcher plus the production npm
/// packages of the lockfile from `npm ls --package-lock-only`.
///
/// # Errors
/// Returns a message when cargo or npm cannot be run or their output cannot be read.
pub fn expected_components(repo: &Path, targets: &[&str]) -> Result<BTreeSet<ComponentId>, String> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut ids = BTreeSet::new();
    for target in targets {
        let mut command = Command::new(&cargo);
        command.current_dir(repo).args([
            "tree",
            "--locked",
            "-p",
            "du-launcher",
            "--target",
            target,
            "-e",
            "normal",
            "--prefix",
            "none",
            "--format",
            "{p}",
        ]);
        let stdout = run(command, &format!("cargo tree --target {target}"))?;
        ids.extend(parse_cargo_tree(&String::from_utf8_lossy(&stdout)));
    }
    let npm = if cfg!(windows) { "npm.cmd" } else { "npm" };
    let mut command = Command::new(npm);
    command.current_dir(repo.join("gui-web")).args([
        "ls",
        "--omit=dev",
        "--all",
        "--json",
        "--package-lock-only",
    ]);
    let stdout = run(command, "npm ls")?;
    ids.extend(parse_npm_ls(&String::from_utf8_lossy(&stdout))?);
    Ok(ids)
}

/// The packages an SBOM for `targets` lists: the metadata closure of each target (names,
/// versions, license expressions) kept only where `cargo tree --target` confirms the crate is in
/// the build graph. `cargo metadata --filter-platform` keeps an edge when any of its `cfg`
/// conditions might hold (for example `rustix_use_libc`), so it lists crates that no build of the
/// target links; `cargo tree` evaluates the same conditions the build does.
///
/// # Errors
/// Returns a message when cargo, npm or the lockfile cannot be read.
pub fn target_packages(repo: &Path, targets: &[&str]) -> Result<Vec<NoticePackage>, String> {
    let confirmed = expected_components(repo, targets)?;
    let mut packages = Vec::new();
    for triple in targets {
        packages.extend(
            shipped_packages_for(repo, Some(triple))?
                .into_iter()
                .filter(|package| {
                    package.ecosystem == Ecosystem::Npm
                        || confirmed.contains(&(
                            "cargo".to_owned(),
                            package.name.clone(),
                            package.version.clone(),
                        ))
                }),
        );
    }
    Ok(packages)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cargo_tree_keeps_registry_crates_only() {
        let text = "du-launcher v0.1.0 (D:\\repo\\launcher)\nserde v1.0.1\nserde v1.0.1 (*)\nserde_derive v1.0.1 (proc-macro)\ngit-dep v0.1.0 (https://example.com/x#abc)\ntoml_edit v0.25.19+spec-1.1.0\n";
        let ids = parse_cargo_tree(text);
        let names: Vec<&str> = ids.iter().map(|id| id.1.as_str()).collect();
        assert_eq!(names, ["serde", "serde_derive", "toml_edit"]);
        assert!(ids.contains(&(
            "cargo".into(),
            "toml_edit".into(),
            "0.25.19+spec-1.1.0".into()
        )));
    }

    #[test]
    fn npm_ls_collects_nested_production_packages() -> Result<(), String> {
        let text = r#"{"name":"gui","version":"0.1.0","dependencies":{"react":{"version":"19.0.0","dependencies":{"scheduler":{"version":"0.1.0"}}},"@scope/w":{"version":"2.0.0"},"gone":{"missing":true}}}"#;
        let ids = parse_npm_ls(text)?;
        assert_eq!(ids.len(), 3);
        assert!(ids.contains(&("npm".into(), "@scope/w".into(), "2.0.0".into())));
        assert!(parse_npm_ls("not json").is_err());
        Ok(())
    }
}
