use super::*;
use du_analysis_index::IngestSink;
use du_core_contracts::{
    ErrorCode, PathBytes, PathEncoding,
    ports::{PortError, PortErrorKind, Random},
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot},
    scan_failures::{ScanFailureKind, ScanFailureStage, ScanIssue},
};
mod admission;
mod filters;

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
    fixture_at("C:\\Fixture")
}

fn fixture_at(root: &str) -> Result<(IngestSink, Id), String> {
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
                    raw: PathBytes::from_bytes(&wide(root)),
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

fn assert_error(response: ResultEnvelope, code: ErrorCode, reason: &str) -> Result<(), String> {
    let error = response.error.ok_or("request accepted")?;
    assert_eq!(error.code, code);
    assert_eq!(error.reason_key, reason);
    Ok(())
}

fn replace(store: &IngestSink, version: Id) -> Result<(), String> {
    let mut replacement = store.open(id(4)?).map_err(|e| format!("{e:?}"))?;
    replacement
        .add_directory(Vec::new())
        .map_err(|e| format!("{e:?}"))?;
    replacement
        .seal(version, ResultCompleteness::Complete)
        .map(|_| ())
        .map_err(|e| format!("{e:?}"))
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
fn replaced_result_returns_stale_version_and_requery_reason() -> Result<(), String> {
    let (store, version) = fixture()?;
    let other = id(3)?;
    replace(&store, other)?;
    let engine = engine(store, true)?;
    assert_error(
        page(&engine, json!({"resultVersion":version}))?,
        ErrorCode::StaleVersion,
        "result.requery_required",
    )
}

#[test]
fn continuation_cursor_from_another_result_returns_cursor_stale() -> Result<(), String> {
    let (store, version) = fixture()?;
    let engine = engine(store.clone(), true)?;
    let first = page(&engine, json!({"resultVersion":version,"count":1}))?;
    let cursor = first.body["nextCursor"].as_str().ok_or("missing cursor")?;
    let other = id(3)?;
    replace(&store, other.clone())?;
    assert_error(
        page(&engine, json!({"resultVersion":other,"cursor":cursor}))?,
        ErrorCode::StaleVersion,
        "cursor.stale",
    )
}

#[test]
fn released_result_returns_not_found_and_unavailable_reason() -> Result<(), String> {
    let engine = engine(IngestSink::default(), true)?;
    assert_error(
        page(&engine, json!({"resultVersion":id(2)?}))?,
        ErrorCode::NotFound,
        "ref.unavailable",
    )
}

#[test]
fn malformed_forged_and_misbound_cursors_remain_invalid_arguments() -> Result<(), String> {
    let (store, version) = fixture()?;
    let engine = engine(store, true)?;
    assert_error(
        page(&engine, json!({"resultVersion":version,"cursor":"bad"}))?,
        ErrorCode::InvalidArgument,
        "cursor.invalid",
    )?;
    let first = page(&engine, json!({"resultVersion":version,"count":1}))?;
    let cursor = first.body["nextCursor"].as_str().ok_or("missing cursor")?;
    assert_error(
        page(
            &engine,
            json!({"resultVersion":version,"cursor":cursor,"kind":"unread_range"}),
        )?,
        ErrorCode::InvalidArgument,
        "cursor.invalid",
    )?;
    let mut forged = cursor.to_owned();
    let signature = forged.rfind('.').ok_or("missing signature")? + 1;
    let replacement = if forged.as_bytes()[signature] == b'A' {
        'B'
    } else {
        'A'
    };
    forged.replace_range(signature..signature + 1, &replacement.to_string());
    assert_error(
        page(&engine, json!({"resultVersion":version,"cursor":forged}))?,
        ErrorCode::InvalidArgument,
        "cursor.invalid",
    )?;
    assert_error(
        page(&engine, json!({"resultVersion":version,"count":1001}))?,
        ErrorCode::InvalidArgument,
        "cursor.limit_exceeded",
    )
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

#[test]
fn extended_root_failures_show_the_drive_form_and_keep_raw_references() -> Result<(), String> {
    let (store, version) = fixture_at(r"\\?\C:\Fixture")?;
    let engine = engine(store, true)?;
    let page = page(
        &engine,
        json!({"resultVersion":version, "kind":"unread_range"}),
    )?;
    assert!(page.error.is_none());
    let row = &page.body["rows"][0];
    assert_eq!(row["displayPath"], r"C:\Fixture\denied");
    assert_eq!(row["pathRef"]["rootIndex"], 0);
    let denied: Vec<u8> = "denied".encode_utf16().flat_map(u16::to_le_bytes).collect();
    assert_eq!(
        row["pathRef"]["relative"],
        json!([PathBytes::from_bytes(&denied)])
    );
    Ok(())
}

#[test]
fn failures_keep_the_prefix_for_trailing_dots_and_long_paths() -> Result<(), String> {
    // The long root is 255 units without the prefix; its failure reaches 262.
    let long = format!(r"\\?\C:\{}", "g".repeat(252));
    for root in [r"\\?\C:\trailing.", long.as_str()] {
        let (store, version) = fixture_at(root)?;
        let engine = engine(store, true)?;
        let page = page(
            &engine,
            json!({"resultVersion":version, "kind":"unread_range"}),
        )?;
        assert!(page.error.is_none());
        assert_eq!(
            page.body["rows"][0]["displayPath"],
            format!(r"{root}\denied")
        );
    }
    Ok(())
}
