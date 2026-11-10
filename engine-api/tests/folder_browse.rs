use du_analysis_index::{
    IngestSink, Observation,
    query::{self, page::Total},
};
use du_core_contracts::{Id, ItemRef, result::ResultCompleteness};
use du_engine_api::browse::folder::{Error, FolderBrowse, OpenRequest, Sort, View};
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
fn fixture(partial: bool) -> Result<(IngestSink, FolderBrowse), String> {
    let sink = IngestSink::default();
    let mut build = sink.open(id(1)?).map_err(|e| format!("{e:?}"))?;
    for path in [
        vec![],
        vec![b"a".to_vec()],
        vec![b"a".to_vec(), b"empty".to_vec()],
    ] {
        build.add_directory(path).map_err(|e| format!("{e:?}"))?;
    }
    build
        .add_file(Observation {
            path: vec![b"a".to_vec(), b"file".to_vec()],
            size: None,
        })
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_structure(vec![b"ambiguous".to_vec()])
        .map_err(|e| format!("{e:?}"))?;
    let completeness = if partial {
        ResultCompleteness::Partial
    } else {
        ResultCompleteness::Complete
    };
    build
        .seal(id(2)?, completeness)
        .map_err(|e| format!("{e:?}"))?;
    let api = FolderBrowse::new(sink.clone(), Arc::new(Ids(AtomicU8::new(1))));
    Ok((sink, api))
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
fn root_child_parent_and_empty_use_the_same_pinned_folder_query() -> Result<(), String> {
    let (_, api) = fixture(false)?;
    let root = open(&api, request(None))?;
    assert_eq!(root.breadcrumbs.len(), 1);
    assert_eq!(root.page.total, Total::Exact(2));
    assert_eq!(root.page.rows[0].observation.name, b"a");
    assert_eq!(root.page.next_position, Some(1));
    let repeated = open(&api, request(None))?;
    assert_eq!(repeated.page.query_ref, root.page.query_ref);
    let second = api
        .page(&root.page.query_ref, &root.page.result_version, 1, Some(1))
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(second.rows[0].observation.name, b"ambiguous");
    assert_eq!(second.next_position, None);
    let child = open(&api, request(Some(root.page.rows[0].item.clone())))?;
    assert_eq!(child.breadcrumbs.len(), 2);
    assert_eq!(child.page.total, Total::Exact(2));
    assert_eq!(child.page.rows[0].observation.name, b"empty");
    let empty = open(&api, request(Some(child.page.rows[0].item.clone())))?;
    assert_eq!(empty.page.total, Total::Exact(0));
    assert!(empty.page.rows.is_empty());
    let parent = open(&api, request(Some(child.breadcrumbs[0].item.clone())))?;
    assert_eq!(parent.page.query_ref, root.page.query_ref);
    Ok(())
}

#[test]
fn selection_conditions_page_limits_and_replacement_are_isolated() -> Result<(), String> {
    let (sink, api) = fixture(false)?;
    let root = open(&api, request(None))?;
    let mut tree_req = request(None);
    tree_req.view = View::Tree;
    let tree = open(&api, tree_req)?;
    assert_ne!(root.page.query_ref, tree.page.query_ref);
    assert_eq!(tree.page.total, Total::LowerBound(1));
    let mut filtered_req = request(None);
    filtered_req.condition = query::Condition::Predicate(query::Predicate::Name(
        query::NameTest::Equal(b"a".to_vec()),
        query::NameEncoding::Bytes,
    ));
    let filtered = open(&api, filtered_req)?;
    assert_ne!(root.page.query_ref, filtered.page.query_ref);
    assert_eq!(filtered.page.total, Total::Exact(1));
    assert_eq!(api.open(request(None), || true), Err(Error::Cancelled));
    assert_eq!(
        api.page(
            &root.page.query_ref,
            &root.page.result_version,
            0,
            Some(1001)
        ),
        Err(Error::InvalidPage)
    );
    assert_eq!(
        api.page(&root.page.query_ref, &root.page.result_version, 4, Some(1)),
        Err(Error::InvalidPage)
    );
    let missing = ItemRef {
        result_version: root.page.result_version.clone(),
        item_no: u32::MAX,
    };
    assert_eq!(
        api.open(request(Some(missing)), || false),
        Err(Error::MissingLocation)
    );
    let mut build = sink.open(id(1)?).map_err(|e| format!("{e:?}"))?;
    build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    build
        .add_file(Observation {
            path: vec![b"a".to_vec()],
            size: None,
        })
        .map_err(|e| format!("{e:?}"))?;
    build
        .seal(id(3)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        api.page(&root.page.query_ref, &root.page.result_version, 0, None),
        Err(Error::StaleVersion)
    );
    assert_eq!(
        api.open(request(Some(root.page.rows[0].item.clone())), || false),
        Err(Error::StaleVersion)
    );
    let replaced = open(&api, request(None))?;
    assert_eq!(
        api.open(request(Some(replaced.page.rows[0].item.clone())), || false),
        Err(Error::NotFolder)
    );
    Ok(())
}

#[test]
fn partial_information_is_a_lower_bound_and_unknown_kind_is_not_a_folder() -> Result<(), String> {
    let (_, api) = fixture(true)?;
    let mut req = request(None);
    req.page_size = Some(10);
    let root = open(&api, req)?;
    assert_eq!(root.page.completeness, ResultCompleteness::Partial);
    assert_eq!(root.page.total, Total::LowerBound(2));
    let ambiguous = root
        .page
        .rows
        .iter()
        .find(|row| row.observation.name == b"ambiguous")
        .ok_or("missing row")?;
    assert_eq!(
        api.open(request(Some(ambiguous.item.clone())), || false),
        Err(Error::UnknownKind)
    );
    Ok(())
}
