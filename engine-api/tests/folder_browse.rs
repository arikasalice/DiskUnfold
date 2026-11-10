use du_analysis_index::{
    IngestSink, Observation,
    query::{self, page::Total},
};
use du_core_contracts::{
    Id, ItemRef, PathBytes, PathEncoding,
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot},
};
use du_engine_api::browse::folder::{
    Error, FolderBrowse, OpenRequest, Sort, View, cursor::Session,
};
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
    let cursor = first.next_cursor.ok_or("missing forest cursor")?;
    let next = session
        .page(&cursor, Some(1))
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(next.page.rows[0].display_name.as_deref(), Some("/z"));
    assert_eq!(next.page.rows[0].item.item_no, 0);
    assert!(next.next_cursor.is_none());

    let a = open(&api, request(Some(top.page.rows[0].item.clone())))?;
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
    cancellation::check(&api, &top.page.query_ref)?;
    Ok(())
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
    cancellation::check(&api, &root.page.query_ref)?;
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

fn numbered_request(n: u8) -> OpenRequest {
    let mut request = request(None);
    request.condition = query::Condition::Predicate(query::Predicate::Name(
        query::NameTest::Equal(vec![n]),
        query::NameEncoding::Bytes,
    ));
    request
}

#[test]
fn page_reads_refresh_the_single_lru_and_evict_the_oldest_query() -> Result<(), String> {
    let (_, api) = fixture(false)?;
    let mut opened = Vec::new();
    for n in 0..16 {
        opened.push(open(&api, numbered_request(n))?);
    }
    let first = &opened[0].page;
    api.page(&first.query_ref, &first.result_version, 0, None)
        .map_err(|e| format!("{e:?}"))?;
    let _ = open(&api, numbered_request(16))?;
    assert_eq!(
        open(&api, numbered_request(0))?.page.query_ref,
        first.query_ref
    );
    let second = &opened[1].page;
    assert_eq!(
        api.page(&second.query_ref, &second.result_version, 0, None),
        Err(Error::StaleVersion)
    );
    assert_ne!(
        open(&api, numbered_request(1))?.page.query_ref,
        second.query_ref
    );
    Ok(())
}

#[test]
fn long_evaluation_does_not_block_an_unrelated_page_and_replacement_rejects_old_work()
-> Result<(), String> {
    use std::{sync::mpsc, thread, time::Duration};
    let (sink, api) = fixture(false)?;
    let api = Arc::new(api);
    let existing = open(&api, numbered_request(0))?;
    let (started_tx, started_rx) = mpsc::channel();
    let (resume_tx, resume_rx) = mpsc::channel();
    let worker_api = Arc::clone(&api);
    let worker = thread::spawn(move || {
        let mut calls = 0;
        worker_api.open(numbered_request(99), || {
            calls += 1;
            if calls == 2 {
                let _ = started_tx.send(());
                let _ = resume_rx.recv();
            }
            false
        })
    });
    started_rx
        .recv_timeout(Duration::from_secs(5))
        .map_err(|e| format!("{e:?}"))?;
    let (page_tx, page_rx) = mpsc::channel();
    let reader = Arc::clone(&api);
    let old = existing.page.clone();
    let reader_thread = thread::spawn(move || {
        let _ = page_tx.send(reader.page(&old.query_ref, &old.result_version, 0, None));
    });
    let independent = page_rx.recv_timeout(Duration::from_secs(5));
    let mut build = sink.open(id(1)?).map_err(|e| format!("{e:?}"))?;
    build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    build
        .seal(id(3)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    resume_tx.send(()).map_err(|e| format!("{e:?}"))?;
    reader_thread.join().map_err(|_| "reader thread panicked")?;
    let result = worker.join().map_err(|_| "worker thread panicked")?;
    assert!(
        independent
            .map_err(|e| format!("page blocked: {e:?}"))?
            .is_ok()
    );
    assert_eq!(result, Err(Error::StaleVersion));
    assert_eq!(
        api.page(
            &existing.page.query_ref,
            &existing.page.result_version,
            0,
            None
        ),
        Err(Error::StaleVersion)
    );
    Ok(())
}

#[test]
fn concurrent_identical_evaluations_publish_one_reference() -> Result<(), String> {
    use std::{sync::Barrier, thread};
    let (_, api) = fixture(false)?;
    let api = Arc::new(api);
    let barrier = Arc::new(Barrier::new(2));
    let workers: Vec<_> = (0..2)
        .map(|_| {
            let api = Arc::clone(&api);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                let mut calls = 0;
                api.open(numbered_request(90), || {
                    calls += 1;
                    if calls == 2 {
                        barrier.wait();
                    }
                    false
                })
            })
        })
        .collect();
    let mut refs = Vec::new();
    for worker in workers {
        refs.push(
            worker
                .join()
                .map_err(|_| "worker thread panicked")?
                .map_err(|e| format!("{e:?}"))?
                .page
                .query_ref,
        );
    }
    assert_eq!(refs[0], refs[1]);
    Ok(())
}

