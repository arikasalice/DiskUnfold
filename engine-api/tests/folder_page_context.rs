use du_analysis_index::{
    IngestSink,
    query::{self, page::Total},
};
use du_core_contracts::{
    Id, ItemRef, PathBytes, PathEncoding,
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot, ResultVersionRead},
};
use du_engine_api::browse::folder::{FolderBrowse, OpenRequest, Sort, View, cursor::Session};
use du_job_runtime::JobIdentity;
use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
};

struct Ids(AtomicU8);
impl JobIdentity for Ids {
    fn next_id(&self) -> Id {
        let n = self.0.fetch_add(1, Ordering::SeqCst);
        Id::from_v7_bytes([1, 2, 3, 4, 5, 6, 0x70, n, 0x80, 0, 0, 0, 0, 0, 0, n])
    }
}
fn id(n: u8) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}
fn request(location: Option<ItemRef>) -> OpenRequest {
    OpenRequest {
        result_version: None,
        location,
        view: View::List,
        condition: query::Condition::All(vec![]),
        sort: Sort {
            id: "sort.path".into(),
            revision: 1,
            basis: query::Basis::Logical,
            descending: false,
        },
        page_size: Some(1),
    }
}
fn open(
    api: &FolderBrowse,
    req: OpenRequest,
) -> Result<du_engine_api::browse::folder::Opened, String> {
    api.open(req, || false).map_err(|e| format!("{e:?}"))
}

#[test]
fn forest_top_pages_only_real_roots_and_opens_each_root_independently() -> Result<(), String> {
    let sink = IngestSink::default();
    let roots = [(b"/z".as_slice(), 1), (b"/a", 2)].map(|(path, identity)| ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(path),
        },
        volume_id: vec![identity],
        file_id: vec![identity],
    });
    let mut build = sink
        .open_forest(id(1)?, roots.to_vec())
        .map_err(|e| format!("{e:?}"))?;
    for (index, name) in [(0, b"z-child"), (1, b"a-child")] {
        build
            .add_directory_at(index, vec![])
            .map_err(|e| format!("{e:?}"))?;
        build
            .add_directory_at(index, vec![name.to_vec()])
            .map_err(|e| format!("{e:?}"))?;
    }
    build
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let api = Arc::new(FolderBrowse::new(sink, Arc::new(Ids(AtomicU8::new(1)))));
    let session = Session::new(Arc::clone(&api), [7; 32]);
    let first = session
        .open(request(None), &[], || false)
        .map_err(|e| format!("{e:?}"))?;
    let top = first.opened;
    assert!(top.location.is_none());
    assert!(top.breadcrumbs.is_empty());
    assert_eq!(top.page.total, Total::Exact(2));
    assert_eq!(top.page.rows[0].display_name.as_deref(), Some("/a"));
    assert_eq!(top.page.rows[0].item.item_no, 1);
    check_virtual_context(&session, &top)?;
    let cursor = first.next_cursor.ok_or("missing forest cursor")?;
    check_continuation_context(&session, &cursor)?;
    let next = session
        .page(&cursor, Some(1))
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(next.page.rows[0].display_name.as_deref(), Some("/z"));
    assert_eq!(next.page.rows[0].item.item_no, 0);
    assert!(next.next_cursor.is_none());

    let a = open(&api, request(Some(top.page.rows[0].item.clone())))?;
    check_root_context(&session, &a)?;
    assert_eq!(a.location.as_ref(), Some(&top.page.rows[0].item));
    assert_eq!(a.breadcrumbs[0].display_name.as_deref(), Some("/a"));
    assert_eq!(a.page.rows[0].display_name.as_deref(), Some("a-child"));
    assert_ne!(a.page.query_ref, top.page.query_ref);
    let z = open(&api, request(Some(next.page.rows[0].item.clone())))?;
    assert_eq!(z.page.rows[0].display_name.as_deref(), Some("z-child"));
    assert_ne!(z.page.query_ref, a.page.query_ref);
    assert_eq!(
        open(&api, request(None))?.page.query_ref,
        top.page.query_ref
    );
    Ok(())
}

