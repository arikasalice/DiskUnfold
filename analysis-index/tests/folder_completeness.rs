//! Missing results stay version-wide without pinned folder-coverage evidence.
use du_analysis_index::query::{self, page::ResultSets};
use du_analysis_index::{
    BuildHandle, FileIdentity, IngestSink, Observation, ResultVersion, SharingEvidence,
    SpaceObservation, statistics_summary,
};
use du_core_contracts::result::{
    ResultCompleteness, ResultItemKind, ResultNativePath, ResultScopeRoot, ResultVersionRead,
    SpaceAggregate,
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
fn space(result: &ResultVersion, name: &str) -> Result<SpaceAggregate, String> {
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
fn assert_unproven(a: &SpaceAggregate) {
    for value in [&a.logical, &a.allocated, &a.unique_physical] {
        assert!(
            !matches!(value.amount, SizeAmount::Exact { .. }),
            "{value:?}"
        );
    }
    assert_eq!(completeness(a), [ResultCompleteness::Missing; 3]);
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
fn one_unreadable_folder_does_not_prove_its_siblings_complete() -> Test {
    let mut b = builder(&[root(b"/r", 1)])?;
    for parts in [&["a"][..], &["a", "bad"], &["s"]] {
        dir(&mut b, 0, parts)?;
    }
    file(&mut b, 0, &["a", "af"], 10, Some(1))?;
    file(&mut b, 0, &["s", "sf"], 20, Some(2))?;
    gap(&mut b, 0, &["a", "bad"])?;
    let result = b.seal(id(2)?, ResultCompleteness::Missing).map_err(e)?;
    assert_eq!(result.completeness(), ResultCompleteness::Missing);
    assert_unproven(&space(&result, "s")?);
    assert_eq!(space(&result, "s")?.logical.amount, lower(20));
    assert_unproven(&space(&result, "sf")?);
    assert_eq!(space(&result, "sf")?.logical.amount, lower(20));
    assert_unproven(&space(&result, "af")?);
    assert_eq!(space(&result, "af")?.logical.amount, lower(10));
    let bad = space(&result, "bad")?;
    assert_lowered(&bad, 1);
    // The unreadable folder is at least zero bytes, not unknown (#341); its unique
    // physical size stays unknown without bytes.
    assert_eq!(bad.logical.amount, lower(0));
    assert_eq!(bad.unique_physical.amount, SizeAmount::Unknown);
    // Allocation was collected in this scan, so the unread folder is at least zero
    // allocated bytes too.
    assert_eq!(bad.allocated.amount, lower(0));
    let a = space(&result, "a")?;
    assert_lowered(&a, 1);
    assert_eq!(a.logical.amount, lower(10));
    assert_eq!(a.allocated.amount, lower(10));
    assert_eq!(a.unique_physical.amount, lower(10));
    let top = result.space(0).ok_or("root")?;
    assert_lowered(&top, 1);
    assert_eq!(top.logical.amount, lower(30));
    assert_eq!(result.scope_total(), &top);
    Ok(())
}

/// #341: without allocation collected in the scan, an unread folder's allocated size
/// stays unknown ("not collected"); only its logical size is at least zero.
#[test]
fn an_unread_folder_keeps_an_uncollected_allocation_unknown() -> Test {
    let mut b = builder(&[root(b"/r", 1)])?;
    dir(&mut b, 0, &["bad"])?;
    b.add_at(
        0,
        Observation {
            path: path(&["f"]),
            size: Some(size(SizeBasis::Logical, SizeEvidence::Measured, 5)),
        },
    )
    .map_err(e)?;
    gap(&mut b, 0, &["bad"])?;
    let result = b.seal(id(2)?, ResultCompleteness::Missing).map_err(e)?;
    let bad = space(&result, "bad")?;
    assert_eq!(bad.logical.amount, lower(0));
    assert_eq!(bad.allocated.amount, SizeAmount::Unknown);
    Ok(())
}

#[test]
fn a_located_gap_does_not_prove_unrelated_folders_complete() -> Test {
    let mut b = builder(&[root(b"/r", 1)])?;
    for parts in [&["a"][..], &["a", "sub"], &["s"]] {
        dir(&mut b, 0, parts)?;
    }
    file(&mut b, 0, &["a", "sub", "x"], 5, Some(1))?;
    file(&mut b, 0, &["s", "y"], 7, Some(2))?;
    gap(&mut b, 0, &["a", "ghost"])?;
    let result = b.seal(id(2)?, ResultCompleteness::Missing).map_err(e)?;
    assert_unproven(&space(&result, "sub")?);
    assert_eq!(space(&result, "sub")?.logical.amount, lower(5));
    assert_unproven(&space(&result, "s")?);
    assert_eq!(space(&result, "s")?.logical.amount, lower(7));
    assert_lowered(&space(&result, "a")?, 1);
    assert_lowered(&result.space(0).ok_or("root")?, 1);
    Ok(())
}

#[test]
fn missing_item_evidence_does_not_make_an_unrelated_folder_exact() -> Test {
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
    assert_unproven(&space(&result, "s")?);
    assert_eq!(space(&result, "s")?.logical.amount, lower(20));
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
fn a_root_without_a_gap_still_stays_missing() -> Test {
    let mut b = builder(&[root(b"/a", 1), root(b"/b", 2)])?;
    dir(&mut b, 1, &["bad"])?;
    file(&mut b, 0, &["af"], 10, Some(1))?;
    file(&mut b, 1, &["bf"], 5, Some(2))?;
    gap(&mut b, 1, &["bad"])?;
    let result = b.seal(id(2)?, ResultCompleteness::Missing).map_err(e)?;
    assert_eq!(result.completeness(), ResultCompleteness::Missing);
    let roots: Vec<u32> = result.scope_roots().iter().map(|r| r.item_no).collect();
    let first = result.space(roots[0]).ok_or("root a")?;
    assert_unproven(&first);
    assert_eq!(first.logical.amount, lower(10));
    let second = result.space(roots[1]).ok_or("root b")?;
    assert_lowered(&second, 1);
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
        assert_eq!(completeness(&a), [ResultCompleteness::Missing; 3], "{name}");
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
fn replacement_gap_does_not_prove_other_folders_are_complete() -> Test {
    let sink = IngestSink::default();
    let lineage = sink
        .bind_current_analysis("replacement-gap", id(4)?)
        .map_err(e)?;
    let mut original = sink
        .open_forest_in_lineage(id(1)?, vec![root(b"/r", 1)], lineage)
        .map_err(e)?;
    dir(&mut original, 0, &[])?;
    dir(&mut original, 0, &["other"])?;
    file(&mut original, 0, &["other", "old"], 4, Some(9))?;
    let base = original
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(e)?;
    assert_eq!(space(&base, "other")?.logical.amount, exact(4));
    assert_eq!(
        space(&base, "other")?.completeness,
        ResultCompleteness::Complete
    );
    let mut replacement = sink.open_replacement(&base).map_err(e)?;
    dir(&mut replacement, 0, &[])?;
    dir(&mut replacement, 0, &["gap"])?;
    dir(&mut replacement, 0, &["other"])?;
    file(&mut replacement, 0, &["other", "file"], 10, Some(1))?;
    gap(&mut replacement, 0, &["gap"])?;
    let result = replacement
        .seal(id(3)?, ResultCompleteness::Missing)
        .map_err(e)?;
    let other = space(&result, "other")?;
    assert_eq!(other.completeness, ResultCompleteness::Missing);
    assert_eq!(other.logical.amount, lower(10));
    // Publishing the replacement does not rewrite the already pinned base version.
    assert_eq!(space(&base, "other")?.logical.amount, exact(4));
    assert_eq!(
        space(&base, "other")?.completeness,
        ResultCompleteness::Complete
    );
    Ok(())
}

#[test]
fn partial_and_cancelled_versions_stay_version_wide() -> Test {
    let mut b = builder(&[root(b"/r", 1)])?;
    for parts in [&["a"][..], &["a", "bad"], &["s"]] {
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
        for name in ["s", "sf", "a"] {
            let a = space(result, name)?;
            assert_eq!(a.completeness, state, "{name}");
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
    // The sibling shares an identity with a file under the gap but counts it once itself.
    assert_unproven(&space(&result, "s")?);
    assert_eq!(space(&result, "s")?.logical.amount, lower(10));
    let a = space(&result, "a")?;
    assert_lowered(&a, 1);
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
fn folder_listings_and_summaries_keep_the_version_state_without_folder_evidence() -> Test {
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
fn an_untraversed_link_is_not_a_scope_gap() -> Test {
    let mut b = builder(&[root(b"/r", 1)])?;
    dir(&mut b, 0, &["links"])?;
    dir(&mut b, 0, &["denied"])?;
    b.add_untraversed_link_at(0, path(&["links", "link"]))
        .map_err(e)?;
    gap(&mut b, 0, &["denied"])?;
    let result = b.seal(id(2)?, ResultCompleteness::Missing).map_err(e)?;

    let link = result
        .items()
        .iter()
        .find(|item| item.name.as_slice() == b"link")
        .ok_or("link item")?;
    assert_eq!(link.kind, ResultItemKind::UntraversedLink);
    assert_eq!(space(&result, "links")?.untraversed_links, 1);
    assert_eq!(space(&result, "links")?.inaccessible_ranges, None);
    assert_eq!(result.scope_gaps().len(), 1);
    assert_eq!(result.scope_gaps()[0].path, path(&["denied"]));
    assert_eq!(result.scope_gaps()[0].reason_key, "scan.path_denied");
    Ok(())
}

#[test]
fn link_summary_characterizes_version_state_folder_state_and_link_count() -> Test {
    let mut b = builder(&[root(b"/r", 1)])?;
    dir(&mut b, 0, &["folder"])?;
    b.add_untraversed_link_at(0, path(&["folder", "link"]))
        .map_err(e)?;
    let result = b.seal(id(2)?, ResultCompleteness::Complete).map_err(e)?;
    let folder = ItemRef {
        result_version: id(2)?,
        item_no: item(&result, "folder")?,
    };
    // This records the existing distinction; it does not resolve the summary contract.
    let folder_aggregate = space(&result, "folder")?;
    assert_eq!(folder_aggregate.completeness, ResultCompleteness::Missing);
    assert_eq!(folder_aggregate.untraversed_links, 1);
    let summary = statistics_summary::summarize(&result, &folder, &mut || false).map_err(e)?;
    assert_eq!(summary.result_completeness, ResultCompleteness::Complete);
    assert_eq!(summary.aggregate.completeness, ResultCompleteness::Missing);
    assert_eq!(summary.counts.untraversed_links, 1);
    Ok(())
}
