use super::*;
use du_core_contracts::{
    PathEncoding,
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot},
    scan_failures::unentered_reason,
};

fn forest_fixture(
    root_count: usize,
    rows: &[(u32, ScanFailureKind, ScanFailureStage, &str)],
) -> Result<(IngestSink, Id), String> {
    let roots = (0..root_count)
        .map(|index| {
            let name = format!("C:\\Root{index}");
            let raw = name
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>();
            ResultScopeRoot {
                path: ResultNativePath {
                    encoding: PathEncoding::WindowsUtf16le,
                    raw: PathBytes::from_bytes(&raw),
                },
                volume_id: vec![(index % 254 + 1) as u8],
                file_id: (index as u64 + 1).to_le_bytes().to_vec(),
            }
        })
        .collect();
    let store = IngestSink::default();
    let mut build = store
        .open_forest(id(1)?, roots)
        .map_err(|e| format!("{e:?}"))?;
    for root in 0..root_count {
        build
            .add_directory_at(root as u32, Vec::new())
            .map_err(|e| format!("{e:?}"))?;
    }
    for (position, (root_index, kind, stage, reason_key)) in rows.iter().enumerate() {
        let name = format!("issue-{position}");
        let relative = name
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        build
            .record_scan_issue(ScanIssue {
                root_index: *root_index,
                relative: vec![relative],
                kind: *kind,
                stage: *stage,
                reason_key: (*reason_key).into(),
                observed_at: None,
            })
            .map_err(|e| format!("{e:?}"))?;
    }
    let version = id(2)?;
    build
        .seal(version.clone(), ResultCompleteness::Missing)
        .map_err(|e| format!("{e:?}"))?;
    Ok((store, version))
}

fn classified_fixture() -> Result<(IngestSink, Id), String> {
    use ScanFailureKind::{CollectionFailed, Uncertain, UnreadRange, UntraversedLink};
    use ScanFailureStage::{Enumerate, Metadata, OpenDirectory, Traverse};
    forest_fixture(
        2,
        &[
            (0, CollectionFailed, Metadata, "scan.path_denied"),
            (0, UnreadRange, OpenDirectory, "scan.path_denied"),
            (0, UnreadRange, Enumerate, "scan.disconnected"),
            (
                0,
                UnreadRange,
                Traverse,
                unentered_reason::BOUNDARY_UNVERIFIABLE,
            ),
            (
                0,
                UnreadRange,
                Traverse,
                unentered_reason::BOUNDARY_OTHER_VOLUME,
            ),
            (0, UnreadRange, Traverse, unentered_reason::REVISIT_BLOCKED),
            (0, UnreadRange, Traverse, "scan.unclassified"),
            (0, Uncertain, Metadata, "scan.item_unverifiable"),
            (0, UntraversedLink, Traverse, "scan.link_untraversed"),
            (1, CollectionFailed, Metadata, "scan.path_vanished"),
            (1, UnreadRange, OpenDirectory, "scan.path_denied"),
            (
                1,
                UnreadRange,
                Traverse,
                unentered_reason::BOUNDARY_OTHER_VOLUME,
            ),
        ],
    )
}

#[test]
fn root_indexes_filter_rows_and_total_count() -> Result<(), String> {
    let (store, version) = classified_fixture()?;
    let response = page(
        &engine(store, true)?,
        json!({"resultVersion":version,"rootIndexes":[1]}),
    )?;
    assert!(response.error.is_none());
    assert_eq!(response.body["totalCount"], 3);
    let rows = response.body["rows"].as_array().ok_or("missing rows")?;
    assert_eq!(rows.len(), 3);
    assert!(rows.iter().all(|row| row["pathRef"]["rootIndex"] == 1));
    Ok(())
}

