use du_analysis_index::{IngestSink, Observation, QueryError, ResultVersion, browse};
use du_core_contracts::{
    Id, ItemRef,
    result::{ResultCompleteness, ResultVersionRead},
};
use std::sync::Arc;

fn id(n: u8) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}
fn item(version: &ResultVersion, name: &[u8]) -> Result<ItemRef, String> {
    let item = version
        .items()
        .iter()
        .find(|item| item.name == name)
        .ok_or("missing fixture item")?;
    Ok(ItemRef {
        result_version: version.version_id().clone(),
        item_no: item.item_no,
    })
}
fn fixture() -> Result<(IngestSink, Arc<ResultVersion>), String> {
    let sink = IngestSink::default();
    let mut build = sink.open(id(1)?).map_err(|e| format!("{e:?}"))?;
    build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    build
        .add_directory(vec![b"a".to_vec()])
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_directory(vec![b"a".to_vec(), b"empty".to_vec()])
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_file(Observation {
            path: vec![b"a".to_vec(), b"file".to_vec()],
            size: None,
        })
        .map_err(|e| format!("{e:?}"))?;
    build
        .add(Observation {
            path: vec![b"unknown".to_vec()],
            size: None,
        })
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_structure(vec![b"ambiguous".to_vec()])
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_untraversed_link(vec![b"link".to_vec()])
        .map_err(|e| format!("{e:?}"))?;
    let version = build
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    Ok((sink, version))
}
fn names(items: &[du_core_contracts::result::ResultItem]) -> Vec<&[u8]> {
    items.iter().map(|item| item.name.as_slice()).collect()
}

#[test]
fn reads_only_immediate_children_and_root_to_location_breadcrumbs() -> Result<(), String> {
    let (_, version) = fixture()?;
    let root = browse::open(&version, None).map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        names(root.children),
        vec![
            b"a".as_slice(),
            b"ambiguous".as_slice(),
            b"link".as_slice(),
            b"unknown".as_slice()
        ]
    );
    assert_eq!(root.breadcrumbs.len(), 1);
    let dir = item(&version, b"a")?;
    let folder = browse::open(&version, Some(&dir)).map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        names(folder.children),
        vec![b"empty".as_slice(), b"file".as_slice()]
    );
    assert_eq!(
        folder
            .breadcrumbs
            .iter()
            .map(|x| x.name.as_slice())
            .collect::<Vec<_>>(),
        vec![b"".as_slice(), b"a".as_slice()]
    );
    let empty = item(&version, b"empty")?;
    let view = browse::open(&version, Some(&empty)).map_err(|e| format!("{e:?}"))?;
    assert!(view.children.is_empty());
    assert_eq!(view.breadcrumbs.len(), 3);
    Ok(())
}

#[test]
fn distinguishes_wrong_version_missing_file_and_unknown_kind() -> Result<(), String> {
    let (_, version) = fixture()?;
    let bad = ItemRef {
        result_version: id(3)?,
        item_no: 0,
    };
    assert!(matches!(
        browse::open(&version, Some(&bad)),
        Err(QueryError::StaleVersion)
    ));
    let missing = ItemRef {
        result_version: id(2)?,
        item_no: u32::MAX,
    };
    assert!(matches!(
        browse::open(&version, Some(&missing)),
        Err(QueryError::NotFound)
    ));
    assert!(matches!(
        browse::open(&version, Some(&item(&version, b"file")?)),
        Err(QueryError::NotDirectory)
    ));
    assert!(matches!(
        browse::open(&version, Some(&item(&version, b"unknown")?)),
        Err(QueryError::KindUnknown)
    ));
    assert!(matches!(
        browse::open(&version, Some(&item(&version, b"ambiguous")?)),
        Err(QueryError::KindUnknown)
    ));
    assert!(matches!(
        browse::open(&version, Some(&item(&version, b"link")?)),
        Err(QueryError::NotDirectory)
    ));
    assert!(matches!(
        IngestSink::default().pin_current(&id(2)?),
        Err(QueryError::NotFound)
    ));
    Ok(())
}

#[test]
fn pinned_old_reader_remains_consistent_after_replacement_and_withdrawal() -> Result<(), String> {
    let (sink, old) = fixture()?;
    let old_dir = item(&old, b"a")?;
    let mut build = sink.open(id(1)?).map_err(|e| format!("{e:?}"))?;
    build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    build
        .add_directory(vec![b"new".to_vec()])
        .map_err(|e| format!("{e:?}"))?;
    let newer = build
        .publish_partial(id(3)?)
        .map_err(|e| format!("{e:?}"))?;
    assert!(matches!(
        sink.pin_current(&id(2)?),
        Err(QueryError::StaleVersion)
    ));
    assert!(matches!(
        browse::open(&newer, Some(&old_dir)),
        Err(QueryError::StaleVersion)
    ));
    assert_eq!(
        names(
            browse::open(&old, Some(&old_dir))
                .map_err(|e| format!("{e:?}"))?
                .children
        ),
        vec![b"empty".as_slice(), b"file".as_slice()]
    );
    build.abort();
    assert!(matches!(
        sink.pin_current(&id(3)?),
        Err(QueryError::StaleVersion)
    ));
    assert_eq!(
        names(
            browse::open(&newer, None)
                .map_err(|e| format!("{e:?}"))?
                .children
        ),
        vec![b"new".as_slice()]
    );
    assert_eq!(
        names(
            browse::open(&old, Some(&old_dir))
                .map_err(|e| format!("{e:?}"))?
                .children
        )
        .len(),
        2
    );
    Ok(())
}
