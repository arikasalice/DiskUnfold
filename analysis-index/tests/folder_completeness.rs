//! A Missing result does not promote folders without coverage evidence (F03 FR5.HD1).
use du_analysis_index::query::{self, page::ResultSets};
use du_analysis_index::{
    BuildHandle, FileIdentity, IngestSink, Observation, ResultVersion, SharingEvidence,
    SpaceObservation, statistics_summary,
};
use du_core_contracts::result::{
    ResultCompleteness, ResultNativePath, ResultScopeRoot, ResultVersionRead, SpaceAggregate,
};
use du_core_contracts::{
    ByteSize, Id, ItemRef, PathBytes, PathEncoding, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
};

type Test = Result<(), String>;

fn id(n: u8) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}
fn e<T: std::fmt::Debug>(error: T) -> String {
    format!("{error:?}")
}
fn root(name: &[u8], n: u8) -> ResultScopeRoot {
    ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(name),
        },
        volume_id: vec![n],
        file_id: vec![n],
    }
}
fn path(parts: &[&str]) -> Vec<Vec<u8>> {
    parts.iter().map(|part| part.as_bytes().to_vec()).collect()
}
fn size(basis: SizeBasis, evidence: SizeEvidence, bytes: u64) -> SizeValue {
    SizeValue {
        basis,
        evidence,
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
fn lower(bytes: u64) -> SizeAmount {
    SizeAmount::LowerBound {
        bytes: ByteSize::from(bytes),
    }
}
fn builder(roots: &[ResultScopeRoot]) -> Result<BuildHandle, String> {
    builder_in(&IngestSink::default(), roots)
}
fn builder_in(sink: &IngestSink, roots: &[ResultScopeRoot]) -> Result<BuildHandle, String> {
    let mut build = sink.open_forest(id(1)?, roots.to_vec()).map_err(e)?;
    for index in 0..roots.len() as u32 {
        build.add_directory_at(index, vec![]).map_err(e)?;
    }
    Ok(build)
}
fn dir(build: &mut BuildHandle, root: u32, parts: &[&str]) -> Test {
    build.add_directory_at(root, path(parts)).map_err(e)
}
fn gap(build: &mut BuildHandle, root: u32, parts: &[&str]) -> Test {
    build
        .record_gap_at(root, path(parts), "scan.path_denied".into(), None)
        .map_err(e)
}
/// Adds a file with logical and allocated size `bytes` and the identity `file`.
fn file(build: &mut BuildHandle, root: u32, parts: &[&str], bytes: u64, file: Option<u8>) -> Test {
    build
        .add_with_space_at(
            root,
            Observation {
                path: path(parts),
                size: Some(size(SizeBasis::Logical, SizeEvidence::Measured, bytes)),
            },
            SpaceObservation {
                allocated: Some(size(SizeBasis::Allocated, SizeEvidence::OsReported, bytes)),
                identity: file.map(|file| FileIdentity {
                    volume: vec![1],
                    file: vec![file],
                }),
                link_count: Some(1),
                sharing: SharingEvidence::None,
            },
        )
        .map_err(e)
}
fn item(result: &ResultVersion, name: &str) -> Result<u32, String> {
    result
        .items()
        .iter()
        .find(|item| item.name == name.as_bytes())
        .map(|item| item.item_no)
        .ok_or_else(|| format!("no item {name}"))
}
fn space<'a>(result: &'a ResultVersion, name: &str) -> Result<&'a SpaceAggregate, String> {
    result
        .space(item(result, name)?)
        .ok_or("no aggregate".into())
}
fn completeness(a: &SpaceAggregate) -> [ResultCompleteness; 3] {
    [
        a.completeness,
        a.allocated_completeness,
        a.unique_physical_completeness,
    ]
}
fn assert_exact(a: &SpaceAggregate, bytes: u64) {
    assert_eq!(a.logical.amount, exact(bytes));
    assert_eq!(a.allocated.amount, exact(bytes));
    assert_eq!(a.unique_physical.amount, exact(bytes));
    assert_eq!(completeness(a), [ResultCompleteness::Complete; 3]);
    assert_eq!(a.inaccessible_ranges, None);
}
/// A gap ancestor has no exact value and no Complete state in any basis.
fn assert_lowered(a: &SpaceAggregate, ranges: u64) {
    for value in [&a.logical, &a.allocated, &a.unique_physical] {
        assert!(
            !matches!(value.amount, SizeAmount::Exact { .. }),
            "{value:?}"
        );
    }
    assert_eq!(completeness(a), [ResultCompleteness::Missing; 3]);
    assert_eq!(a.inaccessible_ranges, Some(ranges));
}

