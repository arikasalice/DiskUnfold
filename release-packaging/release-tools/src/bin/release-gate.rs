//! Release gate commands used by .github/workflows/release.yml and the local dry run.
//!
//! Results are `key=value` lines on stdout (safe to append to GITHUB_OUTPUT). A failed check
//! prints `reason_key: detail` on stderr and exits with 1; a usage error exits with 2.
use du_release_tools::{
    artifact_scan::scan_release,
    release_gate::{
        FormatVersions, FuzzGateInput, GateFailure, check_commits, check_format_versions,
        check_fuzz_gate, check_required_artifacts_scoped, check_signature_claims,
        check_tag_version, merge_manifests, parse_release_tag,
    },
    release_inventory::{Cpu, OsFamily, ReleaseManifest},
    sbom::check_sbom_set,
};
use std::{collections::BTreeMap, env, error::Error, fs, path::PathBuf, process};

const USAGE: &str = "usage:
  release-gate check-tag --tag T --workspace-version V --tag-commit C --checkout-commit C --event-commit C
  release-gate read-formats [--file FILE]
  release-gate merge-manifests --out FILE NAME=DIRECTORY...
  release-gate check-release-set --manifest FILE --signing-applied true|false [--scope FAMILY:CPU,...]
  (scope entries: windows|ubuntu|macos : x64|arm64|universal; default is the full release set)
  release-gate check-sboms --manifest FILE --sbom-dir DIR --notices FILE [--repo DIR]
  release-gate scan-artifacts --manifest FILE --extracted DIR --staging DIR --license FILE --notices FILE
  release-gate check-fuzz --issues FILE --registry FILE --runs FILE --now UNIX-SECONDS --commit SHA
      --require-run true|false [--max-age-days N] [--record-out FILE]";

enum Failure {
    Usage(String),
    Gate(GateFailure),
    Other(Box<dyn Error>),
}

impl From<GateFailure> for Failure {
    fn from(value: GateFailure) -> Self {
        Self::Gate(value)
    }
}

impl From<std::io::Error> for Failure {
    fn from(value: std::io::Error) -> Self {
        Self::Other(Box::new(value))
    }
}

impl From<serde_json::Error> for Failure {
    fn from(value: serde_json::Error) -> Self {
        Self::Other(Box::new(value))
    }
}

fn main() {
    match run() {
        Ok(()) => {}
        Err(Failure::Usage(message)) => {
            eprintln!("release-gate: {message}");
            process::exit(2);
        }
        Err(Failure::Gate(failure)) => {
            eprintln!("{failure}");
            process::exit(1);
        }
        Err(Failure::Other(error)) => {
            eprintln!("release-gate: {error}");
            process::exit(1);
        }
    }
}

fn usage(message: &str) -> Failure {
    Failure::Usage(format!("{message}\n{USAGE}"))
}

/// Splits `--name value` pairs from the remaining positional arguments.
fn options(args: Vec<String>) -> Result<(BTreeMap<String, String>, Vec<String>), Failure> {
    let mut named = BTreeMap::new();
    let mut positional = Vec::new();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        if let Some(name) = arg.strip_prefix("--") {
            let value = args
                .next()
                .ok_or_else(|| usage(&format!("--{name} needs a value")))?;
            if named.insert(name.to_owned(), value).is_some() {
                return Err(usage(&format!("--{name} given twice")));
            }
        } else {
            positional.push(arg);
        }
    }
    Ok((named, positional))
}

fn require(named: &BTreeMap<String, String>, name: &str) -> Result<String, Failure> {
    named
        .get(name)
        .cloned()
        .ok_or_else(|| usage(&format!("missing --{name}")))
}

/// Parses `windows:x64,ubuntu:arm64` into OS and CPU pairs.
fn parse_scope(value: &str) -> Result<Vec<(OsFamily, Cpu)>, Failure> {
    value
        .split(',')
        .map(|entry| {
            let (family, cpu) = entry
                .split_once(':')
                .ok_or_else(|| usage("scope entries must be FAMILY:CPU"))?;
            let family = match family {
                "windows" => OsFamily::Windows,
                "ubuntu" => OsFamily::Ubuntu,
                "macos" => OsFamily::Macos,
                _ => return Err(usage("unknown scope family")),
            };
            let cpu = match cpu {
                "x64" => Cpu::X64,
                "arm64" => Cpu::Arm64,
                "universal" => Cpu::Universal,
                _ => return Err(usage("unknown scope cpu")),
            };
            Ok((family, cpu))
        })
        .collect()
}

