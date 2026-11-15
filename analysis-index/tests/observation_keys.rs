//! Observation keys of one build: order, admission failures and growth between publications.
use du_analysis_index::{BuildError, BuildHandle, IngestSink, Observation, ResultVersion};
use du_core_contracts::Id;
use du_core_contracts::result::{ResultCompleteness, ResultItemKind, ResultVersionRead};
use std::collections::BTreeMap;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn id(n: u8) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}
fn err(error: BuildError) -> String {
    format!("{error:?}")
}
fn directory(build: &mut BuildHandle, path: &[&[u8]]) -> Result<(), BuildError> {
    build.add_directory(path.iter().map(|name| name.to_vec()).collect())
}
fn file(build: &mut BuildHandle, path: &[&[u8]]) -> Result<(), BuildError> {
    build.add_file(Observation {
        path: path.iter().map(|name| name.to_vec()).collect(),
        size: None,
    })
}

/// Every item's raw path from its root, with its kind.
fn keys(version: &ResultVersion) -> BTreeMap<Vec<Vec<u8>>, ResultItemKind> {
    let items = version.items();
    let mut paths: Vec<Vec<Vec<u8>>> = Vec::with_capacity(items.len());
    for item in items {
        // A parent precedes its children, so its path is already known.
        let mut path = item
            .parent
            .map(|parent| paths[parent as usize].clone())
            .unwrap_or_default();
        if item.parent.is_some() {
            path.push(item.name.clone());
        }
        paths.push(path);
    }
    paths
        .into_iter()
        .zip(items.iter().map(|item| item.kind))
        .collect()
}

#[test]
fn siblings_are_numbered_in_raw_name_byte_order() -> TestResult {
    let sink = IngestSink::default();
    let mut build = sink.open(id(1)?).map_err(err)?;
    let names: [&[u8]; 9] = [
        b"b",
        b"\xff",
        b"ab",
        b"a",
        b"B",
        b"a\x00",
        b"\xc3\xa9",
        b"\x80",
        b"a ",
    ];
    for name in names {
        file(&mut build, &[name]).map_err(err)?;
    }
    directory(&mut build, &[]).map_err(err)?;
    let version = build
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(err)?;
    let stored: Vec<&[u8]> = version.items()[1..]
        .iter()
        .map(|item| item.name.as_slice())
        .collect();
    let expected: [&[u8]; 9] = [
        b"B",
        b"a",
        b"a\x00",
        b"a ",
        b"ab",
        b"b",
        b"\x80",
        b"\xc3\xa9",
        b"\xff",
    ];
    assert_eq!(stored, expected);
    Ok(())
}

#[test]
fn recording_order_does_not_change_the_published_items() -> TestResult {
    let paths: [&[&[u8]]; 9] = [
        &[],
        &[b"a"],
        &[b"a", b"deep"],
        &[b"a", b"deep", b"er"],
        &[b"a", b"deep", b"er", b"file"],
        &[b"a", b"file"],
        &[b"b"],
        &[b"b", b"file"],
        &[b"file"],
    ];
    let record =
        |order: &[usize], scope: u8, version: u8| -> Result<_, Box<dyn std::error::Error>> {
            let sink = IngestSink::default();
            let mut build = sink.open(id(scope)?).map_err(err)?;
            for index in order {
                let path = paths[*index];
                if path.last() == Some(&&b"file"[..]) {
                    file(&mut build, path).map_err(err)?;
                } else {
                    directory(&mut build, path).map_err(err)?;
                }
            }
            Ok(build
                .seal(id(version)?, ResultCompleteness::Complete)
                .map_err(err)?)
        };
    let parents_first = record(&[0, 1, 2, 3, 4, 5, 6, 7, 8], 1, 2)?;
    // Children before their parents, and the root last.
    let children_first = record(&[4, 8, 3, 7, 5, 2, 6, 1, 0], 1, 2)?;
    assert_eq!(parents_first.items(), children_first.items());
    assert_eq!(
        parents_first
            .items()
            .iter()
            .map(|item| (item.parent, item.name.as_slice()))
            .collect::<Vec<_>>(),
        vec![
            (None, &b""[..]),
            (Some(0), b"a"),
            (Some(0), b"b"),
            (Some(0), b"file"),
            (Some(1), b"deep"),
            (Some(1), b"file"),
            (Some(2), b"file"),
            (Some(4), b"er"),
            (Some(7), b"file"),
        ]
    );
    Ok(())
}

