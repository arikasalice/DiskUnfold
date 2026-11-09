use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};
use syn::visit::Visit;

#[derive(Deserialize)]
struct Metadata {
    packages: Vec<Package>,
    workspace_members: Vec<String>,
}
#[derive(Deserialize)]
struct Package {
    id: String,
    name: String,
    manifest_path: PathBuf,
    dependencies: Vec<Dependency>,
}
#[derive(Deserialize)]
struct Dependency {
    name: String,
    kind: Option<String>,
    rename: Option<String>,
}
#[derive(Deserialize)]
struct Policy {
    crates: BTreeMap<String, Vec<String>>,
}
mod model;
use model::Report;

fn name(s: &str) -> &str {
    s.strip_prefix("du-").unwrap_or(s)
}
fn metadata(root: &Path) -> Result<Metadata, String> {
    let output = Command::new("cargo")
        .args(["metadata", "--locked", "--format-version", "1", "--no-deps"])
        .current_dir(root)
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    serde_json::from_slice(&output.stdout).map_err(|e| e.to_string())
}
fn dependencies(meta: &Metadata, policy: &Policy, report: &mut Report) {
    let internal: BTreeSet<_> = meta
        .packages
        .iter()
        .filter(|p| meta.workspace_members.contains(&p.id))
        .map(|p| p.name.as_str())
        .collect();
    for package in meta
        .packages
        .iter()
        .filter(|p| meta.workspace_members.contains(&p.id))
    {
        let source = name(&package.name);
        let allowed = policy.crates.get(source);
        if allowed.is_none() {
            report.add(
                "missing-crate-policy",
                package.manifest_path.display().to_string(),
                format!("{source} has no dependency policy"),
            );
        }
        let mut direct = BTreeSet::new();
        for dep in &package.dependencies {
            if !internal.contains(dep.name.as_str()) {
                continue;
            }
            let target = name(&dep.name);
            if target != "core-contracts" {
                direct.insert(target.to_string());
            }
            let kind = dep.kind.as_deref().unwrap_or("normal");
            if allowed.is_none_or(|list| !list.iter().any(|a| a == target)) {
                report.add(
                    "crate-dependency",
                    package.manifest_path.display().to_string(),
                    format!(
                        "{source} -> {target} ({kind}, alias {}) is not allowed",
                        dep.rename.as_deref().unwrap_or(&dep.name)
                    ),
                );
            }
            if target == "change-executor" && source != "app-host" {
                report.add(
                    "executor-boundary",
                    package.manifest_path.display().to_string(),
                    format!("{source} depends on change-executor ({kind})"),
                );
            }
        }
        report
            .metrics
            .crate_direct_dependencies
            .insert(source.into(), direct.len());
        if direct.len() > 9 {
            report.add(
                "dependency-count",
                source,
                format!(
                    "{} direct internal dependencies exceeds 9 (core-contracts excluded)",
                    direct.len()
                ),
            );
        }
    }
}
fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    if !dir.exists() {
        return Ok(());
    }
    let entries = fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for entry in entries {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.is_dir() {
            rust_files(&path, out)?;
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
    out.sort();
    Ok(())
}
fn module_for(path: &Path, src: &Path) -> Vec<String> {
    let Ok(relative) = path.strip_prefix(src) else {
        return Vec::new();
    };
    let mut parts: Vec<_> = relative
        .iter()
        .map(|x| x.to_string_lossy().into_owned())
        .collect();
    if let Some(last) = parts.last_mut() {
        *last = last.trim_end_matches(".rs").to_string();
    }
    if parts
        .last()
        .is_some_and(|s| s == "lib" || s == "main" || s == "mod")
    {
        parts.pop();
    }
    parts
}
mod scanner;
use scanner::Scan;

fn analyze(root: &Path) -> Result<Report, String> {
    let meta = metadata(root)?;
    let policy_text = fs::read_to_string(root.join("release-packaging/allowed-deps.toml"))
        .map_err(|e| e.to_string())?;
    let policy: Policy =
        toml::from_str(&policy_text).map_err(|e| format!("invalid allowed-deps.toml: {e}"))?;
    let mut report = Report {
        schema_version: 1,
        ..Report::default()
    };
    dependencies(&meta, &policy, &mut report);
    let mut modules = BTreeSet::new();
    let mut edges = BTreeSet::new();
    for package in meta
        .packages
        .iter()
        .filter(|p| meta.workspace_members.contains(&p.id))
    {
        let crate_name = name(&package.name);
        let Some(dir) = package.manifest_path.parent() else {
            continue;
        };
        let src = dir.join("src");
        let mut files = Vec::new();
        rust_files(&src, &mut files)?;
        for extra in ["tests", "examples", "benches"] {
            rust_files(&dir.join(extra), &mut files)?;
        }
        let build = dir.join("build.rs");
        if build.exists() {
            files.push(build);
        }
        for file in files {
            let content =
                fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
            let lines = content.lines().count();
            report.metrics.rust_files += 1;
            report.metrics.rust_lines += lines;
            report
                .metrics
                .file_lines
                .insert(file.display().to_string(), lines);
            if lines > 500 {
                report.add(
                    "file-lines",
                    file.display().to_string(),
                    format!("{lines} lines exceeds 500"),
                );
            }
            let module = if file.starts_with(&src) {
                module_for(&file, &src)
            } else {
                module_for(&file, dir)
            };
            for part in &module {
                if ["common", "util", "helpers", "misc"].contains(&part.as_str()) {
                    report.add(
                        "module-name",
                        file.display().to_string(),
                        format!("module name {part} is forbidden"),
                    );
                }
            }
            let id = format!("{}::{}", crate_name, module.join("::"));
            modules.insert(id);
            let parsed =
                syn::parse_file(&content).map_err(|e| format!("{}: {e}", file.display()))?;
            let mut scan = Scan {
                report: &mut report,
                crate_name,
                file: &file,
                module,
                edges: &mut edges,
                modules: &mut modules,
            };
            for item in &parsed.items {
                scan.visit_item(item);
            }
        }
    }
    // An imported symbol belongs to the longest declared module prefix.
    let resolved: BTreeSet<_> = edges
        .into_iter()
        .filter_map(|(from, mut to)| {
            loop {
                if modules.contains(&to) {
                    break;
                }
                match to.rfind("::") {
                    Some(i) if i > 0 => to.truncate(i),
                    _ => return None,
                }
            }
            if from == to { None } else { Some((from, to)) }
        })
        .collect();
    report.metrics.module_count = modules.len();
    report.metrics.module_edges = resolved.len();
    for (from, to) in &resolved {
        let mut seen = BTreeSet::new();
        let mut pending = vec![to.clone()];
        let mut cyclic = false;
        while let Some(node) = pending.pop() {
            if node == *from {
                cyclic = true;
                break;
            }
            if seen.insert(node.clone()) {
                pending.extend(
                    resolved
                        .iter()
                        .filter(|(a, _)| *a == node)
                        .map(|(_, b)| b.clone()),
                );
            }
        }
        if cyclic {
            report.add(
                "module-cycle",
                from,
                format!("module dependency {from} -> {to} participates in a cycle"),
            );
        }
    }
    report.diagnostics.sort_by(|a, b| {
        (&a.rule, &a.location, &a.message).cmp(&(&b.rule, &b.location, &b.message))
    });
    Ok(report)
}
fn main() {
    let root = env::args_os()
        .nth(1)
        .map_or_else(|| env::current_dir().unwrap_or_default(), PathBuf::from);
    match analyze(&root) {
        Ok(report) => {
            let output = root.join("quality-output/arch-report.json");
            let result = fs::create_dir_all(output.parent().unwrap_or(&root))
                .and_then(|()| serde_json::to_vec_pretty(&report).map_err(std::io::Error::other))
                .and_then(|data| fs::write(&output, data));
            if let Err(e) = result {
                eprintln!("cannot write {}: {e}", output.display());
                std::process::exit(2);
            }
            for diagnostic in &report.diagnostics {
                eprintln!(
                    "{}: {}: {}",
                    diagnostic.location, diagnostic.rule, diagnostic.message
                );
            }
            println!(
                "Architecture report: {} ({} violations)",
                output.display(),
                report.diagnostics.len()
            );
            if !report.diagnostics.is_empty() {
                std::process::exit(1);
            }
        }
        Err(error) => {
            eprintln!("architecture check failed: {error}");
            std::process::exit(2);
        }
    }
}

#[cfg(test)]
mod tests;
