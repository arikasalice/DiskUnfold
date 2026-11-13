use super::*;
use crate::release_inventory::{
    Cpu, DistributionChannel, DistributionRow, OsFamily, OsRange, OsVersion, OsVersionScope,
    Sha256Digest, SignatureMethod,
};
use tempfile::TempDir;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const LICENSE: &[u8] = b"MIT License\n";
const NOTICES: &[u8] = b"DiskUnfold THIRD-PARTY-NOTICES\ncargo a 1.0.0: MIT\n";

fn write(root: &Path, relative: &str, bytes: &[u8]) -> TestResult {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, bytes)?;
    Ok(())
}

fn tree_for(format: PackageFormat) -> Result<TempDir, Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let prefix = match format {
        PackageFormat::TarGz => "DiskUnfold-0.1.0-linux-x86_64/",
        PackageFormat::Deb => "usr/share/doc/diskunfold/",
        _ => "",
    };
    write(dir.path(), &format!("{prefix}{LICENSE_FILE}"), LICENSE)?;
    write(dir.path(), &format!("{prefix}{NOTICES_FILE}"), NOTICES)?;
    write(dir.path(), &format!("{prefix}diskunfold"), b"exe")?;
    Ok(dir)
}

fn scan(format: PackageFormat, tree: &Path) -> Result<(), GateFailure> {
    scan_package("pkg", format, tree, LICENSE, NOTICES)
}

const FORMATS: [PackageFormat; 5] = [
    PackageFormat::TarGz,
    PackageFormat::Deb,
    PackageFormat::Msix,
    PackageFormat::Dmg,
    PackageFormat::PortableExe,
];

#[test]
fn every_format_with_both_documents_passes() -> TestResult {
    for format in FORMATS {
        let tree = tree_for(format)?;
        scan(format, tree.path())?;
    }
    Ok(())
}

#[test]
fn a_missing_document_fails_for_every_format() -> TestResult {
    for format in FORMATS {
        for name in [LICENSE_FILE, NOTICES_FILE] {
            let tree = tree_for(format)?;
            let dir = document_dir(format, tree.path())?;
            fs::remove_file(dir.join(name))?;
            let failure = scan(format, tree.path()).err().ok_or("must fail")?;
            assert_eq!(
                failure.reason_key, "release.notice_mismatch",
                "{format:?} {name}"
            );
            assert!(failure.detail.contains(name), "{failure}");
        }
    }
    Ok(())
}

#[test]
fn a_one_byte_difference_fails() -> TestResult {
    for format in FORMATS {
        let tree = tree_for(format)?;
        let dir = document_dir(format, tree.path())?;
        let mut changed = NOTICES.to_vec();
        changed.push(b'\n');
        fs::write(dir.join(NOTICES_FILE), changed)?;
        let failure = scan(format, tree.path()).err().ok_or("must fail")?;
        assert_eq!(failure.reason_key, "release.notice_mismatch");
        assert!(failure.detail.contains("differs"), "{failure}");
    }
    Ok(())
}

#[test]
fn a_tar_with_two_top_level_entries_fails() -> TestResult {
    let tree = tree_for(PackageFormat::TarGz)?;
    write(tree.path(), "extra/file", b"x")?;
    assert!(scan(PackageFormat::TarGz, tree.path()).is_err());
    Ok(())
}

#[cfg(unix)]
#[test]
fn a_symbolic_link_in_place_of_a_document_fails() -> TestResult {
    let tree = tree_for(PackageFormat::Msix)?;
    let outside = tempfile::tempdir()?;
    write(outside.path(), "real", NOTICES)?;
    fs::remove_file(tree.path().join(NOTICES_FILE))?;
    std::os::unix::fs::symlink(outside.path().join("real"), tree.path().join(NOTICES_FILE))?;
    let failure = scan(PackageFormat::Msix, tree.path())
        .err()
        .ok_or("must fail")?;
    assert!(failure.detail.contains("not a regular file"), "{failure}");
    Ok(())
}

#[test]
fn shared_libraries_libdbus_and_gpl_files_are_denied() -> TestResult {
    for member in [
        "usr/lib/libdbus-1.so.3",
        "lib/libwebkit.so",
        "COPYING.LESSER",
        "licenses/GPL-3.0.txt",
        "LGPL-2.1",
    ] {
        let tree = tree_for(PackageFormat::Msix)?;
        write(tree.path(), member, b"x")?;
        let failure = scan(PackageFormat::Msix, tree.path())
            .err()
            .ok_or("must fail")?;
        assert_eq!(failure.reason_key, "release.license_denied", "{member}");
    }
    Ok(())
}

#[test]
fn permissive_member_names_are_not_flagged() -> TestResult {
    let tree = tree_for(PackageFormat::Msix)?;
    write(tree.path(), "Assets/Square44x44Logo.png", b"x")?;
    write(tree.path(), "usr/share/doc/diskunfold/copyright", b"x")?;
    assert!(copyleft_members(tree.path()).is_empty());
    Ok(())
}

#[test]
fn msi_has_no_scan_layout_and_fails_closed() -> TestResult {
    let tree = tree_for(PackageFormat::Msix)?;
    assert!(scan(PackageFormat::Msi, tree.path()).is_err());
    Ok(())
}

fn row(name: &str, format: PackageFormat) -> Result<DistributionRow, Box<dyn std::error::Error>> {
    let digest = Sha256Digest::parse(&"b".repeat(64))?;
    Ok(DistributionRow {
        file_name: name.into(),
        version: "0.1.0".into(),
        os: OsRange {
            family: OsFamily::Ubuntu,
            versions: OsVersionScope::Range {
                minimum: OsVersion::parse("24.04")?,
                maximum: None,
            },
        },
        cpu: Cpu::X64,
        format,
        channel: DistributionChannel::Direct,
        signature: SignatureMethod::Unsigned,
        signature_scope: "none".into(),
        sha256: digest.clone(),
        executable_file_name: "diskunfold".into(),
        executable_sha256: digest,
        run_instructions: "run".into(),
        sbom_file_name: None,
    })
}

#[test]
fn a_release_scan_requires_every_package_to_be_opened() -> TestResult {
    let extracted = tempfile::tempdir()?;
    let deb = extracted.path().join("a.deb");
    write(&deb, "usr/share/doc/diskunfold/LICENSE", LICENSE)?;
    write(
        &deb,
        "usr/share/doc/diskunfold/THIRD-PARTY-NOTICES.txt",
        NOTICES,
    )?;
    let manifest = ReleaseManifest {
        product_name: "DiskUnfold".into(),
        version: "0.1.0".into(),
        rows: vec![row("a.deb", PackageFormat::Deb)?],
        channel_references: Vec::new(),
        staged_files: Vec::new(),
    };
    assert_eq!(
        scan_release(&manifest, extracted.path(), LICENSE, NOTICES)?,
        1
    );

    let mut two = manifest.clone();
    two.rows.push(row("b.tar.gz", PackageFormat::TarGz)?);
    let failure = scan_release(&two, extracted.path(), LICENSE, NOTICES)
        .err()
        .ok_or("must fail")?;
    assert_eq!(failure.reason_key, "release.notice_mismatch");
    assert!(
        failure.detail.contains("b.tar.gz was not opened"),
        "{failure}"
    );
    Ok(())
}
