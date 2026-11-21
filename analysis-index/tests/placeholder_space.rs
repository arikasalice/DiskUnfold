//! F03 FR6.HD1 (#211): a placeholder whose local occupancy the OS does not report has an
//! unknown allocation, is missing on the allocated basis and names that cause. Its
//! logical size still counts.
use du_analysis_index::{
    BuildError, BuildHandle, FileIdentity, IngestSink, Observation, SharingEvidence,
    SpaceObservation,
};
use du_core_contracts::result::{
    ResultCompleteness, ResultNativePath, ResultScopeRoot, ResultVersionRead, SpaceAggregate,
};
use du_core_contracts::scan_observation::PlaceholderObservation;
use du_core_contracts::{
    ByteSize, Id, ItemRef, PathBytes, PathEncoding, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
};
use std::num::NonZeroU64;

type TestResult = Result<(), Box<dyn std::error::Error>>;
const KEY: &str = "space.placeholder_local_unknown";

fn id(n: u8) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}
fn error(e: BuildError) -> String {
    format!("{e:?}")
}
fn size(basis: SizeBasis, evidence: SizeEvidence, n: u64) -> SizeValue {
    SizeValue {
        basis,
        evidence,
        amount: SizeAmount::Exact {
            bytes: ByteSize::from(n),
        },
    }
}

/// Adds a file of `logical` bytes under `folder`; `allocated` is the OS report, if any.
fn file(
    build: &mut BuildHandle,
    name: &str,
    inode: u8,
    logical: u64,
    allocated: Option<u64>,
) -> Result<Vec<Vec<u8>>, BuildError> {
    let path = vec![b"folder".to_vec(), name.as_bytes().to_vec()];
    build.add_with_space(
        Observation {
            path: path.clone(),
            size: Some(size(SizeBasis::Logical, SizeEvidence::Measured, logical)),
        },
        SpaceObservation {
            allocated: allocated.map(|n| size(SizeBasis::Allocated, SizeEvidence::OsReported, n)),
            identity: Some(FileIdentity {
                volume: vec![1],
                file: vec![inode],
            }),
            link_count: Some(1),
            sharing: SharingEvidence::None,
        },
    )?;
    let inode = NonZeroU64::new(u64::from(inode)).ok_or(BuildError::InvalidTree)?;
    build.retain_enumerated_inode(0, &path, inode)?;
    Ok(path)
}

/// A folder with a local file and a file without an allocation report. When
/// `placeholder` is set, the second file is a dataless placeholder.
///
/// The scope root carries a volume identity, as a real scan's does: a placeholder
/// observation is kept only for a file whose identity is on the root's volume.
fn sealed(placeholder: bool) -> Result<std::sync::Arc<du_analysis_index::ResultVersion>, String> {
    let sink = IngestSink::default();
    let root = ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(b"/scope"),
        },
        volume_id: vec![1],
        file_id: vec![1],
    };
    let mut build = sink.open_observed(id(1)?, root).map_err(error)?;
    build.add_structure(vec![]).map_err(error)?;
    build
        .add_structure(vec![b"folder".to_vec()])
        .map_err(error)?;
    file(&mut build, "local", 7, 10, Some(4096)).map_err(error)?;
    let cloud = file(&mut build, "cloud", 8, 5_000_000, None).map_err(error)?;
    if placeholder {
        build
            .retain_placeholder_observation(0, &cloud, PlaceholderObservation::MacDataless)
            .map_err(error)?;
    }
    let version = build
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(error)?;
    let cloud = version
        .items()
        .iter()
        .find(|item| item.name == b"cloud")
        .ok_or("cloud")?;
    let kept = version
        .item_placeholder_observation(&ItemRef {
            result_version: version.version_id().clone(),
            item_no: cloud.item_no,
        })
        .map_err(|e| format!("{e:?}"))?;
    // The fixture must really hold the placeholder it claims to.
    assert_eq!(
        kept,
        placeholder.then_some(PlaceholderObservation::MacDataless)
    );
    Ok(version)
}

fn names_cause(aggregate: &SpaceAggregate) -> bool {
    aggregate.limitations.contains(&KEY)
}

#[test]
fn a_placeholder_without_local_occupancy_is_missing_and_named() -> TestResult {
    let version = sealed(true)?;
    let folder = version
        .items()
        .iter()
        .find(|item| item.name == b"folder")
        .ok_or("folder")?;
    for aggregate in [
        version.space(folder.item_no).ok_or("folder total")?,
        version.scope_total().clone(),
    ] {
        // The logical size counts the placeholder in full.
        assert_eq!(
            aggregate.logical.amount,
            SizeAmount::Exact {
                bytes: ByteSize::from(5_000_010)
            }
        );
        // The allocated size is never completed with zero for the placeholder.
        assert_eq!(
            aggregate.allocated.amount,
            SizeAmount::LowerBound {
                bytes: ByteSize::from(4096)
            }
        );
        assert_eq!(
            aggregate.allocated_completeness,
            ResultCompleteness::Missing
        );
        assert_eq!(aggregate.unique_physical.amount, SizeAmount::Unknown);
        assert!(names_cause(aggregate), "{:?}", aggregate.limitations);
    }
    Ok(())
}

#[test]
fn a_missing_allocation_that_is_not_a_placeholder_names_no_placeholder() -> TestResult {
    let version = sealed(false)?;
    let total = version.scope_total();
    assert!(matches!(
        total.allocated.amount,
        SizeAmount::LowerBound { .. }
    ));
    assert!(!names_cause(total), "{:?}", total.limitations);
    Ok(())
}

#[test]
fn a_selection_with_a_placeholder_names_the_cause() -> TestResult {
    let version = sealed(true)?;
    let refs: Vec<ItemRef> = version
        .items()
        .iter()
        .filter(|item| item.name == b"cloud" || item.name == b"local")
        .map(|item| ItemRef {
            result_version: version.version_id().clone(),
            item_no: item.item_no,
        })
        .collect();
    assert_eq!(refs.len(), 2);
    let total = version
        .selection_total(&refs, true, || false)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(total.allocated.amount, SizeAmount::Unknown);
    assert!(names_cause(&total), "{:?}", total.limitations);
    Ok(())
}
