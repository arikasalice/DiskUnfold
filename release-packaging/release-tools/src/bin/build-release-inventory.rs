//! Build local release inventory and checksum files from one manifest.
use du_release_tools::release_inventory::{
    ReleaseManifest, prepare_inventory, render_checksums, render_json, render_markdown,
};
use std::{env, error::Error, fs, io, path::PathBuf, process};

fn main() {
    if let Err(error) = run() {
        eprintln!("build-release-inventory: {error}");
        process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let mut args = env::args_os().skip(1);
    let Some(manifest_path) = args.next().map(PathBuf::from) else {
        return Err(usage().into());
    };
    let Some(staging_root) = args.next().map(PathBuf::from) else {
        return Err(usage().into());
    };
    let Some(output_dir) = args.next().map(PathBuf::from) else {
        return Err(usage().into());
    };
    if args.next().is_some() {
        return Err(usage().into());
    }

    let manifest_text = fs::read_to_string(&manifest_path)?;
    let manifest: ReleaseManifest = serde_json::from_str(&manifest_text)?;
    let (inventory, verified) = prepare_inventory(manifest, &staging_root)?;
    let json = render_json(&inventory)?;
    let markdown = render_markdown(&inventory);
    let checksums = render_checksums(&verified);

    fs::create_dir_all(&output_dir)?;
    write_output(&output_dir.join("release-inventory.json"), json.as_bytes())?;
    write_output(&output_dir.join("DISTRIBUTION.md"), markdown.as_bytes())?;
    write_output(&output_dir.join("SHA256SUMS"), checksums.as_bytes())?;
    Ok(())
}

fn write_output(path: &std::path::Path, contents: &[u8]) -> io::Result<()> {
    fs::write(path, contents)
}

fn usage() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "usage: build-release-inventory <manifest.json> <staging-root> <output-directory>",
    )
}
