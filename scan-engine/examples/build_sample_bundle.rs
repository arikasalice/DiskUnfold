//! Builds `assets/sample/bundle.v1.bin` from the authored `bundle.v1.json`.
//!
//! Run `cargo run --locked -p du-scan-engine --example build_sample_bundle` to regenerate the
//! binary, or add `-- --check` to fail when the committed binary differs from the source.
use std::{env, error::Error, fs, path::Path, process::ExitCode};

use du_scan_engine::sample_bundle::format_v1::build_bundle;

const BUNDLE_VERSION: &str = "1.0.0";

fn main() -> Result<ExitCode, Box<dyn Error>> {
    let assets = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("assets")
        .join("sample");
    let source = fs::read(assets.join("bundle.v1.json"))?;
    let built = build_bundle(BUNDLE_VERSION, &source).map_err(|_| "bundle.v1.json is invalid")?;
    let target = assets.join("bundle.v1.bin");
    if env::args().any(|argument| argument == "--check") {
        if fs::read(&target)? == built {
            return Ok(ExitCode::SUCCESS);
        }
        eprintln!("bundle.v1.bin does not match bundle.v1.json; rerun without --check");
        return Ok(ExitCode::FAILURE);
    }
    fs::write(&target, &built)?;
    println!("wrote {} bytes to {}", built.len(), target.display());
    Ok(ExitCode::SUCCESS)
}