#[test]
fn missing_version_with_a_known_gap_keeps_unproven_folders_missing() -> Test {
    let mut b = builder(&[root(b"/r", 1)])?;
    for parts in [&["a"][..], &["a", "bad"], &["s"], &["empty"]] {
        dir(&mut b, 0, parts)?;
    }
    file(&mut b, 0, &["a", "af"], 10, Some(1))?;
    file(&mut b, 0, &["s", "sf"], 20, Some(2))?;
    gap(&mut b, 0, &["a", "bad"])?;
    let result = b.seal(id(2)?, ResultCompleteness::Missing).map_err(e)?;
    assert_eq!(result.completeness(), ResultCompleteness::Missing);
    for (name, bytes) in [("s", 20), ("sf", 20), ("af", 10)] {
        let aggregate = space(&result, name)?;
        assert_eq!(completeness(aggregate), [ResultCompleteness::Missing; 3]);
        assert_eq!(aggregate.logical.amount, lower(bytes));
        for value in [
            &aggregate.logical,
            &aggregate.allocated,
            &aggregate.unique_physical,
        ] {
            assert!(!matches!(value.amount, SizeAmount::Exact { .. }), "{name}");
        }
    }
    let bad = space(&result, "bad")?;
    assert_lowered(bad, 1);
    assert_eq!(bad.logical.amount, SizeAmount::Unknown);
    let empty = space(&result, "empty")?;
    assert_eq!(completeness(empty), [ResultCompleteness::Missing; 3]);
    assert_eq!(empty.logical.amount, SizeAmount::Unknown);
    let a = space(&result, "a")?;
    assert_lowered(a, 1);
    assert_eq!(a.logical.amount, lower(10));
    assert_eq!(a.allocated.amount, lower(10));
    assert_eq!(a.unique_physical.amount, lower(10));
    let top = result.space(0).ok_or("root")?;
    assert_lowered(top, 1);
    assert_eq!(top.logical.amount, lower(30));
    assert_eq!(result.scope_total(), top);
    Ok(())
}

#[test]
fn gap_on_an_unobserved_child_does_not_complete_other_folders() -> Test {
    let mut b = builder(&[root(b"/r", 1)])?;
    for parts in [&["a"][..], &["a", "sub"], &["s"]] {
        dir(&mut b, 0, parts)?;
    }
    file(&mut b, 0, &["a", "sub", "x"], 5, Some(1))?;
    file(&mut b, 0, &["s", "y"], 7, Some(2))?;
    gap(&mut b, 0, &["a", "ghost"])?;
    let result = b.seal(id(2)?, ResultCompleteness::Missing).map_err(e)?;
    for (name, bytes) in [("sub", 5), ("s", 7)] {
        let aggregate = space(&result, name)?;
        assert_eq!(aggregate.completeness, ResultCompleteness::Missing);
        assert_eq!(aggregate.logical.amount, lower(bytes));
    }
    assert_lowered(space(&result, "a")?, 1);
    assert_lowered(result.space(0).ok_or("root")?, 1);
    Ok(())
}

