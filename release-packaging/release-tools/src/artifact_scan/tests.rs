use super::*;
use crate::{
    link_list::detect,
    release_inventory::{
        Cpu, DistributionChannel, DistributionRow, OsFamily, OsRange, OsVersion, OsVersionScope,
        Sha256Digest, SignatureMethod, StagedFile,
    },
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
    scan_package("pkg", format, tree, "diskunfold", LICENSE, NOTICES)
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

#[test]
fn a_tar_whose_only_top_level_entry_is_a_file_fails() -> TestResult {
    let tree = tempfile::tempdir()?;
    write(tree.path(), "only-a-file", b"x")?;
    let failure = scan(PackageFormat::TarGz, tree.path())
        .err()
        .ok_or("must fail")?;
    assert!(
        failure.detail.contains("exactly one top-level folder"),
        "{failure}"
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn a_tar_whose_only_top_level_entry_is_a_link_to_a_folder_fails() -> TestResult {
    let real = tree_for(PackageFormat::TarGz)?;
    let tree = tempfile::tempdir()?;
    let folder = fs::read_dir(real.path())?.next().ok_or("folder")??.path();
    std::os::unix::fs::symlink(&folder, tree.path().join("DiskUnfold-0.1.0-linux-x86_64"))?;
    let failure = scan(PackageFormat::TarGz, tree.path())
        .err()
        .ok_or("must fail")?;
    assert_eq!(failure.reason_key, "release.notice_mismatch");
    assert!(failure.detail.contains("not a link"), "{failure}");
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
fn links_that_leave_the_tree_are_recognized_lexically() {
    let dir =
        |parts: &[&str]| -> Vec<String> { parts.iter().map(|part| (*part).to_owned()).collect() };
    assert!(link_escapes(&dir(&[]), Path::new("../outside")));
    assert!(link_escapes(&dir(&["a"]), Path::new("../../outside")));
    assert!(link_escapes(
        &dir(&["a", "b"]),
        Path::new("x/../../../../out")
    ));
    assert!(link_escapes(&dir(&["a"]), Path::new("/etc/passwd")));
    assert!(!link_escapes(&dir(&["a", "b"]), Path::new("../../a/c")));
    assert!(!link_escapes(&dir(&["a"]), Path::new("./sibling")));
    assert!(!link_escapes(&dir(&[]), Path::new("inside/file")));
}

#[cfg(unix)]
#[test]
fn a_member_link_that_escapes_the_root_fails_and_an_inside_link_passes() -> TestResult {
    let tree = tree_for(PackageFormat::Msix)?;
    std::os::unix::fs::symlink("diskunfold", tree.path().join("inside-link"))?;
    scan(PackageFormat::Msix, tree.path())?;
    for target in ["../../../etc/passwd", "/etc/passwd"] {
        let tree = tree_for(PackageFormat::Msix)?;
        write(tree.path(), "usr/bin/placeholder", b"x")?;
        std::os::unix::fs::symlink(target, tree.path().join("usr/bin/escape"))?;
        let failure = scan(PackageFormat::Msix, tree.path())
            .err()
            .ok_or("must fail")?;
        assert_eq!(failure.reason_key, "release.license_denied", "{target}");
        assert!(failure.detail.contains("leaves the package"), "{failure}");
    }
    Ok(())
}

#[test]
fn a_missing_tree_is_an_error_not_an_empty_list() {
    let result = denied_members(Path::new("definitely/not/here"), "diskunfold");
    assert!(matches!(result, Err(ref failure) if failure.reason_key == "release.license_denied"));
}

#[cfg(unix)]
#[test]
fn an_unreadable_directory_fails_the_scan() -> TestResult {
    use std::os::unix::fs::PermissionsExt;
    let tree = tree_for(PackageFormat::Msix)?;
    let locked = tree.path().join("locked");
    fs::create_dir(&locked)?;
    fs::write(locked.join("hidden"), b"x")?;
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000))?;
    // A privileged user can still read the directory, so only assert when the OS refuses.
    let refused = fs::read_dir(&locked).is_err();
    let result = scan(PackageFormat::Msix, tree.path());
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755))?;
    if refused {
        let failure = result.err().ok_or("an unreadable directory must fail")?;
        assert_eq!(failure.reason_key, "release.license_denied");
        assert!(failure.detail.contains("cannot be read"), "{failure}");
    }
    Ok(())
}

