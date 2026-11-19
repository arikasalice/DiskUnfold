//! Untraversed links in a classification scope: coverage and the scope total (F03 FR5.HD1).
mod group_support;

use std::sync::Arc;

use du_analysis_index::{IngestSink, Observation, ResultVersion, group::Parallelism};
use du_core_contracts::{
    PathBytes, PathEncoding, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot, ResultVersionRead},
};
use group_support::{classify, err, id, item_ref};

fn sized(bytes: u64) -> SizeValue {
    SizeValue {
        basis: SizeBasis::Logical,
        evidence: SizeEvidence::OsReported,
        amount: SizeAmount::Exact {
            bytes: bytes.into(),
        },
    }
}

/// Builds `/` with a 10-byte file and `links/` holding one link of the given own size.
fn scope_with_link(link_size: Option<u64>) -> Result<Arc<ResultVersion>, String> {
    let root = ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(b"/r"),
        },
        volume_id: vec![1],
        file_id: vec![1],
    };
    let mut build = IngestSink::default()
        .open_forest(id(30), vec![root])
        .map_err(err)?;
    build.add_directory_at(0, vec![]).map_err(err)?;
    build
        .add_directory_at(0, vec![b"links".to_vec()])
        .map_err(err)?;
    build
        .add_file_at(
            0,
            Observation {
                path: vec![b"a".to_vec()],
                size: Some(sized(10)),
            },
        )
        .map_err(err)?;
    build
        .add_observed_untraversed_link_at(
            0,
            Observation {
                path: vec![b"links".to_vec(), b"up".to_vec()],
                size: link_size.map(sized),
            },
            None,
        )
        .map_err(err)?;
    build
        .seal(id(31), ResultCompleteness::Complete)
        .map_err(err)
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
fn a_link_with_a_known_own_size_is_counted_in_the_scope_and_keeps_coverage() -> Result<(), String> {
    let version = scope_with_link(Some(3))?;
    let read = classify(&version, None, vec![], Parallelism::default())?;
    let recount = read.reconcile(|| false).map_err(err)?;
    assert!(recount.source_coverage_complete);
    assert_eq!(recount.observed_files, 1);
    let totals = read.totals(&[], || false).map_err(err)?;
    // The scope total equals the F03 range total, including the link's own 3 bytes.
    let f03 = version.space(0).ok_or("missing total")?;
    assert_eq!(f03.logical.amount, SizeAmount::Exact { bytes: 13.into() });
    assert_eq!(totals.scope.logical.amount, f03.logical.amount);
    // The link is not a group member: the unclassified files stay the file alone.
    assert_eq!(
        totals.unclassified.logical.amount,
        SizeAmount::Exact { bytes: 10.into() }
    );

    // A scope that holds only the link.
    let links = item_ref(&version, item_no(&version, b"links")?);
    let read = classify(&version, Some(&links), vec![], Parallelism::default())?;
    assert!(
        read.reconcile(|| false)
            .map_err(err)?
            .source_coverage_complete
    );
    let totals = read.totals(&[], || false).map_err(err)?;
    let f03 = version.space(links.item_no).ok_or("missing total")?;
    assert_eq!(f03.logical.amount, SizeAmount::Exact { bytes: 3.into() });
    assert_eq!(totals.scope.logical.amount, f03.logical.amount);
    Ok(())
}

#[test]
fn a_link_whose_own_size_is_unknown_is_never_counted_as_zero() -> Result<(), String> {
    let version = scope_with_link(None)?;
    let read = classify(&version, None, vec![], Parallelism::default())?;
    // The untraversed target does not lower coverage, so the member totals stay exact.
    assert!(
        read.reconcile(|| false)
            .map_err(err)?
            .source_coverage_complete
    );
    let totals = read.totals(&[], || false).map_err(err)?;
    assert_eq!(
        totals.unclassified.logical.amount,
        SizeAmount::Exact { bytes: 10.into() }
    );
    // F03 keeps the range a lower bound; the scope set total is not an exact 10.
    let f03 = version.space(0).ok_or("missing total")?;
    assert_eq!(
        f03.logical.amount,
        SizeAmount::LowerBound { bytes: 10.into() }
    );
    assert!(!matches!(
        totals.scope.logical.amount,
        SizeAmount::Exact { .. }
    ));
    Ok(())
}

#[test]
fn a_boundary_file_is_neither_a_member_nor_in_the_scope_total() -> Result<(), String> {
    let root = ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(b"/r"),
        },
        volume_id: vec![1],
        file_id: vec![1],
    };
    let mut build = IngestSink::default()
        .open_forest(id(32), vec![root])
        .map_err(err)?;
    build.add_directory_at(0, vec![]).map_err(err)?;
    build
        .add_file_at(
            0,
            Observation {
                path: vec![b"a".to_vec()],
                size: Some(sized(10)),
            },
        )
        .map_err(err)?;
    build
        .add_boundary_file_at(0, vec![b"hosts".to_vec()])
        .map_err(err)?;
    let version = build
        .seal(id(33), ResultCompleteness::Complete)
        .map_err(err)?;
    let read = classify(&version, None, vec![], Parallelism::default())?;
    let recount = read.reconcile(|| false).map_err(err)?;
    assert!(recount.source_coverage_complete);
    assert_eq!(recount.observed_files, 1);
    let totals = read.totals(&[], || false).map_err(err)?;
    let f03 = version.space(0).ok_or("missing total")?;
    assert_eq!(f03.logical.amount, SizeAmount::Exact { bytes: 10.into() });
    assert_eq!(totals.scope.logical.amount, f03.logical.amount);
    Ok(())
}
