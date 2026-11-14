//! Verifies a sample bundle header and digest, then prints its version and hash.
use du_release_tools::sample_bundle::verify_sample_bundle;
use std::{env, error::Error, fs, process};

fn main() {
    if let Err(error) = run() {
        eprintln!("check-sample-bundle: {error}");
        process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let path = env::args_os()
        .nth(1)
        .ok_or("usage: check-sample-bundle <bundle.v1.bin>")?;
    let record = verify_sample_bundle(&fs::read(path)?)?;
    println!("bundle_version={}", record.bundle_version);
    println!("blake3={}", record.blake3);
    Ok(())
}