fn run() -> Result<(), Failure> {
    let mut args = env::args().skip(1);
    let command = args.next().ok_or_else(|| usage("missing command"))?;
    let (named, positional) = options(args.collect())?;
    match command.as_str() {
        "check-tag" => {
            let tag = parse_release_tag(&require(&named, "tag")?)?;
            check_tag_version(&tag, &require(&named, "workspace-version")?)?;
            check_commits(
                &require(&named, "tag-commit")?,
                &require(&named, "checkout-commit")?,
                &require(&named, "event-commit")?,
            )?;
            println!("version={}", tag.version());
        }
        "read-formats" => {
            // With --file the recorded versions must equal the versions defined in code.
            let formats = match named.get("file") {
                Some(file) => check_format_versions(&fs::read_to_string(file)?)?,
                None => FormatVersions::from_code(),
            };
            println!("DU_APP_DB_FORMAT={}", formats.app_db);
            println!("DU_SNAPSHOT_FORMAT={}", formats.snapshot);
        }
        "merge-manifests" => {
            let out = PathBuf::from(require(&named, "out")?);
            let mut parts = Vec::new();
            for part in positional {
                let (name, dir) = part
                    .split_once('=')
                    .ok_or_else(|| usage("parts must be NAME=DIRECTORY"))?;
                let text = fs::read_to_string(PathBuf::from(dir).join("manifest.json"))?;
                let manifest: ReleaseManifest = serde_json::from_str(&text)?;
                parts.push((name.to_owned(), manifest));
            }
            let merged = merge_manifests(parts)?;
            fs::write(&out, serde_json::to_string_pretty(&merged)? + "\n")?;
            println!("rows={}", merged.rows.len());
        }
        "check-release-set" => {
            let text = fs::read_to_string(require(&named, "manifest")?)?;
            let manifest: ReleaseManifest = serde_json::from_str(&text)?;
            let applied = match require(&named, "signing-applied")?.as_str() {
                "true" => true,
                "false" => false,
                _ => return Err(usage("--signing-applied must be true or false")),
            };
            check_signature_claims(&manifest, applied)?;
            let scope = named
                .get("scope")
                .map(|value| parse_scope(value))
                .transpose()?;
            for warning in check_required_artifacts_scoped(&manifest, scope.as_deref())? {
                eprintln!("warning: {warning}");
                println!("warning={}", warning.reason_key);
            }
            println!("rows={}", manifest.rows.len());
        }
        _ => return run_stage_command(&command, &named),
    }
    Ok(())
}

/// Reads a merged manifest or a `release-inventory.json`; the inventory's `fuzz_gate` record is
/// not part of the manifest and is ignored here.
fn read_manifest(named: &BTreeMap<String, String>) -> Result<ReleaseManifest, Failure> {
    let mut value: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(require(named, "manifest")?)?)?;
    if let Some(object) = value.as_object_mut() {
        object.remove("fuzz_gate");
    }
    Ok(serde_json::from_value(value)?)
}

/// The release stages that check generated documents and opened packages.
fn run_stage_command(command: &str, named: &BTreeMap<String, String>) -> Result<(), Failure> {
    match command {
        "check-sboms" => {
            let notices = fs::read_to_string(require(named, "notices")?)?;
            let dir = PathBuf::from(require(named, "sbom-dir")?);
            let repo = named.get("repo").map(PathBuf::from);
            let checked = check_sbom_set(&read_manifest(named)?, &dir, &notices, repo.as_deref())?;
            println!("sboms={checked}");
        }
        "scan-artifacts" => {
            let license = fs::read(require(named, "license")?)?;
            let notices = fs::read(require(named, "notices")?)?;
            let extracted = PathBuf::from(require(named, "extracted")?);
            let staging = PathBuf::from(require(named, "staging")?);
            let scanned = scan_release(
                &read_manifest(named)?,
                &extracted,
                &staging,
                &license,
                &notices,
            )?;
            println!("packages_scanned={scanned}");
        }
        "check-fuzz" => run_fuzz_gate(named)?,
        _ => return Err(usage("unknown command")),
    }
    Ok(())
}

fn run_fuzz_gate(named: &BTreeMap<String, String>) -> Result<(), Failure> {
    let number = |name: &str, default: Option<i64>| -> Result<i64, Failure> {
        match named.get(name) {
            Some(value) => value
                .parse()
                .map_err(|_| usage(&format!("--{name} must be a number"))),
            None => default.ok_or_else(|| usage(&format!("missing --{name}"))),
        }
    };
    let require_run = match require(named, "require-run")?.as_str() {
        "true" => true,
        "false" => false,
        _ => return Err(usage("--require-run must be true or false")),
    };
    let issues = fs::read_to_string(require(named, "issues")?)?;
    let registry = fs::read_to_string(require(named, "registry")?)?;
    let runs = fs::read_to_string(require(named, "runs")?)?;
    let commit = require(named, "commit")?;
    let record = check_fuzz_gate(&FuzzGateInput {
        issues_json: &issues,
        registry_text: &registry,
        runs_json: &runs,
        now_unix: number("now", None)?,
        release_commit: &commit,
        require_run,
        max_age_days: number("max-age-days", Some(3))?,
    })?;
    println!("open_fuzz_crashes=0");
    println!("fuzz_targets={}", record.registered_targets);
    println!("fuzz_outcome={}", record.outcome);
    for note in &record.notes {
        println!("::warning::{note}");
    }
    if let Some(out) = named.get("record-out") {
        fs::write(out, serde_json::to_string_pretty(&record)? + "\n")?;
    }
    Ok(())
}
