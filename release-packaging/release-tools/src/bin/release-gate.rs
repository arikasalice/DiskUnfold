//! Release gate commands used by .github/workflows/release.yml and the local dry run.
//!
//! Results are `key=value` lines on stdout (safe to append to GITHUB_OUTPUT). A failed check
//! prints `reason_key: detail` on stderr and exits with 1; a usage error exits with 2.
use du_release_tools::{
    release_gate::{
        GateFailure, check_commits, check_required_artifacts_scoped, check_signature_claims,
        check_tag_version, merge_manifests, parse_format_versions, parse_release_tag,
    },
    release_inventory::{Cpu, OsFamily, ReleaseManifest},
};
use std::{collections::BTreeMap, env, error::Error, fs, path::PathBuf, process};

const USAGE: &str = "usage:
  release-gate check-tag --tag T --workspace-version V --tag-commit C --checkout-commit C --event-commit C
  release-gate read-formats --file FILE
  release-gate merge-manifests --out FILE NAME=DIRECTORY...
  release-gate check-release-set --manifest FILE --signing-applied true|false [--scope FAMILY:CPU,...]
  (scope entries: windows|ubuntu|macos : x64|arm64|universal; default is the full release set)";

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
            let text = fs::read_to_string(require(&named, "file")?)?;
            let formats = parse_format_versions(&text)?;
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
        _ => return Err(usage("unknown command")),
    }
    Ok(())
}
