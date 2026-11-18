mod folder_support;

use du_analysis_index::{
    IngestSink, Observation, QueryError, SampleSessionKey,
    folder_collection::{FolderBoundary, FolderCollection, REASON_DATA_INSUFFICIENT, Unenumerated},
};
use du_core_contracts::{
    ItemRef,
    result::{ResultCompleteness, ResultVersionRead},
};
use folder_support::{
    OTHER_VOLUME, REVISIT, UNPROVEN, UNVERIFIABLE, bounded, dirs, err, forest, gap, id, item, root,
    state, unentered, unenumerated,
};

#[test]
fn sample_versions_do_not_provide_the_facts_even_with_marks() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = sink
        .open_sample_forest(id(1), vec![root(b"/s")], &SampleSessionKey::new(id(9)))
        .map_err(err)?;
    build.add_directory_at(0, Vec::new()).map_err(err)?;
    dirs(&mut build, 0, &[&["a"], &["denied"], &["mnt"]])?;
    gap(&mut build, 0, &["denied"], "scan.path_denied")?;
    unentered(&mut build, 0, &["mnt"], OTHER_VOLUME)?;
    let version = build
        .seal(id(2), ResultCompleteness::Complete)
        .map_err(err)?;
    let expected = FolderCollection::Folder {
        unenumerated: Unenumerated::Unknown {
            reason_key: REASON_DATA_INSUFFICIENT,
        },
        boundary: FolderBoundary::NotProvided,
    };
    for parts in [&[][..], &["a"], &["denied"], &["mnt"]] {
        assert_eq!(state(&version, 0, parts)?, expected);
    }
    Ok(())
}

#[test]
fn replacement_answers_from_its_own_records_and_leaves_the_base_pinned() -> Result<(), String> {
    let sink = IngestSink::default();
    let lineage = sink
        .bind_current_analysis("folder-replacement", id(40))
        .map_err(err)?;
    let mut base = sink
        .open_forest_in_lineage(id(41), vec![root(b"/r")], lineage)
        .map_err(err)?;
    base.add_directory_at(0, Vec::new()).map_err(err)?;
    dirs(&mut base, 0, &[&["a"], &["denied"]])?;
    gap(&mut base, 0, &["denied"], "scan.path_denied")?;
    unentered(&mut base, 0, &["a"], OTHER_VOLUME)?;
    let old = base
        .seal(id(42), ResultCompleteness::Complete)
        .map_err(err)?;
    let mut candidate = sink.open_replacement(&old).map_err(err)?;
    candidate.add_directory_at(0, Vec::new()).map_err(err)?;
    dirs(&mut candidate, 0, &[&["a"], &["denied"]])?;
    unentered(&mut candidate, 0, &["denied"], REVISIT)?;
    let new = candidate
        .seal(id(43), ResultCompleteness::Complete)
        .map_err(err)?;
    assert_eq!(new.origin(), "incremental");
    // Nothing is inherited: the candidate recorded no gap and no issue for "a".
    assert_eq!(state(&new, 0, &["a"])?, UNPROVEN);
    assert_eq!(
        state(&new, 0, &["denied"])?,
        bounded(FolderBoundary::AlreadyCounted)
    );
    assert_eq!(
        state(&old, 0, &["a"])?,
        bounded(FolderBoundary::OtherVolume)
    );
    assert_eq!(
        state(&old, 0, &["denied"])?,
        unenumerated("scan.path_denied")
    );
    Ok(())
}

