//! Files outside the scan boundary are unread ranges (F03 FR5.HD1, DEC-006).
use std::sync::Arc;

use du_analysis_index::{
    FileIdentity, IngestSink, Observation, ResultVersion, SharingEvidence, SpaceObservation,
    visualization::{
        geometry::{Rect, Scale, Weight},
        histogram::{Basis, Bin},
        histogram_projection::{self, Request},
    },
};
use du_core_contracts::{
    Id, ItemRef, PathBytes, PathEncoding, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot, ResultVersionRead},
};

fn e<T: std::fmt::Debug>(error: T) -> String {
    format!("{error:?}")
}
fn sized(basis: SizeBasis, bytes: u64) -> SizeValue {
    SizeValue {
        basis,
        evidence: SizeEvidence::OsReported,
        amount: SizeAmount::Exact {
            bytes: bytes.into(),
        },
    }
}
fn exact(bytes: u64) -> SizeAmount {
    SizeAmount::Exact {
        bytes: bytes.into(),
    }
}
fn path(parts: &[&str]) -> Vec<Vec<u8>> {
    parts.iter().map(|part| part.as_bytes().to_vec()).collect()
}

/// Builds `/` with `a` (10 bytes) and `etc/` holding `b` (5 bytes) and the
/// bind-mounted boundary file `etc/hosts`.
fn scope() -> Result<Arc<ResultVersion>, String> {
    let id = |n| Id::from_v7_bytes([n; 16]);
    let root = ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(b"/r"),
        },
        volume_id: vec![1],
        file_id: vec![1],
    };
    let mut build = IngestSink::default()
        .open_forest(id(40), vec![root])
        .map_err(e)?;
    build.add_directory_at(0, vec![]).map_err(e)?;
    build.add_directory_at(0, path(&["etc"])).map_err(e)?;
    for (parts, bytes, file) in [(&["a"][..], 10, 2u8), (&["etc", "b"][..], 5, 3)] {
        build
            .add_with_space_at(
                0,
                Observation {
                    path: path(parts),
                    size: Some(sized(SizeBasis::Logical, bytes)),
                },
                SpaceObservation {
                    allocated: Some(sized(SizeBasis::Allocated, bytes)),
                    identity: Some(FileIdentity {
                        volume: vec![1],
                        file: vec![file],
                    }),
                    link_count: Some(1),
                    sharing: SharingEvidence::None,
                },
            )
            .map_err(e)?;
    }
    build
        .add_boundary_file_at(0, path(&["etc", "hosts"]))
        .map_err(e)?;
    build.seal(id(41), ResultCompleteness::Complete).map_err(e)
}

fn item_no(version: &ResultVersion, name: &[u8]) -> Result<u32, String> {
    version
        .items()
        .iter()
        .find(|item| item.name == name)
        .map(|item| item.item_no)
        .ok_or_else(|| "missing item".into())
}

#[test]
fn a_boundary_file_keeps_its_folder_and_ancestors_exact_with_no_missing_item() -> Result<(), String>
{
    let version = scope()?;
    let etc = item_no(&version, b"etc")?;
    let hosts = item_no(&version, b"hosts")?;
    assert!(version.is_boundary_item(hosts));
    assert!(!version.is_boundary_item(item_no(&version, b"b")?));
    for (item, bytes) in [(etc, 5), (0, 15)] {
        let aggregate = version.space(item).ok_or("missing aggregate")?;
        assert_eq!(aggregate.completeness, ResultCompleteness::Complete);
        assert_eq!(aggregate.logical.amount, exact(bytes));
        assert_eq!(aggregate.allocated.amount, exact(bytes));
        assert_eq!(aggregate.missing_items, 0);
    }
    // The boundary file's own size stays unknown and is never zero.
    let own = version.space(hosts).ok_or("missing own aggregate")?;
    assert_eq!(own.logical.amount, SizeAmount::Unknown);
    Ok(())
}

#[test]
fn the_histogram_leaves_out_a_boundary_file() -> Result<(), String> {
    let version = scope()?;
    let scope_ref = ItemRef {
        result_version: version.version_id().clone(),
        item_no: item_no(&version, b"etc")?,
    };
    let bins = [Bin {
        lower: 0,
        upper: None,
    }];
    let model = histogram_projection::project(
        Request {
            version: &version,
            scope: &scope_ref,
            size_basis: SizeBasis::Logical,
            bins: &bins,
            bounds: Rect {
                left: 0,
                top: 0,
                right: 100,
                bottom: 100,
            },
            histogram_basis: Basis::Bytes,
            scale: Scale::Automatic,
        },
        &mut || false,
    )
    .map_err(e)?;
    assert_eq!(model.histogram.observed_items, 1);
    assert!(model.histogram.uncertain_items.is_empty());
    assert_eq!(model.histogram.bins[0].bytes, Weight::Exact(5));
    assert_eq!(
        model.histogram.source_completeness,
        ResultCompleteness::Complete
    );
    Ok(())
}
