//! Embedded sample bundle, corrupt variants and the sample source, through the public API.
use std::sync::Arc;

use du_core_contracts::{
    ports::{ContentReadPort, PortErrorKind},
    sample_ipc::{CAPABILITY_SAMPLE_EXAMPLE, CAPABILITY_SAMPLE_SOURCE},
    scan_observation::{AnchoredDirectory, ObservedEntry, ObservedKind, ScopeRootPort},
    volume::{Capability, CapabilityState},
};
use du_scan_engine::sample_bundle::{
    BundleFailure, BundleSourcePort, MAX_BUNDLE_SIZE_BYTES, ValidatedBundle,
    embedded::EmbeddedSampleBundle,
    format_v1::{SampleFormatRegistry, build_bundle},
    validate_bundle,
};
use du_scan_engine::sample_source::{
    SAMPLE_ROOT_PATH, SAMPLE_VOLUME_PREFIX, SampleCapability, SampleScope, sample_capability,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

struct Bytes(Option<Vec<u8>>);

impl BundleSourcePort for Bytes {
    fn read_bundle(&self) -> Option<Arc<[u8]>> {
        self.0.as_deref().map(Arc::from)
    }
}

fn check(bytes: Option<Vec<u8>>) -> Result<ValidatedBundle, BundleFailure> {
    validate_bundle(&Bytes(bytes), &SampleFormatRegistry::standard())
}

fn embedded() -> Result<ValidatedBundle, String> {
    EmbeddedSampleBundle::validate().map_err(|failure| format!("{failure:?}"))
}

fn payload_of(bundle: &[u8]) -> Result<Vec<u8>, &'static str> {
    let header_len = u32::from_le_bytes(bundle[10..14].try_into().map_err(|_| "short")?) as usize;
    Ok(bundle[14 + header_len..].to_vec())
}

/// Assembles a body with an arbitrary header JSON around a payload.
fn assemble(header_json: &str, payload: &[u8]) -> Vec<u8> {
    let mut out = b"DUSAMPLE".to_vec();
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&(header_json.len() as u32).to_le_bytes());
    out.extend_from_slice(header_json.as_bytes());
    out.extend_from_slice(payload);
    out
}

fn header_json(payload: &[u8], items: u64, scenarios: &str) -> String {
    format!(
        r#"{{"bundleVersion":"t","digestAlgorithm":"blake3","digest":"{}","declaredItemCount":{items},"scenarioIds":{scenarios}}}"#,
        blake3::hash(payload).to_hex()
    )
}

#[test]
fn embedded_bundle_validates_with_six_scenarios_and_two_observations() -> TestResult {
    let bundle = embedded()?;
    let ids: Vec<&str> = bundle
        .header()
        .scenario_ids
        .iter()
        .map(String::as_str)
        .collect();
    assert_eq!(
        ids,
        [
            "hierarchy_sizes",
            "role_folders",
            "two_point_changes",
            "duplicates",
            "observation_gaps",
            "cleanup_plan_start"
        ]
    );
    assert_eq!(bundle.observation_count(), 2);
    assert_eq!(bundle.header().bundle_version, "1.0.0");
    let total: usize = bundle.observations().iter().map(|o| o.items.len()).sum();
    assert_eq!(total as u64, bundle.header().declared_item_count);
    assert!(total <= 1000);
    let records = bundle.records();
    assert!(!records.protection_rules.is_empty() && !records.cleanup_candidates.is_empty());
    for scenario in ids {
        assert!(
            records
                .scenario_references
                .iter()
                .any(|r| r.scenario_id == scenario),
            "{scenario} has no items"
        );
    }
    Ok(())
}

#[test]
fn committed_binary_matches_the_authored_source() -> TestResult {
    let source = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/assets/sample/bundle.v1.json"
    ))?;
    let built = build_bundle("1.0.0", &source).map_err(|_| "source does not build")?;
    assert!(
        built == EmbeddedSampleBundle::bytes(),
        "rerun the build_sample_bundle example"
    );
    Ok(())
}