#[test]
fn failures_only_matches_counted_failure_rows_and_intersects_kind() -> Result<(), String> {
    let (store, version) = classified_fixture()?;
    let failures_engine = engine(store, true)?;
    let response = page(
        &failures_engine,
        json!({"resultVersion":version,"failuresOnly":true}),
    )?;
    assert_eq!(response.body["totalCount"], 7);
    let rows = response.body["rows"].as_array().ok_or("missing rows")?;
    assert_eq!(rows.len(), 7);
    assert!(rows.iter().all(|row| {
        row["kind"] != "uncertain"
            && row["kind"] != "untraversed_link"
            && row["reasonKey"] != unentered_reason::BOUNDARY_OTHER_VOLUME
            && row["reasonKey"] != unentered_reason::REVISIT_BLOCKED
    }));
    for (kind, stage, reason) in [
        ("collection_failed", "metadata", "scan.path_denied"),
        ("unread_range", "open_directory", "scan.path_denied"),
        ("unread_range", "enumerate", "scan.disconnected"),
        (
            "unread_range",
            "traverse",
            unentered_reason::BOUNDARY_UNVERIFIABLE,
        ),
        ("unread_range", "traverse", "scan.unclassified"),
    ] {
        assert!(rows.iter().any(|row| {
            row["kind"] == kind && row["stage"] == stage && row["reasonKey"] == reason
        }));
    }
    let intersection = page(
        &failures_engine,
        json!({
            "resultVersion":version,
            "rootIndexes":[1],
            "kind":"unread_range",
            "failuresOnly":true
        }),
    )?;
    assert_eq!(intersection.body["totalCount"], 1);
    assert_eq!(intersection.body["rows"][0]["kind"], "unread_range");
    assert_eq!(intersection.body["rows"][0]["pathRef"]["rootIndex"], 1);
    Ok(())
}

#[test]
fn cursor_is_bound_to_roots_and_failures_only_but_version_stays_stale() -> Result<(), String> {
    let (store, version) = classified_fixture()?;
    let engine = engine(store.clone(), true)?;
    let first = page(
        &engine,
        json!({"resultVersion":version,"rootIndexes":[0],"count":1}),
    )?;
    let cursor = first.body["nextCursor"].as_str().ok_or("missing cursor")?;
    assert_error(
        page(
            &engine,
            json!({"resultVersion":version,"rootIndexes":[1],"cursor":cursor}),
        )?,
        ErrorCode::InvalidArgument,
        "cursor.invalid",
    )?;
    assert_error(
        page(
            &engine,
            json!({
                "resultVersion":version,
                "rootIndexes":[0],
                "failuresOnly":true,
                "cursor":cursor
            }),
        )?,
        ErrorCode::InvalidArgument,
        "cursor.invalid",
    )?;
    let other = id(3)?;
    replace(&store, other.clone())?;
    assert_error(
        page(
            &engine,
            json!({"resultVersion":other,"rootIndexes":[0],"cursor":cursor}),
        )?,
        ErrorCode::StaleVersion,
        "cursor.stale",
    )
}

#[test]
fn malformed_or_out_of_range_root_indexes_are_body_invalid() -> Result<(), String> {
    let (store, version) = classified_fixture()?;
    let initial_engine = engine(store, true)?;
    for indexes in [
        json!(null),
        json!([]),
        json!([0, 0]),
        json!([1, 0]),
        json!([2]),
    ] {
        assert_error(
            page(
                &initial_engine,
                json!({"resultVersion":version,"rootIndexes":indexes}),
            )?,
            ErrorCode::InvalidArgument,
            "command.body_invalid",
        )?;
    }
    let roots_64 = (0..64)
        .map(|root| {
            (
                root,
                ScanFailureKind::UnreadRange,
                ScanFailureStage::OpenDirectory,
                "scan.path_denied",
            )
        })
        .collect::<Vec<_>>();
    let (store_64, version_64) = forest_fixture(64, &roots_64)?;
    let engine_64 = engine(store_64, true)?;
    let accepted = page(
        &engine_64,
        json!({"resultVersion":version_64,"rootIndexes":(0..64).collect::<Vec<u32>>()}),
    )?;
    assert!(accepted.error.is_none());
    assert_eq!(accepted.body["totalCount"], 64);
    assert_error(
        page(
            &engine_64,
            json!({"resultVersion":version_64,"rootIndexes":(0..65).collect::<Vec<u32>>()}),
        )?,
        ErrorCode::InvalidArgument,
        "command.body_invalid",
    )
}