#[test]
fn a_missing_size_and_gap_keep_an_unproven_sibling_missing() -> Test {
    let mut b = builder(&[root(b"/r", 1)])?;
    for parts in [&["u"][..], &["s"], &["g"]] {
        dir(&mut b, 0, parts)?;
    }
    file(&mut b, 0, &["u", "known"], 10, Some(1))?;
    b.add_at(
        0,
        Observation {
            path: path(&["u", "mystery"]),
            size: None,
        },
    )
    .map_err(e)?;
    file(&mut b, 0, &["s", "sf"], 20, Some(2))?;
    gap(&mut b, 0, &["g"])?;
    let result = b.seal(id(2)?, ResultCompleteness::Missing).map_err(e)?;
    let sibling = space(&result, "s")?;
    assert_eq!(sibling.completeness, ResultCompleteness::Missing);
    assert_eq!(sibling.logical.amount, lower(20));
    let u = space(&result, "u")?;
    assert_eq!(u.logical.amount, lower(10));
    assert_eq!(u.completeness, ResultCompleteness::Missing);
    assert_eq!(u.missing_items, 1);
    assert_eq!(u.inaccessible_ranges, None);
    let top = result.space(0).ok_or("root")?;
    assert_eq!(top.completeness, ResultCompleteness::Missing);
    assert_eq!(top.logical.amount, lower(30));
    assert_eq!(top.inaccessible_ranges, Some(1));
    Ok(())
}

#[test]
fn a_root_without_a_gap_stays_missing_when_another_root_has_one() -> Test {
    let mut b = builder(&[root(b"/a", 1), root(b"/b", 2)])?;
    dir(&mut b, 1, &["bad"])?;
    file(&mut b, 0, &["af"], 10, Some(1))?;
    file(&mut b, 1, &["bf"], 5, Some(2))?;
    gap(&mut b, 1, &["bad"])?;
    let result = b.seal(id(2)?, ResultCompleteness::Missing).map_err(e)?;
    assert_eq!(result.completeness(), ResultCompleteness::Missing);
    let roots: Vec<u32> = result.scope_roots().iter().map(|r| r.item_no).collect();
    let first = result.space(roots[0]).ok_or("root a")?;
    assert_eq!(first.completeness, ResultCompleteness::Missing);
    assert_eq!(first.logical.amount, lower(10));
    let second = result.space(roots[1]).ok_or("root b")?;
    assert_lowered(second, 1);
    assert_eq!(second.logical.amount, lower(5));
    let total = result.scope_total();
    assert_lowered(total, 1);
    assert_eq!(total.logical.amount, lower(15));
    assert_eq!(total.allocated.amount, lower(15));
    Ok(())
}

#[test]
fn sealed_missing_without_a_gap_keeps_every_folder_missing() -> Test {
    let mut b = builder(&[root(b"/r", 1)])?;
    dir(&mut b, 0, &["s"])?;
    dir(&mut b, 0, &["empty"])?;
    file(&mut b, 0, &["s", "sf"], 20, Some(1))?;
    let result = b.seal(id(2)?, ResultCompleteness::Missing).map_err(e)?;
    for name in ["s", "empty"] {
        let a = space(&result, name)?;
        assert_eq!(completeness(a), [ResultCompleteness::Missing; 3], "{name}");
        assert!(
            !matches!(a.logical.amount, SizeAmount::Exact { .. }),
            "{name}"
        );
        assert_eq!(a.inaccessible_ranges, None);
    }
    let top = result.space(0).ok_or("root")?;
    assert_eq!(top.logical.amount, lower(20));
    assert_eq!(top.allocated.amount, lower(20));
    assert_eq!(top.completeness, ResultCompleteness::Missing);
    Ok(())
}

#[test]
fn complete_source_with_a_gap_does_not_complete_unrelated_folders() -> Test {
    let mut build = builder(&[root(b"/r", 1)])?;
    dir(&mut build, 0, &["gap"])?;
    dir(&mut build, 0, &["other"])?;
    file(&mut build, 0, &["other", "file"], 10, Some(1))?;
    gap(&mut build, 0, &["gap"])?;

    let result = build
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(e)?;

    assert_eq!(result.completeness(), ResultCompleteness::Missing);
    let other = space(&result, "other")?;
    assert_eq!(completeness(other), [ResultCompleteness::Missing; 3]);
    assert_eq!(other.logical.amount, lower(10));
    assert!(!matches!(other.allocated.amount, SizeAmount::Exact { .. }));
    assert!(!matches!(
        other.unique_physical.amount,
        SizeAmount::Exact { .. }
    ));
    Ok(())
}

