use sha2::{Digest, Sha256};
use std::{env, fs, io, path::Path};

fn main() -> io::Result<()> {
    if env::var_os("CARGO_FEATURE_GUI").is_none() {
        return Ok(());
    }
    let dist = Path::new("../gui-web/dist")
        .canonicalize()
        .map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("Build gui-web before the native shell: {error}"),
            )
        })?;
    if !dist.join("index.html").is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "gui-web/dist/index.html",
        ));
    }
    println!("cargo:rerun-if-changed=../gui-web/dist");
    let mut entries = Vec::new();
    collect(&dist, &dist, &mut entries)?;
    entries.sort();
    let out = env::var_os("OUT_DIR").ok_or(io::Error::other("missing OUT_DIR"))?;
    let mut bytes = Vec::new();
    let mut manifest = String::new();
    for (url, mime, path) in entries {
        let start = bytes.len();
        bytes.extend(fs::read(path)?);
        manifest.push_str(&format!("/{url}\t{mime}\t{start}\t{}\n", bytes.len()));
    }
    fs::write(Path::new(&out).join("assets.txt"), manifest)?;
    // The startup check recomputes this digest to detect a damaged asset store.
    let digest: String = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    fs::write(Path::new(&out).join("assets.sha256"), digest)?;
    fs::write(Path::new(&out).join("assets.bin"), bytes)
}

fn collect(
    root: &Path,
    dir: &Path,
    entries: &mut Vec<(String, &'static str, std::path::PathBuf)>,
) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let kind = entry.file_type()?;
        if kind.is_dir() {
            collect(root, &path, entries)?;
            continue;
        }
        if !kind.is_file() {
            return Err(io::Error::other(format!(
                "Invalid GUI asset: {}",
                path.display()
            )));
        }
        let mime = match path.extension().and_then(|s| s.to_str()) {
            Some("html") => "text/html; charset=utf-8",
            Some("js") => "text/javascript; charset=utf-8",
            Some("css") => "text/css; charset=utf-8",
            Some("svg") => "image/svg+xml",
            Some("png") => "image/png",
            _ => {
                return Err(io::Error::other(format!(
                    "Unsupported GUI asset: {}",
                    path.display()
                )));
            }
        };
        let relative = path.strip_prefix(root).map_err(io::Error::other)?;
        let url = relative
            .to_str()
            .ok_or(io::Error::other("Non-UTF-8 asset name"))?
            .replace('\\', "/");
        if !url
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-._/".contains(&b))
        {
            return Err(io::Error::other(format!("Invalid GUI asset name: {url}")));
        }
        entries.push((url, mime, path));
    }
    Ok(())
}
