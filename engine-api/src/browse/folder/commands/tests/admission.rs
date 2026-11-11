use super::*;

#[test]
fn busy_replacement_preserves_only_an_uncancelled_prior_job()
-> Result<(), Box<dyn std::error::Error>> {
    let _serial = GATE_TEST_LOCK.lock().map_err(|_| "gate lock poisoned")?;
    for explicitly_cancel in [false, true] {
        let (engine, runtime, version) = fixture(SYNC_WORK)?;
        let gate = super::super::super::test_gate::Gate::new();
        super::super::super::test_gate::set(Some((version.clone(), gate.clone())))?;
        let regions: Vec<_> = (0..4).map(|_| region(&engine)).collect();
        let mut jobs = Vec::new();
        for region_ref in &regions {
            let reply = call(
                &engine,
                "browse.open",
                json!({"regionRef":region_ref,"viewKind":"list","requestSeq":1,"resultVersion":version}),
                Channel::Gui,
            );
            assert!(reply.error.is_none(), "{:?}", reply.error);
            jobs.push(serde_json::from_value::<Id>(reply.body["jobRef"].clone())?);
        }
        gate.entered()?;
        if explicitly_cancel {
            let cancel = call(
                &engine,
                "query.cancel",
                json!({"regionRef":regions[0],"target":{"kind":"job","jobRef":jobs[0]}}),
                Channel::Gui,
            );
            assert!(cancel.error.is_none(), "{:?}", cancel.error);
        }
        let next = call(
            &engine,
            "browse.open",
            json!({"regionRef":regions[0],"viewKind":"list","requestSeq":2,"resultVersion":version}),
            Channel::Gui,
        );
        let state = runtime.state(&jobs[0])?;
        super::super::super::test_gate::set(None)?;
        for _ in 0..4 {
            gate.release()?;
        }
        assert_eq!(next.error.ok_or("expected busy")?.reason_key, "query.busy");
        if explicitly_cancel {
            assert_eq!(state, JobState::CancelRequested);
        } else {
            assert_eq!(state, JobState::Running);
            await_query(&runtime, &jobs[0])?;
        }
    }
    Ok(())
}

#[test]
fn closed_region_before_job_admission_keeps_existing_error()
-> Result<(), Box<dyn std::error::Error>> {
    let _serial = GATE_TEST_LOCK.lock().map_err(|_| "gate lock poisoned")?;
    let gate = PREPARE_GATE.get_or_init(super::super::super::test_gate::Gate::new);
    let (engine, runtime, version) = fixture(SYNC_WORK)?;
    let region_ref = region(&engine);
    let engine = Arc::new(engine);
    PAUSE_STAGE.store(4, std::sync::atomic::Ordering::Release);
    let (worker_engine, worker_region, worker_version) =
        (engine.clone(), region_ref.clone(), version.clone());
    let worker = std::thread::spawn(move || {
        call(
            &worker_engine,
            "browse.open",
            json!({"regionRef":worker_region,"viewKind":"list","requestSeq":90001,"resultVersion":worker_version}),
            Channel::Gui,
        )
    });
    gate.entered()?;
    let close = call(
        &engine,
        "query.region.close",
        json!({"regionRef":region_ref}),
        Channel::Gui,
    );
    gate.release()?;
    let reply = worker.join().map_err(|_| "thread panicked")?;
    PAUSE_STAGE.store(0, std::sync::atomic::Ordering::Release);
    assert!(close.error.is_none());
    assert_eq!(
        reply.error.ok_or("expected rejection")?.reason_key,
        "support.internal_error"
    );
    assert!(
        runtime.list(None)?.is_empty(),
        "closed region registered a job"
    );
    Ok(())
}

#[test]
fn simultaneous_duplicate_open_accepts_only_one() -> Result<(), Box<dyn std::error::Error>> {
    let _serial = GATE_TEST_LOCK.lock().map_err(|_| "gate lock poisoned")?;
    let (engine, runtime, version) = fixture(SYNC_WORK)?;
    let engine = Arc::new(engine);
    let region_ref = region(&engine);
    let barrier = Arc::new(std::sync::Barrier::new(3));
    let mut threads = Vec::new();
    for _ in 0..2 {
        let (engine, region_ref, version, barrier) = (
            engine.clone(),
            region_ref.clone(),
            version.clone(),
            barrier.clone(),
        );
        threads.push(std::thread::spawn(move || {
            barrier.wait();
            call(&engine,"browse.open",json!({"regionRef":region_ref,"viewKind":"list","requestSeq":1,"resultVersion":version}),Channel::Gui)
        }));
    }
    barrier.wait();
    let replies: Vec<_> = threads
        .into_iter()
        .map(|t| t.join().map_err(|_| "thread panicked"))
        .collect::<Result<_, _>>()?;
    assert_eq!(replies.iter().filter(|r| r.error.is_none()).count(), 1);
    assert_eq!(
        replies
            .iter()
            .filter_map(|r| r.error.as_ref())
            .filter(|e| e.reason_key == "command.body_invalid")
            .count(),
        1
    );
    let winner = replies
        .iter()
        .find(|r| r.error.is_none())
        .ok_or("missing accepted request")?;
    let job: Id = serde_json::from_value(winner.body["jobRef"].clone())?;
    await_query(&runtime, &job)?;
    Ok(())
}

#[test]
fn job_can_finish_before_open_response_without_losing_reference()
-> Result<(), Box<dyn std::error::Error>> {
    let _serial = GATE_TEST_LOCK.lock().map_err(|_| "gate lock poisoned")?;
    let gate = PREPARE_GATE.get_or_init(super::super::super::test_gate::Gate::new);
    let (engine, runtime, version) = fixture(SYNC_WORK)?;
    let region_ref = region(&engine);
    let engine = Arc::new(engine);
    PAUSE_STAGE.store(5, std::sync::atomic::Ordering::Release);
    let (worker_engine, worker_region, worker_version) =
        (engine.clone(), region_ref.clone(), version.clone());
    let worker = std::thread::spawn(move || {
        call(
            &worker_engine,
            "browse.open",
            json!({"regionRef":worker_region,"viewKind":"list","requestSeq":90001,"resultVersion":worker_version}),
            Channel::Gui,
        )
    });
    gate.entered()?;
    let jobs = runtime.list(None)?;
    assert_eq!(jobs.len(), 1);
    let job = jobs[0].job_id.clone();
    let query = await_query(&runtime, &job)?;
    gate.release()?;
    let reply = worker.join().map_err(|_| "thread panicked")?;
    PAUSE_STAGE.store(0, std::sync::atomic::Ordering::Release);
    assert!(reply.error.is_none(), "{:?}", reply.error);
    assert_eq!(reply.body["jobRef"], json!(job));
    let page = call(
        &engine,
        "query.page",
        json!({"queryRef":query,"resultVersion":version,"start":0,"count":1}),
        Channel::Gui,
    );
    assert!(page.error.is_none(), "{:?}", page.error);
    Ok(())
}
