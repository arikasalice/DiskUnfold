//! scan.failures `underItem`: the rows of one folder and everything below it (F02 #358).
use super::*;
use du_analysis_index::{
    FileIdentity, Observation, ResultVersion, SharingEvidence, SpaceObservation,
};
use du_core_contracts::{
    ItemRef, SizeAmount, SizeBasis, SizeEvidence, SizeValue, result::ResultVersionRead,
};

fn unix_root(name: &[u8]) -> ResultScopeRoot {
    ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(name),
        },
        volume_id: vec![1],
        file_id: name.to_vec(),
    }
}

fn path(parts: &[&str]) -> Vec<Vec<u8>> {
    parts.iter().map(|part| part.as_bytes().to_vec()).collect()
}

fn issue(root: u32, parts: &[&str], kind: ScanFailureKind) -> ScanIssue {
    ScanIssue {
        root_index: root,
        relative: path(parts),
        kind,
        stage: ScanFailureStage::OpenDirectory,
        reason_key: "scan.unclassified".into(),
        observed_at: None,
    }
}

/// Two roots, each with folders `b`, `bc` and `b/x`; root 0 also holds the file `b/f`.
/// Root 0 has issues at its root, at `b`, `b/x`, `bc` and `b/f`; root 1 at `b`.
fn forest() -> Result<(IngestSink, Id, std::sync::Arc<ResultVersion>), String> {
    let store = IngestSink::default();
    let mut build = store
        .open_forest(id(1)?, vec![unix_root(b"/r0"), unix_root(b"/r1")])
        .map_err(|e| format!("{e:?}"))?;
    for root in 0..2 {
        build
            .add_directory_at(root, Vec::new())
            .map_err(|e| format!("{e:?}"))?;
        for folder in [&["b"][..], &["bc"], &["b", "x"]] {
            build
                .add_directory_at(root, path(folder))
                .map_err(|e| format!("{e:?}"))?;
        }
    }
    let bytes = |basis, evidence, amount: u64| SizeValue {
        basis,
        evidence,
        amount: SizeAmount::Exact {
            bytes: amount.into(),
        },
    };
    build
        .add_with_space_at(
            0,
            Observation {
                path: path(&["b", "f"]),
                size: Some(bytes(SizeBasis::Logical, SizeEvidence::Measured, 1)),
            },
            SpaceObservation {
                allocated: Some(bytes(SizeBasis::Allocated, SizeEvidence::OsReported, 4096)),
                identity: Some(FileIdentity {
                    volume: vec![1],
                    file: vec![1],
                }),
                link_count: Some(1),
                sharing: SharingEvidence::None,
            },
        )
        .map_err(|e| format!("{e:?}"))?;
    for (root, parts, kind) in [
        (0, &[][..], ScanFailureKind::UnreadRange),
        (0, &["b"][..], ScanFailureKind::UnreadRange),
        (0, &["b", "x"][..], ScanFailureKind::CollectionFailed),
        (0, &["b", "f"][..], ScanFailureKind::Uncertain),
        (0, &["bc"][..], ScanFailureKind::UnreadRange),
        (1, &["b"][..], ScanFailureKind::UnreadRange),
    ] {
        build
            .record_scan_issue(issue(root, parts, kind))
            .map_err(|e| format!("{e:?}"))?;
    }
    let version = id(2)?;
    let sealed = build
        .seal(version.clone(), ResultCompleteness::Missing)
        .map_err(|e| format!("{e:?}"))?;
    Ok((store, version, sealed))
}

fn item(version: &ResultVersion, root: usize, parts: &[&str]) -> Result<ItemRef, String> {
    let mut current = version.scope_roots().get(root).ok_or("no root")?.item_no;
    for part in parts {
        current = version
            .items()
            .iter()
            .find(|entry| entry.parent == Some(current) && entry.name == part.as_bytes())
            .ok_or("missing item")?
            .item_no;
    }
    Ok(ItemRef {
        result_version: version.version_id().clone(),
        item_no: current,
    })
}

