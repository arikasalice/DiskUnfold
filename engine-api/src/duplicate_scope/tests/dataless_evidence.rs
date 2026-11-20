//! #211: a dataless placeholder observation never lets the duplicate check read content.
use super::*;
use du_core_contracts::{
    duplicate_content::PlaceholderEvidence, scan_observation::PlaceholderObservation,
};

#[test]
fn a_dataless_file_keeps_content_reads_refused() -> Result<(), String> {
    let source = IngestSink::default();
    let root = ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(b"/"),
        },
        volume_id: vec![1],
        file_id: vec![1],
    };
    let mut build = source
        .open_observed(id(1), root)
        .map_err(|e| format!("{e:?}"))?;
    build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    let path = vec![b"cloud".to_vec()];
    build
        .add_with_space(
            Observation {
                path: path.clone(),
                size: Some(SizeValue {
                    basis: SizeBasis::Logical,
                    evidence: SizeEvidence::OsReported,
                    amount: SizeAmount::Exact {
                        bytes: ByteSize::from(8),
                    },
                }),
            },
            SpaceObservation {
                allocated: None,
                identity: Some(FileIdentity {
                    volume: vec![1],
                    file: vec![2],
                }),
                link_count: Some(1),
                sharing: SharingEvidence::None,
            },
        )
        .map_err(|e| format!("{e:?}"))?;
    build
        .set_modified_at(0, &path, Timestamp::parse("2026-01-01T00:00:00.000Z")?)
        .map_err(|e| format!("{e:?}"))?;
    build
        .retain_enumerated_inode(0, &path, std::num::NonZeroU64::new(42).ok_or("zero")?)
        .map_err(|e| format!("{e:?}"))?;
    build
        .retain_placeholder_observation(0, &path, PlaceholderObservation::MacDataless)
        .map_err(|e| format!("{e:?}"))?;
    let version = build
        .seal(id(2), ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let selection = select(
        &source,
        Scope {
            version: &id(2),
            roots: &[ItemRef {
                result_version: id(2),
                item_no: 0,
            }],
            exclusions: &[],
            min_bytes: 0,
        },
    )
    .map_err(|e| format!("{e:?}"))?;
    let cloud = reference(&version, b"cloud")?;
    let evidence = selection
        .content_evidence(&cloud)
        .ok_or("missing evidence")?;
    assert_eq!(
        evidence.placeholder_observation,
        Some(PlaceholderObservation::MacDataless)
    );
    // Only a current Local check by the platform adapter allows a read, and the
    // content port refuses anything else; the scan observation never makes it Local.
    assert_eq!(evidence.placeholder, PlaceholderEvidence::Unknown);
    Ok(())
}
