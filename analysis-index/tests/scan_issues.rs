use du_analysis_index::IngestSink;
use du_core_contracts::{
    Id, Timestamp,
    result::ResultCompleteness,
    scan_failures::{ScanFailureKind as Kind, ScanFailureStage as Stage, ScanIssue},
};

fn id(n: u8) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}

#[test]
fn publications_pin_and_sort_issue_facts_without_creating_gaps() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = sink.open(id(1)?).map_err(|e| format!("{e:?}"))?;
    build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    let make = |path: &[u8], kind, when: Option<&str>| -> Result<ScanIssue, String> {
        Ok(ScanIssue {
            root_index: 0,
            relative: vec![path.to_vec()],
            kind,
            stage: Stage::Metadata,
            reason_key: "scan.unclassified".into(),
            observed_at: when.map(Timestamp::parse).transpose()?,
        })
    };
    build
        .record_scan_issue(make(b"z", Kind::Uncertain, None)?)
        .map_err(|e| format!("{e:?}"))?;
    let old = build
        .publish_partial(id(2)?)
        .map_err(|e| format!("{e:?}"))?;
    for issue in [
        make(b"a", Kind::Uncertain, Some("2022-01-01T00:00:00.000Z"))?,
        make(b"a", Kind::CollectionFailed, None)?,
        make(b"a", Kind::Uncertain, Some("2021-01-01T00:00:00.000Z"))?,
    ] {
        build
            .record_scan_issue(issue)
            .map_err(|e| format!("{e:?}"))?;
    }
    let new = build
        .seal(id(3)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(old.scan_issues().len(), 1);
    let facts = new.scan_issues();
    assert_eq!(
        facts
            .iter()
            .map(|v| (v.relative[0].as_slice(), v.kind))
            .collect::<Vec<_>>(),
        vec![
            (b"a".as_slice(), Kind::CollectionFailed),
            (b"a".as_slice(), Kind::Uncertain),
            (b"a".as_slice(), Kind::Uncertain),
            (b"z".as_slice(), Kind::Uncertain)
        ]
    );
    assert_eq!(
        facts[1].observed_at.as_ref().map(Timestamp::as_str),
        Some("2021-01-01T00:00:00.000Z")
    );
    assert!(new.scan_issues()[0].observed_at.is_none());
    assert!(du_core_contracts::result::ResultVersionRead::scope_gaps(&*new).is_empty());
    Ok(())
}

#[test]
fn forest_issues_sort_by_root_then_raw_components() -> Result<(), String> {
    use du_core_contracts::{
        PathBytes, PathEncoding,
        result::{ResultNativePath, ResultScopeRoot},
    };
    let root = |raw: &[u8]| ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(raw),
        },
        volume_id: vec![1],
        file_id: vec![1],
    };
    let sink = IngestSink::default();
    let mut build = sink
        .open_forest(id(1)?, vec![root(b"/a"), root(b"/b")])
        .map_err(|e| format!("{e:?}"))?;
    for n in 0..2 {
        build
            .add_directory_at(n, vec![])
            .map_err(|e| format!("{e:?}"))?;
    }
    for (root_index, path) in [(1, vec![0]), (0, vec![255]), (0, vec![2])] {
        build
            .record_scan_issue(ScanIssue {
                root_index,
                relative: vec![path],
                kind: Kind::Uncertain,
                stage: Stage::Metadata,
                reason_key: "scan.unclassified".into(),
                observed_at: None,
            })
            .map_err(|e| format!("{e:?}"))?;
    }
    let result = build
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        result
            .scan_issues()
            .iter()
            .map(|v| (v.root_index, v.relative[0][0]))
            .collect::<Vec<_>>(),
        [(0, 2), (0, 255), (1, 0)]
    );
    Ok(())
}