fn shown(response: &ResultEnvelope) -> Vec<String> {
    response.body["rows"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .map(|row| {
                    let root = row["pathRef"]["rootIndex"].to_string();
                    let parts: Vec<_> = row["pathRef"]["relative"]
                        .as_array()
                        .map(|parts| {
                            parts
                                .iter()
                                .filter_map(|part| part.as_str())
                                .filter_map(|part| PathBytes::parse(part).ok())
                                .filter_map(|part| part.bytes().ok())
                                .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
                                .collect()
                        })
                        .unwrap_or_default();
                    format!("{root}:{}", parts.join("/"))
                })
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn a_folder_keeps_its_own_row_and_those_below_and_nothing_beside_or_above() -> Result<(), String> {
    let (store, version, sealed) = forest()?;
    let engine = engine(store, true)?;
    let b = item(&sealed, 0, &["b"])?;
    let response = page(&engine, json!({"resultVersion": version, "underItem": b}))?;
    assert!(response.error.is_none());
    // b, b/f and b/x: not the root row above it, not bc beside it, not root 1's b.
    assert_eq!(shown(&response), ["0:b", "0:b/f", "0:b/x"]);
    assert_eq!(response.body["totalCount"], 3);
    // The scope root holds every row of its own root, and none of root 1.
    let all = page(
        &engine,
        json!({"resultVersion": version, "underItem": item(&sealed, 0, &[])?}),
    )?;
    assert_eq!(all.body["totalCount"], 5);
    // The other root's folder of the same name takes only its own row.
    let other = page(
        &engine,
        json!({"resultVersion": version, "underItem": item(&sealed, 1, &["b"])?}),
    )?;
    assert_eq!(shown(&other), ["1:b"]);
    Ok(())
}

#[test]
fn the_folder_combines_with_the_other_filters_by_and() -> Result<(), String> {
    let (store, version, sealed) = forest()?;
    let engine = engine(store, true)?;
    let b = item(&sealed, 0, &["b"])?;
    let kind = page(
        &engine,
        json!({"resultVersion": version, "underItem": b, "kind": "collection_failed"}),
    )?;
    assert_eq!(shown(&kind), ["0:b/x"]);
    let failures = page(
        &engine,
        json!({"resultVersion": version, "underItem": b, "failuresOnly": true}),
    )?;
    assert_eq!(shown(&failures), ["0:b", "0:b/x"]);
    let excluded = page(
        &engine,
        json!({"resultVersion": version, "underItem": b, "rootIndexes": [1]}),
    )?;
    assert_eq!(excluded.body["totalCount"], 0);
    Ok(())
}

#[test]
fn a_cursor_keeps_to_its_folder() -> Result<(), String> {
    let (store, version, sealed) = forest()?;
    let engine = engine(store, true)?;
    let b = item(&sealed, 0, &["b"])?;
    let first = page(
        &engine,
        json!({"resultVersion": version, "underItem": b, "count": 2}),
    )?;
    assert_eq!(shown(&first), ["0:b", "0:b/f"]);
    let cursor = first.body["nextCursor"].as_str().ok_or("no cursor")?;
    let rest = page(
        &engine,
        json!({"resultVersion": version, "underItem": b, "cursor": cursor}),
    )?;
    assert_eq!(shown(&rest), ["0:b/x"]);
    // Another folder, or none, does not continue it.
    for body in [
        json!({"resultVersion": version, "underItem": item(&sealed, 0, &["bc"])?, "cursor": cursor}),
        json!({"resultVersion": version, "cursor": cursor}),
    ] {
        assert_error(
            page(&engine, body)?,
            ErrorCode::InvalidArgument,
            "cursor.invalid",
        )?;
    }
    Ok(())
}

#[test]
fn the_reference_is_checked_for_version_then_item_then_kind() -> Result<(), String> {
    let (store, version, sealed) = forest()?;
    let engine = engine(store, true)?;
    let other_version = ItemRef {
        result_version: id(9)?,
        item_no: 0,
    };
    assert_error(
        page(
            &engine,
            json!({"resultVersion": version, "underItem": other_version}),
        )?,
        ErrorCode::InvalidArgument,
        "command.body_invalid",
    )?;
    let missing = ItemRef {
        result_version: version.clone(),
        item_no: 9_999,
    };
    assert_error(
        page(
            &engine,
            json!({"resultVersion": version, "underItem": missing}),
        )?,
        ErrorCode::NotFound,
        "ref.unavailable",
    )?;
    let file = item(&sealed, 0, &["b", "f"])?;
    assert_error(
        page(
            &engine,
            json!({"resultVersion": version, "underItem": file}),
        )?,
        ErrorCode::InvalidArgument,
        "command.body_invalid",
    )?;
    let null = page(
        &engine,
        json!({"resultVersion": version, "underItem": null}),
    )?;
    assert!(null.error.is_some());
    Ok(())
}