#[test]
fn replacement_gap_does_not_prove_other_folders_are_complete() -> Test {
    let sink = IngestSink::default();
    let lineage = sink
        .bind_current_analysis("replacement-gap", id(4)?)
        .map_err(e)?;
    let mut original = sink
        .open_forest_in_lineage(id(1)?, vec![root(b"/r", 1)], lineage)
        .map_err(e)?;
    dir(&mut original, 0, &[])?;
    dir(&mut original, 0, &["old"])?;
    file(&mut original, 0, &["old", "file"], 30, Some(3))?;
    let base = original
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(e)?;
    let mut replacement = sink.open_replacement(&base).map_err(e)?;
    dir(&mut replacement, 0, &[])?;
    dir(&mut replacement, 0, &["gap"])?;
    dir(&mut replacement, 0, &["other"])?;
    file(&mut replacement, 0, &["other", "file"], 10, Some(1))?;
    gap(&mut replacement, 0, &["gap"])?;
    let result = replacement
        .seal(id(3)?, ResultCompleteness::Missing)
        .map_err(e)?;
    assert_eq!(result.completeness(), ResultCompleteness::Missing);
    assert_eq!(result.base_version_id(), Some(base.version_id()));
    assert_eq!(result.lineage_id(), base.lineage_id());
    assert_eq!(result.data_source(), base.data_source());
    assert!(item(&result, "old").is_err());
    assert_exact(space(&base, "old")?, 30);
    assert_eq!(result.scope_gaps().len(), 1);
    let other = space(&result, "other")?;
    assert_eq!(completeness(other), [ResultCompleteness::Missing; 3]);
    assert_eq!(other.logical.amount, lower(10));
    assert!(!matches!(other.allocated.amount, SizeAmount::Exact { .. }));
    assert!(!matches!(
        other.unique_physical.amount,
        SizeAmount::Exact { .. }
    ));
    Ok(())
}

#[test]
fn partial_and_cancelled_versions_stay_version_wide() -> Test {
    let mut b = builder(&[root(b"/r", 1)])?;
    for parts in [&["a"][..], &["a", "bad"], &["s"], &["empty"]] {
        dir(&mut b, 0, parts)?;
    }
    file(&mut b, 0, &["s", "sf"], 20, Some(2))?;
    gap(&mut b, 0, &["a", "bad"])?;
    let partial = b.publish_partial(id(2)?).map_err(e)?;
    let cancelled = b.seal(id(3)?, ResultCompleteness::Cancelled).map_err(e)?;
    for (result, state) in [
        (&partial, ResultCompleteness::Partial),
        (&cancelled, ResultCompleteness::Cancelled),
    ] {
        assert_eq!(result.completeness(), state);
        for name in ["s", "sf", "a", "empty"] {
            let a = space(result, name)?;
            assert_eq!(completeness(a), [state; 3], "{name}");
            assert!(
                !matches!(a.logical.amount, SizeAmount::Exact { .. }),
                "{name}"
            );
        }
        assert_eq!(space(result, "s")?.logical.amount, lower(20));
    }
    Ok(())
}

