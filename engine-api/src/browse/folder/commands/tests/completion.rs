use super::*;

#[test]
fn async_open_status_and_finished_event_share_query_reference()
-> Result<(), Box<dyn std::error::Error>> {
    let store = IngestSink::default();
    let mut scan = store.open(Ids.next_id()).map_err(|e| format!("{e:?}"))?;
    scan.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    for index in 0..SYNC_WORK {
        scan.add_directory(vec![format!("folder-{index:05}").into_bytes()])
            .map_err(|e| format!("{e:?}"))?;
    }
    let version = Ids.next_id();
    scan.seal(version.clone(), ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let events = Arc::new(DesktopEvents::default());
    let runtime = Arc::new(JobRuntime::new(
        Arc::new(Ids),
        Arc::new(Clock),
        Arc::new(History),
        events.clone(),
    )?);
    let workers = Arc::new(Workers::start(runtime.clone())?);
    let mut builder = EngineBuilder::default();
    crate::jobs::register(&mut builder, runtime.clone());
    register(
        &mut builder,
        Arc::new(ResultSets::new(store)),
        Arc::new(Ids),
        workers,
        &Entropy,
    )?;
    let engine = builder.build()?;
    let region = region(&engine);
    let open = call(
        &engine,
        "browse.open",
        json!({"regionRef":region,"viewKind":"list","requestSeq":1}),
        Channel::Gui,
    );
    assert!(open.error.is_none(), "{:?}", open.error);
    assert_eq!(open.body["kind"], "job");
    assert_eq!(open.body["resultVersion"], json!(version));
    let job: Id = serde_json::from_value(open.body["jobRef"].clone())?;
    let query = await_query(&runtime, &job)?;
    let status = call(&engine, "job.status", json!({"jobRef":job}), Channel::Gui);
    assert_eq!(status.body["queryRef"], json!(query));
    assert_eq!(status.body["resultVersion"], Value::Null);
    let deadline = Instant::now() + Duration::from_secs(4);
    loop {
        if let Some(value) = events.next_json() {
            let event: Value = serde_json::from_str(&value)?;
            if event["topic"] == "job.finished" && event["jobRef"] == json!(job) {
                assert_eq!(event["body"]["queryRef"], json!(query));
                assert!(event.get("resultVersion").is_none());
                break;
            }
        }
        if Instant::now() >= deadline {
            return Err("finished event did not arrive".into());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    Ok(())
}

#[test]
fn large_folder_returns_a_job_whose_reference_reads_the_original_result()
-> Result<(), Box<dyn std::error::Error>> {
    let (engine, runtime, version) = fixture(SYNC_WORK)?;
    let region_ref = region(&engine);
    let open = call(
        &engine,
        "browse.open",
        json!({"regionRef":region_ref,"viewKind":"list","requestSeq":8,"resultVersion":version}),
        Channel::Gui,
    );
    assert!(open.error.is_none(), "{:?}", open.error);
    assert_eq!(open.body["kind"], "job");
    assert_eq!(open.body["requestSeq"], 8);
    assert_eq!(open.body["resultVersion"], json!(version));
    let job: Id = serde_json::from_value(open.body["jobRef"].clone())?;
    let query_ref = await_query(&runtime, &job)?;
    let status = call(&engine, "job.status", json!({"jobRef":job}), Channel::Gui);
    assert!(status.error.is_none(), "{:?}", status.error);
    assert_eq!(status.body["queryRef"], json!(query_ref));
    assert_eq!(status.body["resultVersion"], Value::Null);
    let first = call(
        &engine,
        "query.page",
        json!({"queryRef":query_ref,"resultVersion":version,"start":0,"count":200}),
        Channel::Gui,
    );
    assert!(first.error.is_none(), "{:?}", first.error);
    assert_eq!(first.body["queryRef"], json!(query_ref));
    assert_eq!(first.body["resultVersion"], json!(version));
    let context = &first.body["browseContext"];
    assert_eq!(context["rootKind"], "scope_root");
    assert_eq!(context["origin"], "scan");
    assert_eq!(context["location"]["resultVersion"], json!(version));
    assert_eq!(context["breadcrumbs"][0]["item"], context["location"]);
    assert_eq!(
        first.body["rows"].as_array().ok_or("missing rows")?.len(),
        200
    );
    Ok(())
}