fn check_virtual_context(
    session: &Session,
    top: &du_engine_api::browse::folder::Opened,
) -> Result<(), String> {
    let context_page = session
        .page_at(&top.page.query_ref, &top.page.result_version, 0, Some(1))
        .map_err(|e| format!("{e:?}"))?
        .into_response()
        .map_err(|e| format!("{e:?}"))?;
    let body = serde_json::to_value(context_page).map_err(|e| e.to_string())?;
    assert_eq!(body["browseContext"]["rootKind"], "virtual_root");
    assert_eq!(body["browseContext"]["location"], serde_json::Value::Null);
    assert_eq!(body["browseContext"]["breadcrumbs"], serde_json::json!([]));
    let later = session
        .page_at(&top.page.query_ref, &top.page.result_version, 1, Some(1))
        .map_err(|e| format!("{e:?}"))?
        .into_response()
        .map_err(|e| format!("{e:?}"))?;
    assert!(
        serde_json::to_value(later)
            .map_err(|e| e.to_string())?
            .get("browseContext")
            .is_none()
    );
    assert!(
        session
            .page_at(&top.page.query_ref, &id(99)?, 0, Some(1))
            .is_err()
    );
    Ok(())
}

fn check_continuation_context(session: &Session, cursor: &str) -> Result<(), String> {
    let continued = session
        .page(cursor, Some(1))
        .map_err(|e| format!("{e:?}"))?
        .into_response()
        .map_err(|e| format!("{e:?}"))?;
    assert!(
        serde_json::to_value(continued)
            .map_err(|e| e.to_string())?
            .get("browseContext")
            .is_none()
    );
    Ok(())
}

fn check_root_context(
    session: &Session,
    a: &du_engine_api::browse::folder::Opened,
) -> Result<(), String> {
    let root_context = session
        .page_at(&a.page.query_ref, &a.page.result_version, 0, Some(1))
        .map_err(|e| format!("{e:?}"))?
        .into_response()
        .map_err(|e| format!("{e:?}"))?;
    let root_body = serde_json::to_value(root_context).map_err(|e| e.to_string())?;
    assert_eq!(root_body["browseContext"]["rootKind"], "scope_root");
    assert_eq!(root_body["browseContext"]["location"]["itemNo"], 1);
    assert_eq!(
        root_body["browseContext"]["breadcrumbs"][0]["displayName"]["value"],
        "/a"
    );
    Ok(())
}

#[test]
fn direct_first_page_refuses_a_replaced_source() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = sink.open(id(1)?).map_err(|e| format!("{e:?}"))?;
    build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    build
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let api = Arc::new(FolderBrowse::new(
        sink.clone(),
        Arc::new(Ids(AtomicU8::new(1))),
    ));
    let session = Session::new(api, [7; 32]);
    let opened = session
        .open(request(None), &[], || false)
        .map_err(|e| format!("{e:?}"))?;
    let mut replacement = sink.open(id(1)?).map_err(|e| format!("{e:?}"))?;
    replacement
        .add_directory(vec![])
        .map_err(|e| format!("{e:?}"))?;
    replacement
        .seal(id(3)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    assert!(matches!(
        session.page_at(&opened.opened.page.query_ref, &id(2)?, 0, Some(1)),
        Err(du_engine_api::browse::folder::cursor::CursorError::Stale)
    ));
    Ok(())
}

#[test]
fn evicted_query_cannot_return_rows_or_context() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = sink.open(id(1)?).map_err(|e| format!("{e:?}"))?;
    build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    for index in 0..17 {
        build
            .add_directory(vec![format!("dir-{index:02}").into_bytes()])
            .map_err(|e| format!("{e:?}"))?;
    }
    let version_id = id(2)?;
    let version = build
        .seal(version_id.clone(), ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let refs: Vec<_> = version
        .items()
        .iter()
        .skip(1)
        .map(|item| ItemRef {
            result_version: version_id.clone(),
            item_no: item.item_no,
        })
        .collect();
    let api = Arc::new(FolderBrowse::new(sink, Arc::new(Ids(AtomicU8::new(1)))));
    let session = Session::new(api, [7; 32]);
    let mut first = None;
    for reference in refs {
        let opened = session
            .open(request(Some(reference)), &[], || false)
            .map_err(|e| format!("{e:?}"))?;
        if first.is_none() {
            first = Some(opened.opened.page.query_ref);
        }
    }
    assert!(matches!(
        session.page_at(&first.ok_or("missing query")?, &id(2)?, 0, Some(1)),
        Err(du_engine_api::browse::folder::cursor::CursorError::Stale)
    ));
    Ok(())
}
