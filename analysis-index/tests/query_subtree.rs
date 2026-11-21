//! The subtree access path of query.run: a condition over every item below a folder.
use du_analysis_index::{
    IngestSink, Observation, ResultVersion,
    query::{self, Condition, FolderLocation, Predicate, file_type_predicate, subtree},
};
use du_core_contracts::{
    Id, PathBytes, PathEncoding, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot, ResultVersionRead},
};
use std::sync::Arc;

fn error(e: impl std::fmt::Debug) -> String {
    format!("{e:?}")
}

/// clip.mp4, notes.txt, docs/report.pdf, docs/talk.mp4, songs/song.mp3 under one root.
fn version() -> Result<Arc<ResultVersion>, String> {
    let id = |n: u8| Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"));
    let root = ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(b"/fixture"),
        },
        volume_id: vec![1],
        file_id: vec![2],
    };
    let sink = IngestSink::default();
    let mut build = sink.open_forest(id(1)?, vec![root]).map_err(error)?;
    build.add_directory_at(0, vec![]).map_err(error)?;
    for folder in [b"docs".as_slice(), b"songs"] {
        build
            .add_directory_at(0, vec![folder.to_vec()])
            .map_err(error)?;
    }
    for path in [
        vec![b"clip.mp4".to_vec()],
        vec![b"notes.txt".to_vec()],
        vec![b"docs".to_vec(), b"report.pdf".to_vec()],
        vec![b"docs".to_vec(), b"talk.mp4".to_vec()],
        vec![b"songs".to_vec(), b"song.mp3".to_vec()],
    ] {
        build
            .add_file_at(
                0,
                Observation {
                    path,
                    size: Some(SizeValue {
                        basis: SizeBasis::Logical,
                        evidence: SizeEvidence::Measured,
                        amount: SizeAmount::Exact { bytes: 1.into() },
                    }),
                },
            )
            .map_err(error)?;
    }
    build
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(error)
}

fn videos() -> Condition {
    Condition::Predicate(Predicate::Registered {
        id: file_type_predicate::ID.to_owned(),
        revision: file_type_predicate::REVISION,
        operand: b"type.video".to_vec(),
    })
}

fn by_path() -> query::Sort<'static> {
    query::Sort {
        id: "sort.path",
        revision: 1,
        basis: query::Basis::Logical,
        descending: false,
    }
}

fn found(version: &Arc<ResultVersion>, location: FolderLocation) -> Result<Vec<String>, String> {
    let result = subtree::run_subtree_with_sorts(
        &query::Registry::baseline(),
        version.clone(),
        location,
        &videos(),
        by_path(),
        &[],
        || false,
    )
    .map_err(error)?;
    result
        .items
        .iter()
        .map(|item| {
            let row = version
                .items()
                .get(item.item_no as usize)
                .ok_or("no item")?;
            Ok(String::from_utf8_lossy(&row.name).into_owned())
        })
        .collect()
}

fn folder(version: &ResultVersion, name: &[u8]) -> Result<u32, String> {
    version
        .items()
        .iter()
        .find(|item| item.name == name)
        .map(|item| item.item_no)
        .ok_or_else(|| "no folder".to_owned())
}

#[test]
fn a_condition_over_the_scope_reaches_files_in_every_folder() -> Result<(), String> {
    let version = version()?;
    assert_eq!(
        found(&version, FolderLocation::ScopeRoots)?,
        ["clip.mp4", "talk.mp4"]
    );
    let root = version.scope_roots()[0].item_no;
    assert_eq!(
        found(&version, FolderLocation::Item(root))?,
        ["clip.mp4", "talk.mp4"]
    );
    Ok(())
}

#[test]
fn a_folder_location_keeps_to_its_own_subtree() -> Result<(), String> {
    let version = version()?;
    let docs = folder(&version, b"docs")?;
    assert_eq!(found(&version, FolderLocation::Item(docs))?, ["talk.mp4"]);
    let songs = folder(&version, b"songs")?;
    assert!(found(&version, FolderLocation::Item(songs))?.is_empty());
    Ok(())
}

#[test]
fn a_file_is_not_a_location() -> Result<(), String> {
    let version = version()?;
    let clip = folder(&version, b"clip.mp4")?;
    assert!(found(&version, FolderLocation::Item(clip)).is_err());
    Ok(())
}
