//! #341: the allocation of an unread folder follows whether the scan collected it.
use du_analysis_index::{
    BuildHandle, FileIdentity, IngestSink, Observation, ResultVersion, SharingEvidence,
    SpaceObservation,
};
use du_core_contracts::{
    ByteSize, Id, PathBytes, PathEncoding, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    result::{
        ResultCompleteness, ResultNativePath, ResultScopeRoot, ResultVersionRead, SpaceAggregate,
    },
};
use std::sync::Arc;

type Test = Result<(), String>;
const NOT_COLLECTED: &str = "space.allocated_not_collected";

fn e(error: impl std::fmt::Debug) -> String {
    format!("{error:?}")
}

fn id(n: u8) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}")).map_err(e)
}

fn size(basis: SizeBasis, bytes: u64) -> SizeValue {
    SizeValue {
        basis,
        evidence: SizeEvidence::Measured,
        amount: SizeAmount::Exact {
            bytes: ByteSize::from(bytes),
        },
    }
}

/// A scan with a readable file `f` and an unreadable folder `bad`; `collected` says
/// whether the file carries allocation evidence.
fn scan(collected: bool) -> Result<Arc<ResultVersion>, String> {
    let root = ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(b"/r"),
        },
        volume_id: vec![1],
        file_id: vec![2],
    };
    let mut build: BuildHandle = IngestSink::default()
        .open_forest(id(1)?, vec![root])
        .map_err(e)?;
    build.add_directory_at(0, vec![]).map_err(e)?;
    build
        .add_directory_at(0, vec![b"bad".to_vec()])
        .map_err(e)?;
    let observation = Observation {
        path: vec![b"f".to_vec()],
        size: Some(size(SizeBasis::Logical, 5)),
    };
    if collected {
        let space = SpaceObservation {
            allocated: Some(size(SizeBasis::Allocated, 4096)),
            identity: Some(FileIdentity {
                volume: vec![1],
                file: vec![1],
            }),
            link_count: Some(1),
            sharing: SharingEvidence::None,
        };
        build.add_with_space_at(0, observation, space).map_err(e)?;
    } else {
        build.add_at(0, observation).map_err(e)?;
    }
    build
        .record_gap_at(0, vec![b"bad".to_vec()], "scan.path_denied".into(), None)
        .map_err(e)?;
    build.seal(id(2)?, ResultCompleteness::Missing).map_err(e)
}

fn bad(version: &ResultVersion) -> Result<SpaceAggregate, String> {
    let item = version
        .items()
        .iter()
        .find(|item| item.name == b"bad")
        .ok_or("no bad folder")?;
    version
        .space(item.item_no)
        .ok_or_else(|| "no aggregate".into())
}

/// The decision is taken once for the version: reading the same scan twice gives the
/// same aggregate, whether allocation was collected or not.
#[test]
fn the_same_scan_gives_the_same_unread_aggregate_every_time() -> Test {
    for collected in [true, false] {
        let (first, second) = (scan(collected)?, scan(collected)?);
        assert_eq!(bad(&first)?, bad(&second)?, "collected={collected}");
        assert_eq!(bad(&first)?, bad(&first)?);
    }
    Ok(())
}

/// A lower bound of the allocation never comes with "not collected", and an
/// allocation that was not collected stays unknown with that limit.
#[test]
fn a_bounded_allocation_is_never_marked_not_collected() -> Test {
    let zero = SizeAmount::LowerBound {
        bytes: ByteSize::from(0),
    };
    let collected = scan(true)?;
    let aggregate = bad(&collected)?;
    assert_eq!(aggregate.allocated.amount, zero);
    assert!(!aggregate.limitations.contains(&NOT_COLLECTED));
    let uncollected = scan(false)?;
    let aggregate = bad(&uncollected)?;
    assert_eq!(aggregate.allocated.amount, SizeAmount::Unknown);
    assert!(aggregate.limitations.contains(&NOT_COLLECTED));
    // The logical size is a bound in both scans.
    assert_eq!(aggregate.logical.amount, zero);
    Ok(())
}
