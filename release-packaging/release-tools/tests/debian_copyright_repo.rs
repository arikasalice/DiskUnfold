//! The committed Debian copyright file must equal the generator output.

use std::{error::Error, fs, path::PathBuf};

use du_release_tools::debian_copyright;

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn committed_debian_copyright_matches_the_generator() -> Result<(), Box<dyn Error>> {
    assert!(
        repo().join("gui-web/node_modules").is_dir(),
        "run npm ci --ignore-scripts in gui-web so npm license files are read"
    );
    let generated = debian_copyright::generate(&repo())?;
    let committed = fs::read_to_string(repo().join("release-packaging/linux/debian-copyright"))?
        .replace("\r\n", "\n");
    assert!(
        committed == generated,
        "release-packaging/linux/debian-copyright is stale: run build-product-resources --debian-copyright <path> and review the diff"
    );
    Ok(())
}

#[test]
fn generated_file_satisfies_the_packager_and_the_release_gate() -> Result<(), Box<dyn Error>> {
    let text = debian_copyright::generate(&repo())?;
    let lower = text.to_ascii_lowercase();
    // package-linux.sh requires these fields; release.yml rejects these words.
    assert!(text.contains("\nSource: https://github.com/arikasalice/DiskUnfold\n"));
    assert!(text.contains("\nUpstream-Contact: DiskUnfold Maintainers <arikasalice@gmail.com>\n"));
    assert!(!lower.contains("fixture") && !lower.contains("dry run"));
    Ok(())
}
