//! End-to-end internal job handoff with the production result registry.
use du_analysis_index::{IngestSink, Observation, query};
use du_core_contracts::{
    Id, ItemRef, PathBytes, PathEncoding,
    job::{HistoryAppend, JobHistory, JobState, JobSummary},
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot},
};
use du_engine_api::browse::folder::{FolderBrowse, OpenRequest, Sort, View, job::FolderJobs};
use du_job_runtime::{JobClock, JobEvent, JobEvents, JobIdentity, JobRuntime};
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
struct Clock;
impl JobClock for Clock {
    fn now_ms(&self) -> i64 {
        1
    }
}
struct History;
impl JobHistory for History {
    fn append(&self, _: &JobSummary) -> Result<HistoryAppend, String> {
        Ok(HistoryAppend::Added)
    }
}
struct Events;
impl JobEvents for Events {
    fn publish(&self, _: JobEvent) {}
}
fn id(n: u8) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}
fn fixture() -> Result<(IngestSink, Arc<FolderBrowse>, Arc<JobRuntime>), String> {
    let store = IngestSink::default();
    let mut b = store.open(id(1)?).map_err(|e| format!("{e:?}"))?;
    b.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    b.add_directory(vec![b"dir".to_vec()])
        .map_err(|e| format!("{e:?}"))?;
    b.add_file(Observation {
        path: vec![b"dir".to_vec(), b"file".to_vec()],
        size: None,
    })
    .map_err(|e| format!("{e:?}"))?;
    b.seal(id(2)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let browse = Arc::new(FolderBrowse::new(
        store.clone(),
        Arc::new(Ids(AtomicU8::new(1))),
    ));
    let runtime = Arc::new(
        JobRuntime::new(
            Arc::new(Ids(AtomicU8::new(50))),
            Arc::new(Clock),
            Arc::new(History),
            Arc::new(Events),
        )
        .map_err(|e| format!("{e:?}"))?,
    );
    Ok((store, browse, runtime))
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
#[test]
fn queued_job_publishes_folder_and_first_page_only_after_completion() -> Result<(), String> {
    let (store, browse, runtime) = fixture()?;
    let jobs = FolderJobs::new(browse.clone(), runtime.clone()).map_err(|e| format!("{e:?}"))?;
    let reference = jobs
        .submit("main", id(8)?, request(None), &[])
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(runtime.state(reference.id()), Ok(JobState::Queued));
    runtime.run_next().map_err(|e| format!("{e:?}"))?;
    let snapshot = runtime
        .snapshot(reference.id())
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(snapshot.state, JobState::Done);
    let query_ref = snapshot
        .result_version_id
        .ok_or("missing query reference")?;
    let page = browse
        .page(&query_ref, &id(2)?, 0, Some(1))
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(page.rows[0].observation.name, b"dir");
    let nested = jobs
        .submit(
            "main",
            id(9)?,
            request(Some(page.rows[0].item.clone())),
            &[],
        )
        .map_err(|e| format!("{e:?}"))?;
    runtime.run_next().map_err(|e| format!("{e:?}"))?;
    let nested_ref = runtime
        .snapshot(nested.id())
        .map_err(|e| format!("{e:?}"))?
        .result_version_id
        .ok_or("missing nested query")?;
    assert_eq!(
        browse
            .page(&nested_ref, &id(2)?, 0, Some(1))
            .map_err(|e| format!("{e:?}"))?
            .rows[0]
            .observation
            .name,
        b"file"
    );
    let mut replacement = store.open(id(3)?).map_err(|e| format!("{e:?}"))?;
    replacement
        .add_directory(vec![])
        .map_err(|e| format!("{e:?}"))?;
    replacement
        .seal(id(4)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    assert!(matches!(
        browse.page(&query_ref, &id(2)?, 0, Some(1)),
        Err(du_engine_api::browse::folder::Error::StaleVersion)
    ));
    Ok(())
}
#[test]
fn same_channel_cancels_queued_predecessor_and_does_not_publish_it() -> Result<(), String> {
    let (_, browse, runtime) = fixture()?;
    let jobs = FolderJobs::new(browse.clone(), runtime.clone()).map_err(|e| format!("{e:?}"))?;
    let first = jobs
        .submit("main", id(8)?, request(None), &[])
        .map_err(|e| format!("{e:?}"))?;
    let second = jobs
        .submit("main", id(9)?, request(None), &[])
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(runtime.state(first.id()), Ok(JobState::Cancelled));
    assert!(
        runtime
            .snapshot(first.id())
            .map_err(|e| format!("{e:?}"))?
            .result_version_id
            .is_none()
    );
    assert_eq!(runtime.state(second.id()), Ok(JobState::Queued));
    runtime.run_next().map_err(|e| format!("{e:?}"))?;
    assert_eq!(runtime.state(second.id()), Ok(JobState::Done));
    Ok(())
}
#[test]
fn replacement_after_admission_never_substitutes_the_new_version() -> Result<(), String> {
    let (store, browse, runtime) = fixture()?;
    let jobs = FolderJobs::new(browse.clone(), runtime.clone()).map_err(|e| format!("{e:?}"))?;
    let job = jobs
        .submit("main", id(8)?, request(None), &[])
        .map_err(|e| format!("{e:?}"))?;
    let mut next = store.open(id(3)?).map_err(|e| format!("{e:?}"))?;
    next.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    next.seal(id(4)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    runtime.run_next().map_err(|e| format!("{e:?}"))?;
    let snapshot = runtime.snapshot(job.id()).map_err(|e| format!("{e:?}"))?;
    assert_eq!(snapshot.state, JobState::Done);
    let reference = snapshot.result_version_id.ok_or("missing query")?;
    let first = job
        .take_first()
        .map_err(|e| format!("{e:?}"))?
        .ok_or("missing first page")?;
    assert_eq!(first.page.result_version, id(2)?);
    assert_eq!(first.page.query_ref, reference);
    assert!(job.take_first().map_err(|e| format!("{e:?}"))?.is_none());
    assert!(matches!(
        browse.page(&reference, &id(2)?, 0, Some(1)),
        Err(du_engine_api::browse::folder::Error::StaleVersion)
    ));
    Ok(())
}

#[test]
fn job_evaluates_virtual_top_and_rejects_unknown_location() -> Result<(), String> {
    let store = IngestSink::default();
    let roots = [b"/a".as_slice(), b"/b".as_slice()].map(|path| ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(path),
        },
        volume_id: vec![1],
        file_id: path.to_vec(),
    });
    let mut build = store
        .open_forest(id(1)?, roots.to_vec())
        .map_err(|e| format!("{e:?}"))?;
    for root in 0..2 {
        build
            .add_directory_at(root, vec![])
            .map_err(|e| format!("{e:?}"))?;
    }
    build
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let browse = Arc::new(FolderBrowse::new(store, Arc::new(Ids(AtomicU8::new(1)))));
    let runtime = Arc::new(
        JobRuntime::new(
            Arc::new(Ids(AtomicU8::new(50))),
            Arc::new(Clock),
            Arc::new(History),
            Arc::new(Events),
        )
        .map_err(|e| format!("{e:?}"))?,
    );
    let jobs = FolderJobs::new(browse.clone(), runtime.clone()).map_err(|e| format!("{e:?}"))?;
    let invalid = ItemRef {
        result_version: id(2)?,
        item_no: u32::MAX,
    };
    assert!(
        jobs.submit("main", id(8)?, request(Some(invalid)), &[])
            .is_err()
    );
    let job = jobs
        .submit("main", id(9)?, request(None), &[])
        .map_err(|e| format!("{e:?}"))?;
    runtime.run_next().map_err(|e| format!("{e:?}"))?;
    let q = runtime
        .snapshot(job.id())
        .map_err(|e| format!("{e:?}"))?
        .result_version_id
        .ok_or("missing query")?;
    let first = browse
        .page(&q, &id(2)?, 0, Some(1))
        .map_err(|e| format!("{e:?}"))?;
    let second = browse
        .page(&q, &id(2)?, 1, Some(1))
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(first.rows[0].display_name.as_deref(), Some("/a"));
    assert_eq!(second.rows[0].display_name.as_deref(), Some("/b"));
    Ok(())
}

#[test]
fn four_job_limit_and_completed_channel_reaping() -> Result<(), String> {
    let (_, browse, runtime) = fixture()?;
    let jobs = FolderJobs::new(browse, runtime.clone()).map_err(|e| format!("{e:?}"))?;
    for channel in ["a", "b", "c", "d"] {
        jobs.submit(channel, id(8)?, request(None), &[])
            .map_err(|e| format!("{e:?}"))?;
    }
    assert!(matches!(
        jobs.submit("e", id(8)?, request(None), &[]),
        Err(du_engine_api::browse::folder::job::ErrorKind::Busy)
    ));
    for _ in 0..4 {
        runtime.run_next().map_err(|e| format!("{e:?}"))?;
    }
    for n in 0..20 {
        let ticket = jobs
            .submit(&format!("new-{n}"), id(8)?, request(None), &[])
            .map_err(|e| format!("{e:?}"))?;
        runtime.run_next().map_err(|e| format!("{e:?}"))?;
        assert_eq!(runtime.state(ticket.id()), Ok(JobState::Done));
    }
    Ok(())
}

#[test]
fn identical_jobs_reuse_the_registered_query_reference() -> Result<(), String> {
    let (_, browse, runtime) = fixture()?;
    let jobs = FolderJobs::new(browse, runtime.clone()).map_err(|e| format!("{e:?}"))?;
    let first = jobs
        .submit("a", id(8)?, request(None), &[])
        .map_err(|e| format!("{e:?}"))?;
    runtime.run_next().map_err(|e| format!("{e:?}"))?;
    let a = runtime
        .snapshot(first.id())
        .map_err(|e| format!("{e:?}"))?
        .result_version_id
        .ok_or("missing a")?;
    let second = jobs
        .submit("b", id(9)?, request(None), &[])
        .map_err(|e| format!("{e:?}"))?;
    runtime.run_next().map_err(|e| format!("{e:?}"))?;
    let b = runtime
        .snapshot(second.id())
        .map_err(|e| format!("{e:?}"))?
        .result_version_id
        .ok_or("missing b")?;
    assert_eq!(a, b);
    Ok(())
}
#[test]
fn typed_ticket_preserves_admission_across_replacement_and_channel_cancel() -> Result<(), String> {
    use du_core_contracts::browse_open_ipc::BrowseOpenRequest;
    use du_engine_api::browse::folder::cursor::Session;
    let (store, browse, runtime) = fixture()?;
    let session = Arc::new(Session::new(browse.clone(), [7; 32]));
    let jobs = FolderJobs::new(browse, runtime.clone()).map_err(|e| format!("{e:?}"))?;
    let request = |sequence| -> Result<BrowseOpenRequest, String> {
        serde_json::from_value(
            serde_json::json!({"viewKind":"list", "requestSeq":sequence, "pageSize":1}),
        )
        .map_err(|e| e.to_string())
    };
    let first = jobs
        .submit_live(session.clone(), "main", id(8)?, request(31)?)
        .map_err(|e| format!("{e:?}"))?;
    let second = jobs
        .submit_live(session.clone(), "main", id(9)?, request(32)?)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(runtime.state(first.id()), Ok(JobState::Cancelled));
    assert!(first.take_first().map_err(|e| format!("{e:?}"))?.is_none());
    let mut replacement = store.open(id(3)?).map_err(|e| format!("{e:?}"))?;
    replacement
        .add_directory(vec![])
        .map_err(|e| format!("{e:?}"))?;
    replacement
        .seal(id(4)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    runtime.run_next().map_err(|e| format!("{e:?}"))?;
    let result = second
        .take_first()
        .map_err(|e| format!("{e:?}"))?
        .ok_or("missing page")?;
    assert_eq!(result.request_seq, 32);
    assert_eq!(result.page.opened.page.result_version, id(2)?);
    assert_eq!(result.page.opened.page.rows[0].observation.name, b"dir");
    assert!(second.take_first().map_err(|e| format!("{e:?}"))?.is_none());
    assert!(matches!(
        session.page_at(&result.page.opened.page.query_ref, &id(2)?, 0, Some(1)),
        Err(du_engine_api::browse::folder::cursor::CursorError::Stale)
    ));
    Ok(())
}
#[test]
fn typed_request_rejects_unrelated_session_without_cancelling_current_job() -> Result<(), String> {
    use du_core_contracts::browse_open_ipc::BrowseOpenRequest;
    use du_engine_api::browse::folder::{cursor::Session, job::ErrorKind};
    let (_, browse, runtime) = fixture()?;
    let session = Arc::new(Session::new(browse.clone(), [1; 32]));
    let other = Arc::new(Session::new(
        Arc::new(FolderBrowse::new(
            IngestSink::default(),
            Arc::new(Ids(AtomicU8::new(1))),
        )),
        [2; 32],
    ));
    let jobs = FolderJobs::new(browse, runtime.clone()).map_err(|e| format!("{e:?}"))?;
    let request = || -> Result<BrowseOpenRequest, String> {
        serde_json::from_value(serde_json::json!({"viewKind":"list", "requestSeq":1}))
            .map_err(|e| e.to_string())
    };
    let active = jobs
        .submit_live(session, "main", id(8)?, request()?)
        .map_err(|e| format!("{e:?}"))?;
    assert!(matches!(
        jobs.submit_live(other, "main", id(9)?, request()?),
        Err(ErrorKind::Unavailable)
    ));
    assert_eq!(runtime.state(active.id()), Ok(JobState::Queued));
    Ok(())
}
