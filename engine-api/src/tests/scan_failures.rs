use super::*;
use du_analysis_index::IngestSink;
use du_core_contracts::{
    PathBytes, PathEncoding,
    ports::{PortError, PortErrorKind, Random},
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot},
    scan_failures::{ScanFailureKind, ScanFailureStage, ScanIssue},
};

struct TestRandom(bool);

impl Random for TestRandom {
    fn fill(&self, bytes: &mut [u8]) -> Result<(), PortError> {
        if !self.0 {
            return Err(PortError {
                kind: PortErrorKind::Io,
                reason_key: "os.random_unavailable".into(),
                os_code: None,
            });
        }
        bytes.fill(7);
        Ok(())
    }
}

fn id(value: u8) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{value:012x}"))
}

fn fixture() -> Result<(IngestSink, Id), String> {
    use ScanFailureKind::{CollectionFailed, UnreadRange, UntraversedLink};
    use ScanFailureStage::{Metadata, OpenDirectory, Traverse};
    let wide = |path: &str| {
        path.encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>()
    };
    let store = IngestSink::default();
    let mut build = store
        .open_observed(
            id(1)?,
            ResultScopeRoot {
                path: ResultNativePath {
                    encoding: PathEncoding::WindowsUtf16le,
                    raw: PathBytes::from_bytes(&wide("C:\\Fixture")),
                },
                volume_id: vec![1],
                file_id: vec![2],
            },
        )
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_directory(Vec::new())
        .map_err(|e| format!("{e:?}"))?;
    for (name, kind, stage) in [
        ("junction", UntraversedLink, Traverse),
        ("denied", UnreadRange, OpenDirectory),
        ("gone", CollectionFailed, Metadata),
    ] {
        build
            .record_scan_issue(ScanIssue {
                root_index: 0,
                relative: vec![wide(name)],
                kind,
                stage,
                reason_key: "scan.unclassified".into(),
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

fn engine(store: IngestSink, secure: bool) -> Result<Engine, String> {
    let mut builder = EngineBuilder::default();
    crate::scan_failures::register(&mut builder, store, &TestRandom(secure));
    builder.build().map_err(|e| format!("{e:?}"))
}

fn page(engine: &Engine, body: Value) -> Result<ResultEnvelope, String> {
    call(
        engine,
        "scan.failures",
        Channel::Gui,
        ENVELOPE_VERSION,
        body,
    )
}

#[test]
fn pages_pinned_sorted_rows_and_filters_through_the_gui_entry() -> Result<(), String> {
    let (store, version) = fixture()?;
    let engine = engine(store, true)?;
    let first = page(&engine, json!({"resultVersion":version, "count":1}))?;
    assert!(first.error.is_none());
    assert_eq!(first.body["totalCount"], 3);
    assert_eq!(first.body["rows"][0]["displayPath"], "C:\\Fixture\\denied");
    let next = first.body.get("nextCursor").ok_or("missing cursor")?;
    let rest = page(&engine, json!({"resultVersion":version,"cursor":next}))?;
    assert!(rest.error.is_none());
    assert_eq!(rest.body["rows"][0]["kind"], "collection_failed");
    assert_eq!(rest.body["rows"][1]["kind"], "untraversed_link");
    assert!(rest.body.get("nextCursor").is_none());
    let filtered = page(
        &engine,
        json!({"resultVersion":version, "kind":"unread_range"}),
    )?;
    assert_eq!(filtered.body["totalCount"], 1);
    assert_eq!(filtered.body["rows"][0]["stage"], "open_directory");
    Ok(())
}

#[test]
fn rejects_bad_tokens_wrong_versions_and_released_sources() -> Result<(), String> {
    let (store, version) = fixture()?;
    let engine = engine(store.clone(), true)?;
    let first = page(&engine, json!({"resultVersion":version,"count":1}))?;
    let next = first.body.get("nextCursor").ok_or("missing cursor")?;
    let other = id(3)?;
    for (body, reason) in [
        (
            json!({"resultVersion":version,"count":1001}),
            "cursor.limit_exceeded",
        ),
        (
            json!({"resultVersion":version,"cursor":"bad"}),
            "cursor.invalid",
        ),
        (
            json!({"resultVersion":version,"cursor":next,"kind":"unread_range"}),
            "cursor.invalid",
        ),
        (
            json!({"resultVersion":other,"cursor":next}),
            "result.requery_required",
        ),
    ] {
        assert_eq!(
            page(&engine, body)?
                .error
                .ok_or("request accepted")?
                .reason_key,
            reason
        );
    }
    let mut replacement = store.open(id(4)?).map_err(|e| format!("{e:?}"))?;
    replacement
        .add_directory(Vec::new())
        .map_err(|e| format!("{e:?}"))?;
    replacement
        .seal(other, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        page(&engine, json!({"resultVersion":version,"cursor":next}))?
            .error
            .ok_or("released source accepted")?
            .reason_key,
        "ref.unavailable"
    );
    Ok(())
}

#[test]
fn does_not_register_without_secure_randomness_or_expose_rows_outside_gui() -> Result<(), String> {
    let (store, version) = fixture()?;
    let body = json!({"resultVersion":version});
    assert_eq!(
        page(&engine(store.clone(), false)?, body.clone())?
            .error
            .ok_or("missing entropy accepted")?
            .reason_key,
        "command.not_provided"
    );
    let gui_engine = engine(store, true)?;
    let response = call(
        &gui_engine,
        "scan.failures",
        Channel::Cli,
        ENVELOPE_VERSION,
        body,
    )?;
    assert!(response.error.is_some());
    assert!(response.body.is_null());
    Ok(())
}