#[test]
fn ordered_sorts_reuse_the_full_key_and_pin_pages() -> Result<(), String> {
    use du_core_contracts::{ByteSize, SizeAmount, SizeBasis, SizeEvidence, SizeValue};
    let sink = IngestSink::default();
    let mut build = sink.open(id(1)?).map_err(|e| format!("{e:?}"))?;
    build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    for (name, amount) in [("a", 10), ("b", 10), ("c", 5)] {
        let value = SizeValue {
            basis: SizeBasis::Logical,
            evidence: SizeEvidence::Measured,
            amount: SizeAmount::Exact {
                bytes: ByteSize::from(amount),
            },
        };
        build
            .add_file(Observation {
                path: vec![name.as_bytes().to_vec()],
                size: Some(value),
            })
            .map_err(|e| format!("{e:?}"))?;
    }
    build
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let api = FolderBrowse::new(sink.clone(), Arc::new(Ids(AtomicU8::new(1))));
    let mut req = request(None);
    req.sort = Sort {
        id: "sort.size".into(),
        revision: 1,
        basis: query::Basis::Logical,
        descending: true,
    };
    let secondary = Sort {
        id: "sort.name".into(),
        revision: 1,
        basis: query::Basis::Logical,
        descending: true,
    };
    let first = api
        .open_with_sorts(req.clone(), std::slice::from_ref(&secondary), || false)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(first.page.rows[0].observation.name, b"b");
    let repeated = api
        .open_with_sorts(req.clone(), std::slice::from_ref(&secondary), || false)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(first.page.query_ref, repeated.page.query_ref);
    let next = api
        .page(
            &first.page.query_ref,
            &first.page.result_version,
            1,
            Some(1),
        )
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(next.rows[0].observation.name, b"a");
    let ascending = Sort {
        descending: false,
        ..secondary.clone()
    };
    let other = api
        .open_with_sorts(req.clone(), &[ascending], || false)
        .map_err(|e| format!("{e:?}"))?;
    assert_ne!(other.page.query_ref, first.page.query_ref);
    assert_eq!(other.page.rows[0].observation.name, b"a");
    let unsupported = Sort {
        revision: 2,
        ..secondary.clone()
    };
    assert_eq!(
        api.open_with_sorts(req.clone(), &[unsupported], || false),
        Err(Error::UnknownSort)
    );
    let mut swapped = req.clone();
    swapped.sort = secondary.clone();
    let reordered = api
        .open_with_sorts(swapped, &[req.sort.clone()], || false)
        .map_err(|e| format!("{e:?}"))?;
    assert_ne!(reordered.page.query_ref, first.page.query_ref);
    assert_eq!(reordered.page.rows[0].observation.name, b"c");
    let single = open(&api, req.clone())?;
    let single_again = api
        .open_with_sorts(req.clone(), &[], || false)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(single.page.query_ref, single_again.page.query_ref);
    let duplicate = api
        .open_with_sorts(req, &[secondary.clone(), secondary], || false)
        .map_err(|e| format!("{e:?}"))?;
    assert_ne!(duplicate.page.query_ref, first.page.query_ref);
    let mut build = sink.open(id(1)?).map_err(|e| format!("{e:?}"))?;
    build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    build
        .seal(id(3)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(next.rows[0].observation.name, b"a");
    assert_eq!(
        api.page(
            &first.page.query_ref,
            &first.page.result_version,
            2,
            Some(1)
        ),
        Err(Error::StaleVersion)
    );
    Ok(())
}

mod cancellation;
