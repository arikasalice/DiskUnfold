use super::*;
use du_analysis_index::{IngestSink, ScopeHeader};
use du_core_contracts::{
    Id, OsPath, PathBytes, PathEncoding,
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot, ResultVersionRead},
    scan_failures::{ScanFailureKind, ScanFailureStage, ScanIssue, unentered_reason},
    volume::{ConfirmedFirmlink, RootVolumeGroup},
};

fn id(n: u32) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}")).map_err(|error| format!("{error:?}"))
}

fn add_folder(build: &mut du_analysis_index::BuildHandle, path: &[&str]) -> Result<(), String> {
    build
        .add_directory_at(
            0,
            path.iter().map(|part| part.as_bytes().to_vec()).collect(),
        )
        .map_err(|error| format!("{error:?}"))?;
    Ok(())
}

fn item_at(version: &ResultVersion, path: &[&str]) -> Result<ItemRef, String> {
    version
        .items()
        .iter()
        .find(|item| {
            let mut parts = Vec::new();
            let mut current = *item;
            while let Some(parent) = current.parent {
                parts.push(current.name.as_slice());
                current = &version.items()[parent as usize];
            }
            parts.reverse();
            parts == path.iter().map(|part| part.as_bytes()).collect::<Vec<_>>()
        })
        .map(|item| ItemRef {
            result_version: version.version_id().clone(),
            item_no: item.item_no,
        })
        .ok_or_else(|| "folder exists".to_owned())
}

#[test]
fn counted_at_is_only_set_for_the_matching_already_counted_firmlink_target() -> Result<(), String> {
    let group = RootVolumeGroup {
        data_mount: b"/System/Volumes/Data".to_vec(),
        device: 1,
        data_inode: 20,
        firmlinks: vec![ConfirmedFirmlink {
            source: b"/Users".to_vec(),
            target: b"/System/Volumes/Data/Users".to_vec(),
            device: 1,
            inode: 99,
        }],
    };
    let sink = IngestSink::default();
    let mut build = sink
        .open_scoped_forest(
            ScopeHeader {
                version_id: id(1)?,
                fingerprint: "group".into(),
                volume_group: Some(group),
            },
            vec![ResultScopeRoot {
                path: ResultNativePath {
                    encoding: PathEncoding::Unix,
                    raw: PathBytes::from_bytes(b"/"),
                },
                volume_id: b"volume".to_vec(),
                file_id: b"root".to_vec(),
            }],
            None,
        )
        .map_err(|error| format!("{error:?}"))?;
    add_folder(&mut build, &[])?;
    add_folder(&mut build, &["System"])?;
    add_folder(&mut build, &["System", "Volumes"])?;
    add_folder(&mut build, &["System", "Volumes", "Data"])?;
    add_folder(&mut build, &["System", "Volumes", "Data", "Users"])?;
    add_folder(&mut build, &["System", "Volumes", "Data", "Other"])?;
    for relative in [
        ["System", "Volumes", "Data", "Users"],
        ["System", "Volumes", "Data", "Other"],
    ] {
        build
            .record_scan_issue(ScanIssue {
                root_index: 0,
                relative: relative
                    .iter()
                    .map(|part| part.as_bytes().to_vec())
                    .collect(),
                kind: ScanFailureKind::UnreadRange,
                stage: ScanFailureStage::Traverse,
                reason_key: unentered_reason::REVISIT_BLOCKED.into(),
                observed_at: None,
            })
            .map_err(|error| format!("{error:?}"))?;
    }
    let version = build
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(|error| format!("{error:?}"))?;
    let page = |path: &[&str]| -> Result<_, String> {
        page_flags(&version, &[item_at(&version, path)?])
            .map_err(|error| format!("{error:?}"))?
            .into_iter()
            .next()
            .ok_or_else(|| "one row".to_owned())
    };
    assert_eq!(
        page(&["System", "Volumes", "Data", "Users"])?.counted_at,
        Some(OsPath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(b"/Users"),
            display: "/Users".into(),
            lossy: false,
        })
    );
    assert!(
        page(&["System", "Volumes", "Data", "Other"])?
            .counted_at
            .is_none()
    );
    assert!(page(&["System", "Volumes", "Data"])?.counted_at.is_none());
    Ok(())
}
