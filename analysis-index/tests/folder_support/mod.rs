//! Shared builders for the folder collection tests.
#![allow(dead_code)]
use du_analysis_index::folder_collection::{FolderBoundary, FolderCollection, Unenumerated};
use du_analysis_index::{BuildHandle, ResultVersion};
use du_core_contracts::{
    Id, ItemRef, PathBytes, PathEncoding,
    result::{ResultNativePath, ResultScopeRoot, ResultVersionRead},
    scan_failures::{ScanFailureKind, ScanFailureStage, ScanIssue, unentered_reason},
};
use std::fmt::Debug;

pub fn id(n: u8) -> Id {
    Id::from_v7_bytes([n; 16])
}

pub fn err<T: Debug>(value: T) -> String {
    format!("{value:?}")
}

pub fn root(name: &[u8]) -> ResultScopeRoot {
    ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(name),
        },
        volume_id: vec![1],
        file_id: name.to_vec(),
    }
}

/// A root-relative path from its components.
pub fn folder_path(parts: &[&str]) -> Vec<Vec<u8>> {
    parts.iter().map(|part| part.as_bytes().to_vec()).collect()
}

/// Opens a forest of `roots` roots and registers each root directory.
pub fn forest(
    sink: &du_analysis_index::IngestSink,
    version: u8,
    roots: u8,
) -> Result<BuildHandle, String> {
    let list = (0..roots).map(|n| root(&[b'/', b'a' + n])).collect();
    let mut build = sink.open_forest(id(version), list).map_err(err)?;
    for index in 0..u32::from(roots) {
        build.add_directory_at(index, Vec::new()).map_err(err)?;
    }
    Ok(build)
}

pub fn dirs(build: &mut BuildHandle, root: u32, paths: &[&[&str]]) -> Result<(), String> {
    for parts in paths {
        build
            .add_directory_at(root, folder_path(parts))
            .map_err(err)?;
    }
    Ok(())
}

pub fn gap(build: &mut BuildHandle, root: u32, parts: &[&str], reason: &str) -> Result<(), String> {
    build
        .record_gap_at(root, folder_path(parts), reason.into(), None)
        .map_err(err)
}

/// Records the unread-range issue the collector writes for an unentered directory.
pub fn unentered(
    build: &mut BuildHandle,
    root: u32,
    parts: &[&str],
    reason: &str,
) -> Result<(), String> {
    build
        .record_scan_issue(ScanIssue {
            root_index: root,
            relative: folder_path(parts),
            kind: ScanFailureKind::UnreadRange,
            stage: ScanFailureStage::Traverse,
            reason_key: reason.into(),
            observed_at: None,
        })
        .map_err(err)
}

pub const OTHER_VOLUME: &str = unentered_reason::BOUNDARY_OTHER_VOLUME;
pub const REVISIT: &str = unentered_reason::REVISIT_BLOCKED;
pub const UNVERIFIABLE: &str = unentered_reason::BOUNDARY_UNVERIFIABLE;

/// The reference of the item at `parts` below root `root_index`.
pub fn item(version: &ResultVersion, root_index: usize, parts: &[&str]) -> Result<ItemRef, String> {
    let mut current = version
        .scope_roots()
        .get(root_index)
        .ok_or("no root")?
        .item_no;
    for part in parts {
        current = version
            .items()
            .iter()
            .find(|item| item.parent == Some(current) && item.name == part.as_bytes())
            .ok_or("missing item")?
            .item_no;
    }
    Ok(ItemRef {
        result_version: version.version_id().clone(),
        item_no: current,
    })
}

pub const CONFIRMED_NO: FolderCollection<'static> = FolderCollection::Folder {
    unenumerated: Unenumerated::No,
    boundary: FolderBoundary::NoBoundary,
};

pub fn state<'a>(
    version: &'a ResultVersion,
    root_index: usize,
    parts: &[&str],
) -> Result<FolderCollection<'a>, String> {
    version
        .folder_collection(&item(version, root_index, parts)?)
        .map_err(err)
}

pub fn unenumerated(reason_key: &str) -> FolderCollection<'_> {
    FolderCollection::Folder {
        unenumerated: Unenumerated::Yes { reason_key },
        boundary: FolderBoundary::NoBoundary,
    }
}

pub fn bounded(boundary: FolderBoundary) -> FolderCollection<'static> {
    FolderCollection::Folder {
        unenumerated: Unenumerated::No,
        boundary,
    }
}

pub fn unknown(reason_key: &'static str) -> FolderCollection<'static> {
    FolderCollection::Folder {
        unenumerated: Unenumerated::Unknown { reason_key },
        boundary: FolderBoundary::Unknown { reason_key },
    }
}