#[test]
fn shared_identities_and_missing_evidence_stay_lower_bounds() -> Test {
    let mut b = builder(&[root(b"/r", 1)])?;
    for parts in [&["a"][..], &["a", "bad"], &["s"], &["m"]] {
        dir(&mut b, 0, parts)?;
    }
    file(&mut b, 0, &["a", "af"], 10, Some(7))?;
    file(&mut b, 0, &["s", "sf"], 10, Some(7))?;
    file(&mut b, 0, &["m", "mf"], 10, None)?;
    gap(&mut b, 0, &["a", "bad"])?;
    let result = b.seal(id(2)?, ResultCompleteness::Missing).map_err(e)?;
    // The sibling counts a shared identity once, but Missing does not prove full coverage.
    let sibling = space(&result, "s")?;
    assert_eq!(sibling.completeness, ResultCompleteness::Missing);
    assert_eq!(sibling.logical.amount, lower(10));
    let a = space(&result, "a")?;
    assert_lowered(a, 1);
    assert_eq!(a.allocated.amount, lower(10));
    // A file without a usable identity keeps its folder's physical bases inexact.
    let m = space(&result, "m")?;
    assert_eq!(m.logical.amount, lower(10));
    assert_eq!(m.completeness, ResultCompleteness::Missing);
    assert!(!matches!(m.allocated.amount, SizeAmount::Exact { .. }));
    assert!(!matches!(
        m.unique_physical.amount,
        SizeAmount::Exact { .. }
    ));
    assert_eq!(m.allocated_completeness, ResultCompleteness::Missing);
    assert_eq!(m.unique_physical_completeness, ResultCompleteness::Missing);
    // The shared identity is counted once in the root; the missing file leaves unique unknown.
    let top = result.space(0).ok_or("root")?;
    assert_eq!(top.allocated.amount, lower(10));
    assert!(!matches!(
        top.unique_physical.amount,
        SizeAmount::Exact { .. }
    ));
    Ok(())
}

#[test]
fn folder_listings_and_summaries_use_the_folder_state() -> Test {
    let sink = IngestSink::default();
    let mut b = builder_in(&sink, &[root(b"/r", 1)])?;
    for parts in [&["a"][..], &["a", "bad"], &["s"]] {
        dir(&mut b, 0, parts)?;
    }
    file(&mut b, 0, &["a", "af"], 10, Some(1))?;
    file(&mut b, 0, &["s", "sf"], 20, Some(2))?;
    gap(&mut b, 0, &["a", "bad"])?;
    let result = b.seal(id(2)?, ResultCompleteness::Missing).map_err(e)?;
    let reference = |name: &str| -> Result<ItemRef, String> {
        Ok(ItemRef {
            result_version: id(2)?,
            item_no: item(&result, name)?,
        })
    };
    for (name, state) in [
        ("s", ResultCompleteness::Missing),
        ("a", ResultCompleteness::Missing),
    ] {
        let summary =
            statistics_summary::summarize(&result, &reference(name)?, &mut || false).map_err(e)?;
        assert_eq!(summary.result_completeness, state, "{name}");
        let listing = query::run_folder(
            &query::Registry::baseline(),
            result.clone(),
            Some(&reference(name)?),
            &query::Condition::All(vec![]),
            query::Sort {
                id: "sort.size",
                revision: 1,
                basis: query::Basis::Logical,
                descending: true,
            },
            || false,
        )
        .map_err(e)?;
        let store = ResultSets::new(sink.clone());
        store.register(id(3)?, listing).map_err(e)?;
        let page = store.open_page(&id(3)?, &id(2)?, 0, None).map_err(e)?;
        assert_eq!(page.metadata().completeness, state, "{name}");
        assert_eq!(result.completeness(), ResultCompleteness::Missing);
    }
    Ok(())
}

#[test]
fn summary_keeps_source_completeness_separate_from_folder_aggregate() -> Test {
    let mut b = builder(&[root(b"/r", 1)])?;
    dir(&mut b, 0, &["folder"])?;
    b.add_untraversed_link_at(0, path(&["folder", "link"]))
        .map_err(e)?;
    let result = b.seal(id(2)?, ResultCompleteness::Complete).map_err(e)?;
    let folder = ItemRef {
        result_version: id(2)?,
        item_no: item(&result, "folder")?,
    };
    assert_eq!(
        space(&result, "folder")?.completeness,
        ResultCompleteness::Missing
    );
    let summary = statistics_summary::summarize(&result, &folder, &mut || false).map_err(e)?;
    assert_eq!(summary.result_completeness, result.completeness());
    assert_eq!(summary.result_completeness, ResultCompleteness::Complete);
    assert_eq!(summary.aggregate.completeness, ResultCompleteness::Missing);
    assert_eq!(
        summary.aggregate.allocated_completeness,
        ResultCompleteness::Missing
    );
    assert_eq!(
        summary.aggregate.unique_physical_completeness,
        ResultCompleteness::Missing
    );
    Ok(())
}