#[test]
fn page_call_keeps_order_and_single_call_agrees() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = forest(&sink, 1, 1)?;
    dirs(&mut build, 0, &[&["a"], &["denied"], &["mnt"]])?;
    build
        .add_file_at(
            0,
            Observation {
                path: folder_support::folder_path(&["f"]),
                size: None,
            },
        )
        .map_err(err)?;
    gap(&mut build, 0, &["denied"], "scan.path_denied")?;
    unentered(&mut build, 0, &["mnt"], OTHER_VOLUME)?;
    let version = build
        .seal(id(2), ResultCompleteness::Complete)
        .map_err(err)?;
    let names: [&[&str]; 5] = [&["mnt"], &["f"], &["denied"], &[], &["a"]];
    let refs = names
        .iter()
        .map(|parts| item(&version, 0, parts))
        .collect::<Result<Vec<_>, _>>()?;
    let page = version.folder_collections(&refs).map_err(err)?;
    assert_eq!(
        page,
        vec![
            bounded(FolderBoundary::OtherVolume),
            FolderCollection::NotAFolder,
            unenumerated("scan.path_denied"),
            UNPROVEN,
            UNPROVEN,
        ]
    );
    for (reference, answer) in refs.iter().zip(&page) {
        assert_eq!(&version.folder_collection(reference).map_err(err)?, answer);
    }
    assert_eq!(version.folder_collections(&[]).map_err(err)?, vec![]);
    Ok(())
}

#[test]
fn page_and_single_calls_report_stale_and_missing_items() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = forest(&sink, 1, 1)?;
    dirs(&mut build, 0, &[&["a"]])?;
    let version = build
        .seal(id(2), ResultCompleteness::Complete)
        .map_err(err)?;
    let good = item(&version, 0, &["a"])?;
    let stale = ItemRef {
        result_version: id(7),
        item_no: good.item_no,
    };
    let missing = ItemRef {
        result_version: good.result_version.clone(),
        item_no: u32::MAX,
    };
    assert_eq!(
        version.folder_collection(&stale),
        Err(QueryError::StaleVersion)
    );
    assert_eq!(
        version.folder_collection(&missing),
        Err(QueryError::NotFound)
    );
    assert_eq!(
        version.folder_collections(&[good.clone(), stale.clone()]),
        Err(QueryError::StaleVersion)
    );
    assert_eq!(
        version.folder_collections(&[good, missing]),
        Err(QueryError::NotFound)
    );
    Ok(())
}

#[test]
fn a_folder_is_never_unenumerated_and_bounded_at_once() -> Result<(), String> {
    let mut versions = Vec::new();
    for (number, completeness) in [
        (1, ResultCompleteness::Complete),
        (3, ResultCompleteness::Partial),
        (5, ResultCompleteness::Cancelled),
    ] {
        let sink = IngestSink::default();
        let mut build = forest(&sink, number, 2)?;
        dirs(&mut build, 0, &[&["a"], &["b"], &["c"], &["d"], &["e"]])?;
        dirs(&mut build, 1, &[&["a"]])?;
        for name in ["a", "b", "c"] {
            unentered(&mut build, 0, &[name], OTHER_VOLUME)?;
        }
        unentered(&mut build, 0, &["d"], REVISIT)?;
        unentered(&mut build, 1, &[], REVISIT)?;
        unentered(&mut build, 1, &["a"], OTHER_VOLUME)?;
        gap(&mut build, 0, &["a"], "scan.path_denied")?;
        gap(&mut build, 0, &["d"], UNVERIFIABLE)?;
        gap(&mut build, 1, &[], "scan.disconnected")?;
        versions.push(build.seal(id(number + 1), completeness).map_err(err)?);
    }
    let mut yes = 0;
    let mut bounded_count = 0;
    for version in &versions {
        for item in version.items() {
            let reference = ItemRef {
                result_version: version.version_id().clone(),
                item_no: item.item_no,
            };
            if let FolderCollection::Folder {
                unenumerated,
                boundary,
            } = version.folder_collection(&reference).map_err(err)?
            {
                let is_yes = matches!(unenumerated, Unenumerated::Yes { .. });
                yes += usize::from(is_yes);
                bounded_count += usize::from(boundary.name().is_some());
                assert!(
                    !(is_yes && boundary.name().is_some()),
                    "item {}",
                    item.item_no
                );
            }
        }
    }
    assert!(yes > 0 && bounded_count > 0);
    Ok(())
}
