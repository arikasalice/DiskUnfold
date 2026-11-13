//! Write one CycloneDX SBOM per released package (release stage 5, 고지·SBOM 대조).
//!
//! The component list is the same shipped-package inventory that generates THIRD-PARTY-NOTICES
//! (cargo metadata --locked and gui-web/package-lock.json).
use du_release_tools::{
    notices::shipped_packages,
    release_inventory::ReleaseManifest,
    sbom::{BuildTool, SbomSubject, render_cyclonedx, sbom_file_name},
};
use std::{env, error::Error, fs, io, path::PathBuf, process};

const USAGE: &str = "usage: build-sbom <merged-manifest.json> <repository-root> <output-directory>";

fn main() {
    if let Err(error) = run() {
        eprintln!("build-sbom: {error}");
        process::exit(1);
    }
}

fn pinned(repo: &std::path::Path, file: &str, key: Option<&str>) -> Result<String, Box<dyn Error>> {
    let text = fs::read_to_string(repo.join(file))?;
    let value = match key {
        Some(key) => text
            .lines()
            .find_map(|line| line.strip_prefix(key))
            .map(|rest| rest.trim().trim_matches('"').to_owned()),
        None => Some(text.trim().to_owned()),
    };
    value
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("{file} has no pinned version").into())
}

fn run() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().skip(1).collect();
    let [manifest_path, repo, out] = args.as_slice() else {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, USAGE).into());
    };
    let repo = PathBuf::from(repo);
    let manifest: ReleaseManifest = serde_json::from_str(&fs::read_to_string(manifest_path)?)?;
    let packages = shipped_packages(&repo)?;
    let rust = pinned(&repo, "rust-toolchain.toml", Some("channel = "))?;
    let node = pinned(&repo, ".node-version", None)?;
    let tools = [
        BuildTool {
            name: "rustc",
            version: &rust,
        },
        BuildTool {
            name: "node",
            version: node.trim_start_matches('v'),
        },
    ];
    let out = PathBuf::from(out);
    fs::create_dir_all(&out)?;
    let mut sums = String::new();
    for row in &manifest.rows {
        let subject = SbomSubject {
            product_name: &manifest.product_name,
            version: &manifest.version,
            file_name: &row.file_name,
            sha256: row.sha256.as_str(),
        };
        let text = render_cyclonedx(&subject, &packages, &tools)?;
        let name = sbom_file_name(&row.file_name);
        fs::write(out.join(&name), &text)?;
        sums.push_str(&format!("{}  {name}\n", sha256_hex(text.as_bytes())));
    }
    fs::write(out.join("SBOM-SHA256SUMS"), sums)?;
    println!("sboms={}", manifest.rows.len());
    Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
