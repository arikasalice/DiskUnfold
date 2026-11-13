//! Generate the notices, support table and build-info resources for a build.
use du_release_tools::{debian_copyright, product_resources, support_table::Mode};
use std::{env, error::Error, fs, path::PathBuf, process};

const USAGE: &str = "usage: build-product-resources --out <dir> [--repo <dir>] [--source-commit <sha>] [--release] [--allow-blockers]\n       build-product-resources --debian-copyright <file> [--repo <dir>]";

fn main() {
    match run() {
        Ok(true) => {}
        Ok(false) => process::exit(1),
        Err(error) => {
            eprintln!("build-product-resources: {error}");
            process::exit(2);
        }
    }
}

fn run() -> Result<bool, Box<dyn Error>> {
    let mut out = None;
    let mut repo = PathBuf::from(".");
    let mut commit = None;
    let mut mode = Mode::Development;
    let mut allow_blockers = false;
    let mut debian = None;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out" => out = args.next().map(PathBuf::from),
            "--repo" => repo = args.next().map(PathBuf::from).ok_or(USAGE)?,
            "--source-commit" => commit = Some(args.next().ok_or(USAGE)?),
            "--release" => mode = Mode::Release,
            "--allow-blockers" => allow_blockers = true,
            "--debian-copyright" => debian = Some(args.next().map(PathBuf::from).ok_or(USAGE)?),
            _ => return Err(USAGE.into()),
        }
    }
    if let Some(path) = debian {
        if out.is_some() || commit.is_some() || mode == Mode::Release || allow_blockers {
            return Err(USAGE.into());
        }
        fs::write(path, debian_copyright::generate(&repo)?)?;
        return Ok(true);
    }
    let out = out.ok_or(USAGE)?;
    if mode == Mode::Release && allow_blockers {
        return Err("--allow-blockers cannot be used with --release".into());
    }
    let resources = product_resources::generate(&repo, commit, mode)?;
    fs::create_dir_all(&out)?;
    fs::write(out.join("THIRD-PARTY-NOTICES"), &resources.notices)?;
    fs::write(out.join("SUPPORT-TABLE.md"), &resources.support_markdown)?;
    let matrix = serde_json::to_string_pretty(&resources.support_matrix)?;
    fs::write(out.join("support-matrix.json"), matrix + "\n")?;
    let document = serde_json::to_string_pretty(&resources.json)?;
    fs::write(out.join("product-resources.json"), document + "\n")?;
    for blocker in &resources.blockers {
        eprintln!(
            "blocker: {} {}: {}",
            blocker.reason_key, blocker.subject, blocker.detail
        );
    }
    if resources.blockers.is_empty() || allow_blockers {
        Ok(true)
    } else {
        eprintln!("release gate failed; the resources are not suitable for an official build");
        Ok(false)
    }
}
