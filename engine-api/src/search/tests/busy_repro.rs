use super::*;

#[test]
fn busy_replacement_keeps_the_previous_job() -> Result<(), String> {
    let _serial = test_gate::serial().map_err(str::to_owned)?;
    let store = IngestSink::default();
    let mut scan = store.open(id(1)).map_err(|e| format!("{e:?}"))?;
    scan.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    scan.seal(id(2), ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let sets = Arc::new(ResultSets::new(store));
    let ids = Arc::new(Identity {
        next: AtomicUsize::new(0),
        cancel_on: AtomicUsize::new(usize::MAX),
        replace_on: AtomicUsize::new(usize::MAX),
        caller: Mutex::new(None),
        store: Mutex::new(None),
    });
    let runtime = Arc::new(
        JobRuntime::new(
            ids.clone(),
            Arc::new(Clock),
            Arc::new(History),
            Arc::new(DesktopEvents::default()),
        )
        .map_err(|e| format!("{e:?}"))?,
    );
    let mut builder = EngineBuilder::default();
    let regions = Regions::register(&mut builder, ids.clone(), &Entropy, runtime.clone())
        .map_err(|e| format!("{e:?}"))?;
    let mut region_ids = Vec::new();
    for _ in 0..4 {
        region_ids.push(
            regions
                .registry
                .open(&regions.owner, QueryRegionKind::Search)
                .map_err(|e| format!("{e:?}"))?,
        );
    }
    let browse = Arc::new(FolderBrowse::with_result_sets(sets.clone(), ids.clone()));
    let jobs = Arc::new(FolderJobs::new(browse, runtime.clone()).map_err(|e| format!("{e:?}"))?);
    let workers = Arc::new(Workers::start(runtime.clone()).map_err(|e| format!("{e:?}"))?);
    let entry = SearchEntry::new(sets, regions, ids, SearchWorkers::new(jobs, workers));
    let gate = test_gate::Gate::new(test_gate::Stage::Worker);
    test_gate::set(Some((id(2), gate.clone()))).map_err(str::to_owned)?;
    let request = |region_ref: Id, request_seq| SearchRequest {
        region_ref,
        request_seq,
        result_version: Some(id(2)),
        parent_search_ref: None,
        condition: Condition::All(vec![]),
        sorts: vec![SortSpec {
            id: "sort.path".into(),
            revision: 1,
            basis: query::Basis::Logical,
            descending: false,
        }],
        count: None,
    };
    let caller = CallerCtx::new(Channel::Gui, id(90), None);
    let mut active = Vec::new();
    for region in &region_ids {
        active.push(
            entry
                .execute_async(request(region.clone(), 1), &caller)
                .map_err(|e| format!("{e:?}"))?,
        );
    }
    gate.entered().map_err(str::to_owned)?;
    let result = entry.execute_async(request(region_ids[0].clone(), 2), &caller);
    let previous_state = runtime
        .state(&active[0].job_ref)
        .map_err(|e| format!("{e:?}"))?;
    test_gate::set(None).map_err(str::to_owned)?;
    for _ in 0..4 {
        gate.release().map_err(str::to_owned)?;
    }
    let terminal = wait_terminal(&runtime, &active[0].job_ref)?;
    assert!(matches!(result, Err(SearchError::Busy)));
    assert!(
        !matches!(
            previous_state,
            du_core_contracts::job::JobState::CancelRequested
                | du_core_contracts::job::JobState::Cancelled
        ),
        "busy request cancelled the previous job: {previous_state:?}"
    );
    assert_eq!(terminal.state, du_core_contracts::job::JobState::Done);
    assert!(terminal.result_version_id.is_some());
    Ok(())
}

#[test]
fn simultaneous_same_sequence_accepts_only_one_job() -> Result<(), String> {
    let _serial = test_gate::serial().map_err(str::to_owned)?;
    let store = IngestSink::default();
    let mut scan = store.open(id(1)).map_err(|e| format!("{e:?}"))?;
    scan.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    scan.seal(id(2), ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let sets = Arc::new(ResultSets::new(store));
    let ids = Arc::new(Identity {
        next: AtomicUsize::new(0),
        cancel_on: AtomicUsize::new(usize::MAX),
        replace_on: AtomicUsize::new(usize::MAX),
        caller: Mutex::new(None),
        store: Mutex::new(None),
    });
    let runtime = Arc::new(
        JobRuntime::new(
            ids.clone(),
            Arc::new(Clock),
            Arc::new(History),
            Arc::new(DesktopEvents::default()),
        )
        .map_err(|e| format!("{e:?}"))?,
    );
    let mut builder = EngineBuilder::default();
    let regions = Regions::register(&mut builder, ids.clone(), &Entropy, runtime.clone())
        .map_err(|e| format!("{e:?}"))?;
    let region = regions
        .registry
        .open(&regions.owner, QueryRegionKind::Search)
        .map_err(|e| format!("{e:?}"))?;
    let browse = Arc::new(FolderBrowse::with_result_sets(sets.clone(), ids.clone()));
    let jobs = Arc::new(FolderJobs::new(browse, runtime.clone()).map_err(|e| format!("{e:?}"))?);
    let workers = Arc::new(Workers::start(runtime.clone()).map_err(|e| format!("{e:?}"))?);
    let entry = Arc::new(SearchEntry::new(
        sets,
        regions,
        ids,
        SearchWorkers::new(jobs, workers),
    ));
    let barrier = Arc::new(std::sync::Barrier::new(3));
    let mut threads = Vec::new();
    for _ in 0..2 {
        let (entry, region, barrier) = (entry.clone(), region.clone(), barrier.clone());
        threads.push(std::thread::spawn(move || {
            let request = SearchRequest {
                region_ref: region,
                request_seq: 1,
                result_version: Some(id(2)),
                parent_search_ref: None,
                condition: Condition::All(vec![]),
                sorts: vec![SortSpec {
                    id: "sort.path".into(),
                    revision: 1,
                    basis: query::Basis::Logical,
                    descending: false,
                }],
                count: None,
            };
            barrier.wait();
            entry.execute_async(request, &CallerCtx::new(Channel::Gui, id(90), None))
        }));
    }
    barrier.wait();
    let outcomes: Vec<_> = threads
        .into_iter()
        .map(|t| t.join().map_err(|_| "thread panicked".to_owned()))
        .collect::<Result<_, _>>()?;
    assert_eq!(outcomes.iter().filter(|r| r.is_ok()).count(), 1);
    let accepted = outcomes
        .into_iter()
        .find_map(Result::ok)
        .ok_or("no accepted job")?;
    assert_eq!(
        wait_terminal(&runtime, &accepted.job_ref)?.state,
        du_core_contracts::job::JobState::Done
    );
    Ok(())
}
