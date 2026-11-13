//! Write one CycloneDX SBOM per released package (release stage 5, 고지·SBOM 대조).
//!
//! Each SBOM lists the packages that a build for the package's Rust target links: cargo metadata
//! (--locked, --filter-platform) confirmed by cargo tree --target, and the npm lockfile.
use du_release_tools::{
    notices::NoticePackage,
    release_inventory::ReleaseManifest,
    sbom::{
        BuildTool, SbomSubject, expected::target_packages, render_cyclonedx, rust_targets,
        sbom_file_name,
    },
};
use std::{collections::BTreeMap, env, error::Error, fs, io, path::PathBuf, process};

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
    // One cargo metadata run per distinct target set.
    let mut by_targets: BTreeMap<String, Vec<NoticePackage>> = BTreeMap::new();
    for row in &manifest.rows {
        let targets = rust_targets(row.os.family, row.cpu)
            .ok_or_else(|| format!("{} has no Rust target", row.file_name))?;
        let key = targets.join(",");
        if !by_targets.contains_key(&key) {
            by_targets.insert(key.clone(), target_packages(&repo, targets)?);
        }
        let subject = SbomSubject {
            product_name: &manifest.product_name,
            version: &manifest.version,
            file_name: &row.file_name,
            sha256: row.sha256.as_str(),
            rust_targets: targets,
            bundled: &row.bundled_components,
        };
        let text = render_cyclonedx(&subject, &by_targets[&key], &tools)?;
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