#[test]
fn corrupt_bundles_fail_with_their_classification() -> TestResult {
    let good = EmbeddedSampleBundle::bytes().to_vec();
    let payload = payload_of(&good)?;

    assert_eq!(check(None).err(), Some(BundleFailure::Missing));

    let mut unknown_version = good.clone();
    unknown_version[8..10].copy_from_slice(&2u16.to_le_bytes());
    assert_eq!(
        check(Some(unknown_version)).err(),
        Some(BundleFailure::UnsupportedFormat)
    );
    let mut bad_magic = good.clone();
    bad_magic[0] = b'X';
    assert_eq!(
        check(Some(bad_magic)).err(),
        Some(BundleFailure::UnsupportedFormat)
    );

    let mut tampered = good.clone();
    let last = tampered.len() - 2;
    tampered[last] ^= 0x01;
    assert_eq!(check(Some(tampered)).err(), Some(BundleFailure::Corrupt));

    // Digest is valid, but the body is larger than the 8 MiB cap.
    let mut padded = payload.clone();
    padded.resize(MAX_BUNDLE_SIZE_BYTES + 1, b' ');
    let oversize = build_bundle("t", &padded).map_err(|_| "padded payload must build")?;
    assert_eq!(check(Some(oversize)).err(), Some(BundleFailure::Corrupt));

    // Digest is valid, but the header declares more items than the cap.
    let over_items = assemble(
        &header_json(&payload, 20_001, r#"["hierarchy_sizes"]"#),
        &payload,
    );
    assert_eq!(check(Some(over_items)).err(), Some(BundleFailure::Corrupt));

    // Digest is valid, but the records do not decode.
    let broken = b"{\"scenarios\":[]}".to_vec();
    let bad_records = assemble(&header_json(&broken, 0, "[]"), &broken);
    assert_eq!(check(Some(bad_records)).err(), Some(BundleFailure::Corrupt));

    // A record that names a scenario the header does not list.
    let wrong_scenarios = assemble(&header_json(&payload, 345, r#"["other"]"#), &payload);
    assert_eq!(
        check(Some(wrong_scenarios)).err(),
        Some(BundleFailure::Corrupt)
    );
    Ok(())
}

type Listing = Vec<(Vec<String>, ObservedEntry)>;

fn collect(scope: &SampleScope) -> Result<Listing, Box<dyn std::error::Error>> {
    let root = scope
        .open_root(SAMPLE_ROOT_PATH)
        .map_err(|e| format!("{e:?}"))?;
    let mut out = Vec::new();
    let mut stack: Vec<(Vec<String>, Box<dyn AnchoredDirectory>)> =
        vec![(Vec::new(), root.directory)];
    while let Some((path, mut dir)) = stack.pop() {
        loop {
            let batch = dir.next_batch().map_err(|e| format!("{e:?}"))?;
            if batch.is_empty() {
                break;
            }
            for entry in batch {
                let mut child_path = path.clone();
                child_path.push(String::from_utf8(entry.name.clone())?);
                if entry.enumerated_kind == ObservedKind::Directory
                    && let Ok(child) = dir.open_child(&entry)
                {
                    stack.push((child_path.clone(), child));
                }
                out.push((child_path, entry));
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

#[test]
fn enumerating_every_observation_matches_the_declared_item_count() -> TestResult {
    let bundle = embedded()?;
    let mut total = 0;
    for (index, observation) in bundle.observations().iter().enumerate() {
        let scope = SampleScope::new(&bundle, index).map_err(|e| format!("{e:?}"))?;
        let entries = collect(&scope)?;
        assert_eq!(entries.len(), observation.items.len());
        assert_eq!(scope.item_count(), observation.items.len());
        total += entries.len();
    }
    assert_eq!(total as u64, bundle.header().declared_item_count);
    Ok(())
}

#[test]
fn source_reports_state_examples_in_the_observation_fields() -> TestResult {
    let bundle = embedded()?;
    let scope = SampleScope::new(&bundle, 0).map_err(|e| format!("{e:?}"))?;
    assert!(scope.volume_id().starts_with(SAMPLE_VOLUME_PREFIX));
    assert!(scope.open_root(b"/somewhere/else").is_err());
    let entries = collect(&scope)?;
    let find = |name: &str| {
        entries
            .iter()
            .find(|(path, _)| path.join("/") == name)
            .map(|(_, entry)| entry.clone())
            .ok_or("entry missing")
    };
    let denied = find("Gaps/locked.db")?;
    assert_eq!(
        denied.metadata.as_ref().err().map(|e| e.kind.clone()),
        Some(PortErrorKind::PermissionDenied)
    );
    let vanished = find("Gaps/vanished.tmp")?;
    assert_eq!(
        vanished.metadata.as_ref().err().map(|e| e.kind.clone()),
        Some(PortErrorKind::NotFound)
    );
    let link = find("Gaps/shortcut-link")?;
    assert_eq!(link.enumerated_kind, ObservedKind::Link);
    let unknown = find("Gaps/unknown-size.dat")?;
    assert!(unknown.metadata.as_ref().is_ok_and(|m| m.size.is_err()));
    let image = find("Projects/Website/assets/images/image-01.png")?;
    let meta = image.metadata.map_err(|e| format!("{e:?}"))?;
    assert_eq!(meta.size, Ok(6 * 1048576 + 200 * 1024));
    assert!(meta.allocated.is_some() && meta.space_identity.is_some());

    // Denied directory, links and files cannot be opened as directories.
    let root = scope
        .open_root(SAMPLE_ROOT_PATH)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(root.directory.ancestor_file_ids(), Some(&[][..]));
    Ok(())
}

#[test]
fn content_reads_distinguish_duplicates_from_same_size_files() -> TestResult {
    let bundle = embedded()?;
    let scope = SampleScope::new(&bundle, 0).map_err(|e| format!("{e:?}"))?;
    let read = |path: &str| scope.read(format!("/sample/{path}").as_bytes(), 4096);
    let a = read("Duplicates/Photos/holiday.jpg").map_err(|e| format!("{e:?}"))?;
    let b = read("Duplicates/Share/holiday (1).jpg").map_err(|e| format!("{e:?}"))?;
    assert_eq!(a, b);
    assert_eq!(a.len(), 4096);
    let same_a = read("Duplicates/SameSize/block-a.bin").map_err(|e| format!("{e:?}"))?;
    let same_b = read("Duplicates/SameSize/block-b.bin").map_err(|e| format!("{e:?}"))?;
    assert_ne!(same_a, same_b);
    assert!(read("Gaps/vanished.tmp").is_err());
    assert!(read("Nowhere/file").is_err());
    Ok(())
}

#[test]
fn capability_table_rows_have_their_expected_values() -> TestResult {
    let bundle = embedded()?;
    let examples = bundle.records().example_capabilities;
    let supported = Capability {
        state: CapabilityState::Supported,
        reason_key: None,
    };
    let example = |state| Capability {
        state,
        reason_key: Some(CAPABILITY_SAMPLE_EXAMPLE),
    };
    let source = Capability {
        state: CapabilityState::Unsupported,
        reason_key: Some(CAPABILITY_SAMPLE_SOURCE),
    };
    let expected = [
        (SampleCapability::Enumerate, supported.clone()),
        (SampleCapability::MetadataLookup, supported.clone()),
        (SampleCapability::VolumeIdentity, supported.clone()),
        (SampleCapability::FileIdentity, supported.clone()),
        (SampleCapability::ContentRead, supported),
        (
            SampleCapability::AllocatedSize,
            example(CapabilityState::Supported),
        ),
        (
            SampleCapability::LinkDetection,
            example(CapabilityState::Supported),
        ),
        (
            SampleCapability::PlaceholderDetection,
            example(CapabilityState::Partial),
        ),
        (
            SampleCapability::ExtraAttributes,
            example(CapabilityState::Unsupported),
        ),
        (SampleCapability::Trash, source.clone()),
        (SampleCapability::SameVolumeMove, source.clone()),
        (SampleCapability::Delete, source.clone()),
        (SampleCapability::Restore, source.clone()),
        (SampleCapability::OpenInFileManager, source.clone()),
        (SampleCapability::ContentPreview, source.clone()),
        (SampleCapability::ChangeNotification, source),
    ];
    assert_eq!(expected.len(), SampleCapability::ALL.len());
    for (row, value) in expected {
        assert_eq!(sample_capability(row, &examples), value, "{row:?}");
    }
    Ok(())
}

#[test]
fn reading_the_same_bundle_twice_gives_the_same_result() -> TestResult {
    let first = embedded()?;
    let second = embedded()?;
    assert_eq!(first, second);
    for index in 0..2 {
        let a = collect(&SampleScope::new(&first, index).map_err(|e| format!("{e:?}"))?)?;
        let b = collect(&SampleScope::new(&second, index).map_err(|e| format!("{e:?}"))?)?;
        assert_eq!(a, b);
    }
    Ok(())
}