#[test]
fn shared_libraries_libdbus_gpl_and_plain_copying_files_are_denied() -> TestResult {
    for member in [
        "usr/lib/libdbus-1.so.3",
        "lib/libwebkit.so",
        "COPYING.LESSER",
        "COPYING",
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
fn a_renamed_binary_is_found_by_its_magic_bytes() -> TestResult {
    for (name, magic) in [
        ("data.bin", &b"\x7fELFxxxx"[..]),
        ("icon.png", &[0xcf, 0xfa, 0xed, 0xfe, 0][..]),
        ("notes.txt", b"MZ\x90\x00"),
    ] {
        assert!(detect(magic).is_some(), "{name}");
        let tree = tree_for(PackageFormat::Msix)?;
        write(tree.path(), name, magic)?;
        let failure = scan(PackageFormat::Msix, tree.path())
            .err()
            .ok_or("must fail")?;
        assert_eq!(failure.reason_key, "release.license_denied", "{name}");
        assert!(
            failure.detail.contains("bundled executable or library"),
            "{failure}"
        );
    }
    // The executable of the package itself may be a binary.
    let tree = tree_for(PackageFormat::Msix)?;
    write(tree.path(), "diskunfold", b"\x7fELFxxxx")?;
    scan(PackageFormat::Msix, tree.path())?;
    Ok(())
}

#[test]
fn permissive_member_names_are_not_flagged() -> TestResult {
    let tree = tree_for(PackageFormat::Msix)?;
    write(tree.path(), "Assets/Square44x44Logo.png", b"\x89PNG")?;
    write(tree.path(), "usr/share/doc/diskunfold/copyright", b"x")?;
    assert!(denied_members(tree.path(), "diskunfold")?.is_empty());
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

fn manifest_for(rows: Vec<DistributionRow>, executables: &[&str]) -> ReleaseManifest {
    ReleaseManifest {
        product_name: "DiskUnfold".into(),
        version: "0.1.0".into(),
        staged_files: rows
            .iter()
            .zip(executables)
            .map(|(row, path)| StagedFile {
                artifact_file_name: row.file_name.clone(),
                kind: StagedFileKind::Executable,
                relative_path: (*path).into(),
            })
            .collect(),
        rows,
        channel_references: Vec::new(),
    }
}

fn opened_deb() -> Result<(TempDir, TempDir), Box<dyn std::error::Error>> {
    let extracted = tempfile::tempdir()?;
    let deb = extracted.path().join("a.deb");
    write(&deb, "usr/share/doc/diskunfold/LICENSE", LICENSE)?;
    write(
        &deb,
        "usr/share/doc/diskunfold/THIRD-PARTY-NOTICES.txt",
        NOTICES,
    )?;
    let staging = tempfile::tempdir()?;
    write(
        staging.path(),
        "unpacked/diskunfold",
        &crate::link_list::elf_fixture(&["libc.so.6", "libgtk-4.so.1"]),
    )?;
    Ok((extracted, staging))
}

#[test]
fn a_release_scan_requires_every_package_to_be_opened() -> TestResult {
    let (extracted, staging) = opened_deb()?;
    let one = manifest_for(
        vec![row("a.deb", PackageFormat::Deb)?],
        &["unpacked/diskunfold"],
    );
    assert_eq!(
        scan_release(&one, extracted.path(), staging.path(), LICENSE, NOTICES)?,
        1
    );

    let two = manifest_for(
        vec![
            row("a.deb", PackageFormat::Deb)?,
            row("b.tar.gz", PackageFormat::TarGz)?,
        ],
        &["unpacked/diskunfold", "unpacked/diskunfold"],
    );
    let failure = scan_release(&two, extracted.path(), staging.path(), LICENSE, NOTICES)
        .err()
        .ok_or("must fail")?;
    assert_eq!(failure.reason_key, "release.notice_mismatch");
    assert!(
        failure.detail.contains("b.tar.gz was not opened"),
        "{failure}"
    );
    Ok(())
}

#[test]
fn the_scan_reads_the_link_list_of_the_staged_executable() -> TestResult {
    let (extracted, staging) = opened_deb()?;
    let manifest = manifest_for(
        vec![row("a.deb", PackageFormat::Deb)?],
        &["unpacked/diskunfold"],
    );
    write(
        staging.path(),
        "unpacked/diskunfold",
        &crate::link_list::elf_fixture(&["libc.so.6", "libdbus-1.so.3"]),
    )?;
    let failure = scan_release(
        &manifest,
        extracted.path(),
        staging.path(),
        LICENSE,
        NOTICES,
    )
    .err()
    .ok_or("libdbus must be denied")?;
    assert_eq!(failure.reason_key, "release.license_denied");
    assert!(failure.detail.contains("libdbus-1.so.3"), "{failure}");

    // A row without a staged executable, or a path that leaves the staging root, cannot pass.
    let none = manifest_for(vec![row("a.deb", PackageFormat::Deb)?], &[]);
    assert!(scan_release(&none, extracted.path(), staging.path(), LICENSE, NOTICES).is_err());
    let escaping = manifest_for(vec![row("a.deb", PackageFormat::Deb)?], &["../outside"]);
    assert!(
        scan_release(
            &escaping,
            extracted.path(),
            staging.path(),
            LICENSE,
            NOTICES
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn package_file_names_must_be_plain_names() {
    for good in ["a.deb", "DiskUnfold-0.1.0-x64.msix"] {
        assert!(plain_file_name(good), "{good}");
    }
    for bad in ["", ".", "..", "a/b", "a\\b", "C:evil", "../x"] {
        assert!(!plain_file_name(bad), "{bad:?}");
    }
}
