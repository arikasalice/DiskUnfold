//! An untraversed link counts its own observed size; its target is only counted.
use du_analysis_index::{
    BuildHandle, FileIdentity, IngestSink, Observation, ResultVersion, SharingEvidence,
    SpaceObservation,
};
use du_core_contracts::result::{
    ResultCompleteness, ResultNativePath, ResultScopeRoot, ResultVersionRead, SpaceAggregate,
};
use du_core_contracts::{
    ByteSize, Id, PathBytes, PathEncoding, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
};

type Test = Result<(), String>;

fn id(n: u8) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}
fn e<T: std::fmt::Debug>(error: T) -> String {
    format!("{error:?}")
}
fn path(parts: &[&str]) -> Vec<Vec<u8>> {
    parts.iter().map(|part| part.as_bytes().to_vec()).collect()
}
fn size(basis: SizeBasis, bytes: u64) -> SizeValue {
    SizeValue {
        basis,
        evidence: SizeEvidence::OsReported,
        amount: SizeAmount::Exact {
            bytes: ByteSize::from(bytes),
        },
    }
}
fn exact(bytes: u64) -> SizeAmount {
    SizeAmount::Exact {
        bytes: ByteSize::from(bytes),
    }
}
fn space(bytes: u64, file: u8) -> SpaceObservation {
    SpaceObservation {
        allocated: Some(size(SizeBasis::Allocated, bytes)),
        identity: Some(FileIdentity {
            volume: vec![1],
            file: vec![file],
        }),
        link_count: Some(1),
        sharing: SharingEvidence::None,
    }
}
fn folder_with_file() -> Result<BuildHandle, String> {
    let root = ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(b"/r"),
        },
        volume_id: vec![1],
        file_id: vec![1],
    };
    let mut build = IngestSink::default()
        .open_forest(id(1)?, vec![root])
        .map_err(e)?;
    build.add_directory_at(0, vec![]).map_err(e)?;
    build.add_directory_at(0, path(&["folder"])).map_err(e)?;
    build
        .add_with_space_at(
            0,
            Observation {
                path: path(&["folder", "file"]),
                size: Some(size(SizeBasis::Logical, 10)),
            },
            space(10, 2),
        )
        .map_err(e)?;
    Ok(build)
}
fn folder(result: &ResultVersion) -> Result<SpaceAggregate, String> {
    let item = result
        .items()
        .iter()
        .find(|item| item.name == b"folder")
        .ok_or("no folder")?;
    result.space(item.item_no).ok_or("no aggregate".into())
}

#[test]
fn an_observed_link_adds_its_own_size_and_its_target_keeps_the_folder_exact() -> Test {
    let mut build = folder_with_file()?;
    build
        .add_observed_untraversed_link_at(
            0,
            Observation {
                path: path(&["folder", "link"]),
                size: Some(size(SizeBasis::Logical, 5)),
            },
            Some(space(5, 3)),
        )
        .map_err(e)?;
    let result = build
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(e)?;

    for aggregate in [folder(&result)?, result.scope_total().clone()] {
        assert_eq!(aggregate.logical.amount, exact(15));
        assert_eq!(aggregate.completeness, ResultCompleteness::Complete);
        assert_eq!(aggregate.allocated.amount, exact(15));
        assert_eq!(aggregate.untraversed_links, 1);
        assert_eq!(aggregate.missing_items, 0);
    }
    Ok(())
}

#[test]
fn a_link_whose_own_size_is_unknown_is_not_filled_with_zero() -> Test {
    let mut build = folder_with_file()?;
    build
        .add_observed_untraversed_link_at(
            0,
            Observation {
                path: path(&["folder", "link"]),
                size: None,
            },
            None,
        )
        .map_err(e)?;
    let result = build
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(e)?;

    for aggregate in [folder(&result)?, result.scope_total().clone()] {
        assert_eq!(
            aggregate.logical.amount,
            SizeAmount::LowerBound {
                bytes: ByteSize::from(10)
            }
        );
        assert_eq!(aggregate.completeness, ResultCompleteness::Missing);
        assert_eq!(aggregate.untraversed_links, 1);
    }
    Ok(())
}
