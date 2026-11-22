//! The folder scan state in the details of a folder (F02 #347).
use super::*;
use du_analysis_index::{Observation, SharingEvidence, SpaceObservation};
use du_core_contracts::{
    Id, PathEncoding,
    result::{ResultCompleteness, ResultVersionRead},
};
use serde_json::Value;

#[test]
fn detail_carries_the_folder_scan_state_of_its_row_and_none_for_a_file() -> Result<(), String> {
    let sink = IngestSink::default();
    let version = id(2)?;
    let mut build = sink
        .open_forest(id(1)?, vec![root(PathEncoding::Unix, b"/r")])
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_directory_at(0, vec![])
        .map_err(|e| format!("{e:?}"))?;
    for folder in ["clean", "locked"] {
        build
            .add_directory_at(0, vec![folder.as_bytes().to_vec()])
            .map_err(|e| format!("{e:?}"))?;
    }
    build
        .add_with_space_at(
            0,
            Observation {
                path: vec![b"clean".to_vec(), b"f".to_vec()],
                size: None,
            },
            SpaceObservation {
                allocated: None,
                identity: None,
                link_count: None,
                sharing: SharingEvidence::None,
            },
        )
        .map_err(|e| format!("{e:?}"))?;
    build
        .record_gap_at(
            0,
            vec![b"locked".to_vec(), b"deep".to_vec()],
            "scan.unclassified".into(),
            None,
        )
        .map_err(|e| format!("{e:?}"))?;
    let sealed = build
        .seal(version.clone(), ResultCompleteness::Missing)
        .map_err(|e| format!("{e:?}"))?;
    let mut builder = EngineBuilder::default();
    register(&mut builder, sink);
    let engine = builder.build().map_err(|e| e.to_string())?;
    let number = |parts: &[&str]| -> Result<u32, String> {
        let mut current = sealed.scope_roots().first().ok_or("no root")?.item_no;
        for part in parts {
            current = sealed
                .items()
                .iter()
                .find(|item| item.parent == Some(current) && item.name == part.as_bytes())
                .ok_or("missing")?
                .item_no;
        }
        Ok(current)
    };
    let flags = |parts: &[&str]| -> Result<Value, String> {
        let response = call_detail(&engine, &version, &version, number(parts)?)?;
        assert!(response.error.is_none());
        Ok(response
            .body
            .get("folderFlags")
            .cloned()
            .unwrap_or(Value::Null))
    };
    let clean = flags(&["clean"])?;
    // A hand-built version carries no folder attempt evidence, so that fact is unknown,
    // as it is on the folder page.
    assert_eq!(clean["unenumerated"]["state"], "unknown");
    assert_eq!(clean["unreadInside"]["state"], "absent");
    let locked = flags(&["locked"])?;
    assert_eq!(locked["unreadInside"]["state"], "present");
    // A file has no folder state, so the field is omitted rather than null.
    assert!(flags(&["clean", "f"])?.is_null());
    Ok(())
}

/// A one-root result with the folder `d` and, when `gap`, an unread place inside it.
fn folder_result(
    sink: &IngestSink,
    first: u8,
    gap: bool,
) -> Result<(Id, std::sync::Arc<ResultVersion>), String> {
    let mut build = sink
        .open_forest(id(first)?, vec![root(PathEncoding::Unix, b"/r")])
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_directory_at(0, vec![])
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_directory_at(0, vec![b"d".to_vec()])
        .map_err(|e| format!("{e:?}"))?;
    if gap {
        build
            .record_gap_at(
                0,
                vec![b"d".to_vec(), b"x".to_vec()],
                "scan.unclassified".into(),
                None,
            )
            .map_err(|e| format!("{e:?}"))?;
    }
    let version = id(first + 1)?;
    let sealed = build
        .seal(version.clone(), ResultCompleteness::Missing)
        .map_err(|e| format!("{e:?}"))?;
    Ok((version, sealed))
}

#[test]
fn each_version_answers_with_its_own_folder_scan_state_and_a_replaced_one_is_stale()
-> Result<(), String> {
    let sink = IngestSink::default();
    let (old_id, old) = folder_result(&sink, 20, false)?;
    let folder = |version: &ResultVersion| {
        version
            .items()
            .iter()
            .find(|item| item.name == b"d")
            .map(|item| item.item_no)
            .ok_or_else(|| "no folder".to_owned())
    };
    let old_no = folder(&old)?;
    let mut builder = EngineBuilder::default();
    register(&mut builder, sink.clone());
    let engine = builder.build().map_err(|e| e.to_string())?;
    let before = call_detail(&engine, &old_id, &old_id, old_no)?;
    assert_eq!(
        before.body["folderFlags"]["unreadInside"]["state"],
        "absent"
    );
    // A newer result replaces it; its own detail shows its own unread place, and the old
    // reference no longer answers with the old value.
    let (new_id, new) = folder_result(&sink, 30, true)?;
    let after = call_detail(&engine, &new_id, &new_id, folder(&new)?)?;
    assert_eq!(
        after.body["folderFlags"]["unreadInside"]["state"],
        "present"
    );
    let stale = call_detail(&engine, &old_id, &old_id, old_no)?;
    assert!(stale.error.is_some());
    assert!(stale.body.get("folderFlags").is_none_or(Value::is_null));
    Ok(())
}
