use super::*;
use crate::release_inventory::{
    DistributionChannel, DistributionRow, OsRange, OsVersion, OsVersionScope, Sha256Digest,
    StagedFileKind,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const COMMIT_A: &str = "0123456789abcdef0123456789abcdef01234567";
const COMMIT_B: &str = "89abcdef0123456789abcdef0123456789abcdef";

fn row(
    name: &str,
    family: OsFamily,
    cpu: Cpu,
    format: PackageFormat,
    signature: SignatureMethod,
) -> Result<DistributionRow, Box<dyn std::error::Error>> {
    let digest = Sha256Digest::parse(&"a".repeat(64))?;
    Ok(DistributionRow {
        file_name: name.into(),
        version: "0.1.0".into(),
        os: OsRange {
            family,
            versions: OsVersionScope::Range {
                minimum: OsVersion::parse("11 25H2")?,
                maximum: None,
            },
        },
        cpu,
        format,
        channel: DistributionChannel::Direct,
        signature,
        signature_scope: "scope".into(),
        sha256: digest.clone(),
        executable_file_name: "diskunfold".into(),
        executable_sha256: digest,
        run_instructions: "run".into(),
        sbom_file_name: None,
        bundled_components: if format == PackageFormat::Msi {
            vec![crate::release_inventory::BundledComponent {
                name: crate::release_inventory::WEBVIEW2_COMPONENT.into(),
                version: "1.3.315.17".into(),
                sha256: Sha256Digest::parse(&"b".repeat(64))?,
                signer: "CN=Microsoft Corporation, O=Microsoft Corporation".into(),
                source: "https://go.microsoft.com/fwlink/?linkid=2124701".into(),
            }]
        } else {
            Vec::new()
        },
    })
}

fn manifest(rows: Vec<DistributionRow>) -> ReleaseManifest {
    let staged_files = rows
        .iter()
        .map(|row| StagedFile {
            artifact_file_name: row.file_name.clone(),
            kind: StagedFileKind::Package,
            relative_path: PathBuf::from(&row.file_name),
        })
        .collect();
    ReleaseManifest {
        product_name: "DiskUnfold".into(),
        version: "0.1.0".into(),
        rows,
        channel_references: Vec::new(),
        staged_files,
    }
}

fn full_set(signature: SignatureMethod) -> Result<ReleaseManifest, Box<dyn std::error::Error>> {
    let mut rows = Vec::new();
    for (index, (family, cpu, format)) in REQUIRED.iter().enumerate() {
        rows.push(row(
            &format!("pkg-{index}"),
            *family,
            *cpu,
            *format,
            signature,
        )?);
    }
    Ok(manifest(rows))
}

#[test]
fn accepts_only_stable_three_part_tags() -> TestResult {
    assert_eq!(parse_release_tag("v0.1.0")?.version(), "0.1.0");
    assert_eq!(parse_release_tag("v10.20.30")?.version(), "10.20.30");
    for bad in [
        "",
        "0.1.0",
        "v0.1",
        "v0.1.0.1",
        "v01.1.0",
        "v0.1.0-rc.1",
        "v0.1.0+b",
        "V0.1.0",
        "refs/tags/v0.1.0",
        "v0.1.x",
        "v-1.0.0",
        "v0.1.0 ",
        "v1..0",
    ] {
        let failure = parse_release_tag(bad).err().ok_or(bad)?;
        assert_eq!(failure.reason_key, TAG_FORMAT_INVALID, "{bad}");
    }
    Ok(())
}

#[test]
fn tag_version_must_match_workspace_version() -> TestResult {
    let tag = parse_release_tag("v0.1.0")?;
    check_tag_version(&tag, "0.1.0")?;
    let failure = check_tag_version(&tag, "0.1.1").err().ok_or("accepted")?;
    assert_eq!(failure.reason_key, TAG_VERSION_MISMATCH);
    Ok(())
}

#[test]
fn tag_commit_checkout_and_event_must_be_identical_full_commits() -> TestResult {
    check_commits(COMMIT_A, COMMIT_A, COMMIT_A)?;
    for (tag, head, event) in [
        (COMMIT_A, COMMIT_B, COMMIT_A),
        (COMMIT_A, COMMIT_A, COMMIT_B),
        (COMMIT_B, COMMIT_A, COMMIT_A),
        ("0123456", "0123456", "0123456"),
        ("", COMMIT_A, COMMIT_A),
        (&COMMIT_A.to_uppercase(), COMMIT_A, COMMIT_A),
    ] {
        let failure = check_commits(tag, head, event).err().ok_or("accepted")?;
        assert_eq!(failure.reason_key, TAG_COMMIT_MISMATCH);
    }
    Ok(())
}

#[test]
fn format_versions_must_be_positive_integers() -> TestResult {
    let ok = parse_format_versions(r#"{"appDbFormat": 5, "snapshotFormat": 1}"#)?;
    assert_eq!((ok.app_db, ok.snapshot), (5, 1));
    for bad in [
        "not json",
        "{}",
        r#"{"appDbFormat": null, "snapshotFormat": 1}"#,
        r#"{"appDbFormat": 5, "snapshotFormat": 0}"#,
        r#"{"appDbFormat": "5", "snapshotFormat": 1}"#,
        r#"{"appDbFormat": 5.5, "snapshotFormat": 1}"#,
        r#"{"appDbFormat": 5, "snapshotFormat": 4294967296}"#,
        r#"{"appDbFormat": -1, "snapshotFormat": 1}"#,
    ] {
        let failure = parse_format_versions(bad).err().ok_or(bad)?;
        assert_eq!(failure.reason_key, FORMAT_VERSION_MISSING, "{bad}");
    }
    Ok(())
}

#[test]
fn merge_moves_staged_paths_under_the_part_name() -> TestResult {
    let left = manifest(vec![row(
        "a.exe",
        OsFamily::Windows,
        Cpu::X64,
        PackageFormat::PortableExe,
        SignatureMethod::Unsigned,
    )?]);
    let right = manifest(vec![row(
        "b.deb",
        OsFamily::Ubuntu,
        Cpu::Arm64,
        PackageFormat::Deb,
        SignatureMethod::Unsigned,
    )?]);
    let merged = merge_manifests(vec![
        ("win-x64".into(), left),
        ("linux-arm64".into(), right),
    ])?;
    assert_eq!(merged.rows.len(), 2);
    let paths: Vec<_> = merged
        .staged_files
        .iter()
        .map(|file| file.relative_path.clone())
        .collect();
    assert_eq!(
        paths,
        vec![
            PathBuf::from("win-x64").join("a.exe"),
            PathBuf::from("linux-arm64").join("b.deb")
        ]
    );
    Ok(())
}

#[test]
fn merge_rejects_mismatched_duplicate_and_unsafe_parts() -> TestResult {
    let make = |name: &str| {
        row(
            name,
            OsFamily::Ubuntu,
            Cpu::X64,
            PackageFormat::Deb,
            SignatureMethod::Unsigned,
        )
    };
    let mut other_version = manifest(vec![make("b.deb")?]);
    other_version.version = "0.2.0".into();
    let mut other_product = manifest(vec![make("c.deb")?]);
    other_product.product_name = "Other".into();
    let cases = [
        vec![],
        vec![
            ("a".to_owned(), manifest(vec![make("a.deb")?])),
            ("b".into(), other_version),
        ],
        vec![
            ("a".to_owned(), manifest(vec![make("a.deb")?])),
            ("b".into(), other_product),
        ],
        vec![
            ("a".to_owned(), manifest(vec![make("same.deb")?])),
            ("b".to_owned(), manifest(vec![make("SAME.deb")?])),
        ],
        vec![
            ("a".to_owned(), manifest(vec![make("a.deb")?])),
            ("a".into(), manifest(vec![make("b.deb")?])),
        ],
        vec![("../x".to_owned(), manifest(vec![make("a.deb")?]))],
        vec![("a/b".to_owned(), manifest(vec![make("a.deb")?]))],
        vec![(String::new(), manifest(vec![make("a.deb")?]))],
    ];
    for case in cases {
        let failure = merge_failure(case)?;
        assert_eq!(failure.reason_key, MANIFEST_INCONSISTENT);
    }
    Ok(())
}

fn merge_failure(parts: Vec<(String, ReleaseManifest)>) -> Result<GateFailure, &'static str> {
    merge_manifests(parts).err().ok_or("merge accepted")
}

#[test]
fn complete_set_passes_without_warnings() -> TestResult {
    let warnings = check_required_artifacts(&full_set(SignatureMethod::Unsigned)?)?;
    assert!(warnings.is_empty());
    Ok(())
}

#[test]
fn a_missing_msi_stops_the_release() -> TestResult {
    let mut set = full_set(SignatureMethod::Unsigned)?;
    set.rows.retain(|row| row.format != PackageFormat::Msi);
    let failure = check_required_artifacts(&set).err().ok_or("accepted")?;
    assert_eq!(failure.reason_key, ARTIFACT_MISSING);
    assert!(failure.detail.contains("Msi"));
    Ok(())
}

#[test]
fn each_missing_required_package_stops_the_release() -> TestResult {
    for index in 0..REQUIRED.len() {
        let mut set = full_set(SignatureMethod::Unsigned)?;
        set.rows.remove(index);
        let failure = check_required_artifacts(&set).err().ok_or("accepted")?;
        assert_eq!(failure.reason_key, ARTIFACT_MISSING);
    }
    Ok(())
}

#[test]
fn a_cpu_is_not_interchangeable_with_another_cpu() -> TestResult {
    let mut set = full_set(SignatureMethod::Unsigned)?;
    for row in &mut set.rows {
        if row.cpu == Cpu::Arm64 {
            row.cpu = Cpu::X64;
        }
    }
    let failure = check_required_artifacts(&set).err().ok_or("accepted")?;
    assert!(failure.detail.contains("Arm64"));
    Ok(())
}

#[test]
fn unsigned_run_rejects_signature_claims() -> TestResult {
    for claim in [SignatureMethod::SignPath, SignatureMethod::Store] {
        let set = full_set(claim)?;
        let failure = check_signature_claims(&set, false)
            .err()
            .ok_or("accepted")?;
        assert_eq!(failure.reason_key, SIGNATURE_UNVERIFIED);
        check_signature_claims(&set, true)?;
    }
    check_signature_claims(&full_set(SignatureMethod::Unsigned)?, false)?;
    Ok(())
}

#[test]
fn ad_hoc_signature_is_accepted_only_on_macos_rows() -> TestResult {
    let mut set = full_set(SignatureMethod::Unsigned)?;
    for row in &mut set.rows {
        if row.os.family == OsFamily::Macos {
            row.signature = SignatureMethod::HardenedRuntimeAdHoc;
        }
    }
    check_signature_claims(&set, false)?;
    for row in &mut set.rows {
        if row.os.family == OsFamily::Windows {
            row.signature = SignatureMethod::HardenedRuntimeAdHoc;
        }
    }
    let failure = check_signature_claims(&set, false)
        .err()
        .ok_or("accepted")?;
    assert_eq!(failure.reason_key, SIGNATURE_UNVERIFIED);
    Ok(())
}

const RECORDED_FORMATS: &str = include_str!("../../../release/format-versions.json");

#[test]
fn self_signed_signature_is_accepted_only_on_macos_rows() -> TestResult {
    let mut set = full_set(SignatureMethod::Unsigned)?;
    for row in &mut set.rows {
        if row.os.family == OsFamily::Macos {
            row.signature = SignatureMethod::HardenedRuntimeSelfSigned;
        }
    }
    check_signature_claims(&set, false)?;
    for row in &mut set.rows {
        if row.os.family == OsFamily::Windows {
            row.signature = SignatureMethod::HardenedRuntimeSelfSigned;
        }
    }
    let failure = check_signature_claims(&set, false)
        .err()
        .ok_or("accepted")?;
    assert_eq!(failure.reason_key, SIGNATURE_UNVERIFIED);
    Ok(())
}

#[test]
fn recorded_format_versions_equal_the_versions_defined_in_code() -> TestResult {
    let recorded = check_format_versions(RECORDED_FORMATS)?;
    assert_eq!(recorded, FormatVersions::from_code());
    assert_eq!(recorded.app_db, du_app_db::HISTORY_SCHEMA_VERSION);
    assert_eq!(
        recorded.snapshot,
        u32::from(du_snapshot_store::envelope::DUSNAP_FORMAT_VERSION)
    );
    Ok(())
}

#[test]
fn a_recorded_version_that_differs_from_code_is_rejected() -> TestResult {
    let defined = FormatVersions::from_code();
    let stale = format!(
        r#"{{"appDbFormat": {}, "snapshotFormat": {}}}"#,
        defined.app_db + 1,
        defined.snapshot
    );
    let failure = check_format_versions(&stale).err().ok_or("accepted")?;
    assert_eq!(failure.reason_key, FORMAT_VERSION_MISMATCH);
    let unset = r#"{"appDbFormat": null, "snapshotFormat": null}"#;
    let failure = check_format_versions(unset).err().ok_or("accepted")?;
    assert_eq!(failure.reason_key, FORMAT_VERSION_MISSING);
    Ok(())
}

#[test]
fn scoped_check_ignores_platforms_outside_the_scope() -> TestResult {
    let mut set = full_set(SignatureMethod::Unsigned)?;
    set.rows
        .retain(|row| row.os.family == OsFamily::Windows && row.cpu == Cpu::X64);
    let scope = [(OsFamily::Windows, Cpu::X64)];
    check_required_artifacts_scoped(&set, Some(&scope))?;
    assert!(check_required_artifacts(&set).is_err());
    set.rows.retain(|row| row.format != PackageFormat::Msix);
    let failure = check_required_artifacts_scoped(&set, Some(&scope))
        .err()
        .ok_or("accepted")?;
    assert_eq!(failure.reason_key, ARTIFACT_MISSING);
    assert!(failure.detail.contains("Msix") && !failure.detail.contains("Deb"));
    Ok(())
}