#[test]
fn an_item_without_an_observed_ancestor_fails_publication_and_withdraws_the_build() -> TestResult {
    let sink = IngestSink::default();
    let mut first = sink.open(id(1)?).map_err(err)?;
    directory(&mut first, &[]).map_err(err)?;
    let baseline = first
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(err)?;
    let mut build = sink.open(id(1)?).map_err(err)?;
    directory(&mut build, &[]).map_err(err)?;
    directory(&mut build, &[b"a"]).map_err(err)?;
    let partial = build.publish_partial(id(3)?).map_err(err)?;
    assert_eq!(partial.items().len(), 2);
    // The path names "missing", which was never recorded itself.
    file(&mut build, &[b"a", b"missing", b"file"]).map_err(err)?;
    assert_eq!(
        build.publish_partial(id(4)?).err(),
        Some(BuildError::InvalidTree)
    );
    let current = sink.pin().map_err(err)?.ok_or("missing current")?;
    assert_eq!(current.version_id(), baseline.version_id());
    assert_eq!(
        directory(&mut build, &[b"a", b"missing"]),
        Err(BuildError::Unavailable)
    );
    // A missing root is the same failure.
    let mut rootless = sink.open(id(1)?).map_err(err)?;
    file(&mut rootless, &[b"file"]).map_err(err)?;
    assert_eq!(
        rootless.seal(id(5)?, ResultCompleteness::Complete).err(),
        Some(BuildError::InvalidTree)
    );
    Ok(())
}

#[test]
fn a_repeated_path_is_rejected_and_aborts_the_build() -> TestResult {
    let sink = IngestSink::default();
    let mut build = sink.open(id(1)?).map_err(err)?;
    directory(&mut build, &[]).map_err(err)?;
    directory(&mut build, &[b"a"]).map_err(err)?;
    file(&mut build, &[b"a", b"file"]).map_err(err)?;
    build.publish_partial(id(2)?).map_err(err)?;
    // The kind of the second observation does not matter.
    assert_eq!(
        directory(&mut build, &[b"a", b"file"]),
        Err(BuildError::InvalidTree)
    );
    assert!(sink.pin().map_err(err)?.is_none());
    assert_eq!(file(&mut build, &[b"other"]), Err(BuildError::Unavailable));
    assert_eq!(
        build.publish_partial(id(3)?).err(),
        Some(BuildError::Unavailable)
    );
    Ok(())
}

#[test]
fn an_empty_path_component_is_rejected_and_aborts_the_build() -> TestResult {
    let sink = IngestSink::default();
    let mut build = sink.open(id(1)?).map_err(err)?;
    directory(&mut build, &[]).map_err(err)?;
    assert_eq!(
        file(&mut build, &[b"a", b"", b"file"]),
        Err(BuildError::InvalidTree)
    );
    assert_eq!(directory(&mut build, &[b"a"]), Err(BuildError::Unavailable));
    Ok(())
}

#[test]
fn later_publications_of_one_build_keep_every_earlier_key() -> TestResult {
    let sink = IngestSink::default();
    let mut build = sink.open(id(1)?).map_err(err)?;
    directory(&mut build, &[]).map_err(err)?;
    directory(&mut build, &[b"m"]).map_err(err)?;
    file(&mut build, &[b"m", b"one"]).map_err(err)?;
    let first = build.publish_partial(id(2)?).map_err(err)?;
    // New names sort before, between and after the published ones.
    directory(&mut build, &[b"a"]).map_err(err)?;
    file(&mut build, &[b"a", b"zero"]).map_err(err)?;
    file(&mut build, &[b"m", b"a"]).map_err(err)?;
    file(&mut build, &[b"z"]).map_err(err)?;
    let second = build.publish_partial(id(3)?).map_err(err)?;
    directory(&mut build, &[b"m", b"sub"]).map_err(err)?;
    file(&mut build, &[b"m", b"sub", b"two"]).map_err(err)?;
    let sealed = build
        .seal(id(4)?, ResultCompleteness::Complete)
        .map_err(err)?;
    let (first, second, sealed) = (keys(&first), keys(&second), keys(&sealed));
    assert_eq!((first.len(), second.len(), sealed.len()), (3, 7, 9));
    for (earlier, later) in [(&first, &second), (&second, &sealed)] {
        for (path, kind) in earlier {
            assert_eq!(later.get(path), Some(kind));
        }
    }
    Ok(())
}

#[test]
fn a_chain_of_four_thousand_directories_is_published_whole() -> TestResult {
    const DEPTH: usize = 4_000;
    let sink = IngestSink::default();
    let mut build = sink.open(id(1)?).map_err(err)?;
    let mut path: Vec<Vec<u8>> = Vec::new();
    build.add_directory(path.clone()).map_err(err)?;
    for level in 0..DEPTH {
        for name in ["a", "b"] {
            let mut leaf = path.clone();
            leaf.push(name.as_bytes().to_vec());
            build
                .add_file(Observation {
                    path: leaf,
                    size: None,
                })
                .map_err(err)?;
        }
        path.push(format!("d{level}").into_bytes());
        build.add_directory(path.clone()).map_err(err)?;
    }
    let partial = build.publish_partial(id(2)?).map_err(err)?;
    let sealed = build
        .seal(id(3)?, ResultCompleteness::Complete)
        .map_err(err)?;
    assert_eq!(partial.items(), sealed.items());
    let items = sealed.items();
    assert_eq!(items.len(), 1 + DEPTH * 3);
    let deepest = items.last().ok_or("no items")?;
    assert_eq!(deepest.name, format!("d{}", DEPTH - 1).into_bytes());
    let mut depth = 0;
    let mut current = deepest;
    while let Some(parent) = current.parent {
        current = &items[parent as usize];
        depth += 1;
    }
    assert_eq!(depth, DEPTH);
    Ok(())
}
