use super::*;
use du_core_contracts::{
    ByteSize, OsPath, PathBytes, PathEncoding, Timestamp,
    platform_os_change::{
        ChangeIdentity, ChangeItemObservation, TrashLocationRef, TrashMoveOutcome,
    },
    ports::EntryKind,
    volume::Capability,
};

#[test]
fn approved_trash_records_each_item_through_f21_and_registers_session_undo() -> TestResult {
    let analysis = FakeAnalysis::real().with_trash_facts();
    let ops = fake_change_ops(&[1, 2], CapabilityState::Supported)?;
    for item_no in [1, 2] {
        ops.enqueue_move_outcome(TrashMoveOutcome::Moved {
            identity_after_move: change_identity(item_no),
            trash_location: Some(TrashLocationRef::from_adapter(format!(
                "fake-trash-{item_no}"
            ))),
        });
    }
    let undo = Arc::new(crate::session_undo_info::SessionUndoInfoRegistry::default());
    let app = assemble_with_ports(analysis.clone(), None, ops.clone(), undo.clone())?;
    let plan_id = build_approved_trash_plan(&app, &analysis, &[1, 2])?;
    let execute = ok(
        &app.engine,
        "change.execute",
        json!({"requestVersionId":plan_id}),
    )?;
    let job_ref: Id = serde_json::from_value(execute["jobRef"].clone())?;
    let terminal = wait_for_job(&app.engine, &execute["jobRef"])?;
    assert_eq!(terminal["state"], "done", "terminal={terminal}");
    wait_for_finished_event(&app.events, &job_ref)?;

    let page = ok(
        &app.engine,
        "change.item_results",
        json!({"jobRef":job_ref}),
    )?;
    assert_eq!(page["counts"]["completed"], 2);
    assert_eq!(page["itemRecording"], "recorded");
    assert_eq!(page["items"].as_array().map(Vec::len), Some(2));
    assert!(page["items"].as_array().is_some_and(|items| {
        items
            .iter()
            .all(|item| item["state"] == "completed" && item["undoAvailable"] == true)
    }));

    let database = rusqlite::Connection::open(
        app.profile
            .path()
            .join(du_platform_os::application_data::DATABASE_FILE),
    )?;
    let job_bytes = job_ref.v7_bytes().ok_or("invalid job id")?;
    let rows: Vec<(i64, String)> = {
        let mut statement = database.prepare(
            "SELECT item_seq, outcome FROM job_runtime_item_result WHERE job_id=?1 ORDER BY item_seq",
        )?;
        statement
            .query_map([job_bytes.as_slice()], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<_, _>>()?
    };
    assert_eq!(rows, [(0, "completed".into()), (1, "completed".into())]);
    let history_page = ok(&app.engine, "history.items", json!({"jobRef":job_ref}))?;
    assert_eq!(history_page["itemRecording"], "recorded");
    assert_eq!(history_page["counts"]["recorded"], "2");
    let finish_state: String = database.query_row(
        "SELECT end_state FROM job_runtime_approval_check
         WHERE job_id=?1 AND record_kind='finish'",
        [job_bytes.as_slice()],
        |row| row.get(0),
    )?;
    assert_eq!(finish_state, "done");
    let approval = ok(
        &app.engine,
        "approval.review",
        json!({"requestVersionId":plan_id,"requestKind":"plan"}),
    )?;
    assert_eq!(approval["currentApprovalState"], "spent");
    for item_no in [1, 2] {
        assert!(
            app.undo
                .get_trashed(&job_ref, item_no)
                .map_err(|error| format!("undo lookup failed: {error:?}"))?
                .is_some()
        );
    }
    let jobs_before_replay: i64 =
        database.query_row("SELECT COUNT(*) FROM job_runtime_job_start", [], |row| {
            row.get(0)
        })?;

    let replay = call(
        &app.engine,
        "change.execute",
        json!({"requestVersionId":plan_id}),
    )?;
    assert_eq!(
        replay.error.as_ref().map(|error| error.reason_key.as_str()),
        Some(du_core_contracts::approval_ipc::APPROVAL_ALREADY_USED),
    );
    assert_eq!(ops.simulated_change_call_count(), 2);
    let jobs_after_replay: i64 =
        database.query_row("SELECT COUNT(*) FROM job_runtime_job_start", [], |row| {
            row.get(0)
        })?;
    assert_eq!(jobs_after_replay, jobs_before_replay);
    Ok(())
}

#[test]
fn unsupported_trash_capability_skips_every_item_without_change_calls() -> TestResult {
    let analysis = FakeAnalysis::real().with_trash_facts();
    let ops = fake_change_ops(&[1, 2], CapabilityState::Unsupported)?;
    let app = assemble_with_ports(
        analysis.clone(),
        None,
        ops.clone(),
        Arc::new(crate::session_undo_info::SessionUndoInfoRegistry::default()),
    )?;
    let plan_id = build_approved_trash_plan(&app, &analysis, &[1, 2])?;
    let execute = ok(
        &app.engine,
        "change.execute",
        json!({"requestVersionId":plan_id}),
    )?;
    let job_ref: Id = serde_json::from_value(execute["jobRef"].clone())?;
    let terminal = wait_for_job(&app.engine, &execute["jobRef"])?;
    assert_eq!(terminal["state"], "failed");
    wait_for_finished_event(&app.events, &job_ref)?;
    let page = ok(
        &app.engine,
        "change.item_results",
        json!({"jobRef":job_ref}),
    )?;
    assert_eq!(page["counts"]["skipped"], 2);
    assert_eq!(page["itemRecording"], "recorded");
    assert!(page["items"].as_array().is_some_and(|items| {
        items.iter().all(|item| {
            item["reasonKey"] == du_core_contracts::change_ipc::CHANGE_ITEM_TRASH_UNSUPPORTED
        })
    }));
    assert_eq!(ops.simulated_change_call_count(), 0);
    assert!(
        !ops.calls()
            .contains(&du_platform_os::fake::FakeChangeCall::Trash)
    );
    assert_eq!(history_item_count(&app, &job_ref)?, 2);
    Ok(())
}

#[test]
fn identity_race_stops_the_batch_and_unknown_outcome_is_queried_once() -> TestResult {
    let analysis = FakeAnalysis::real().with_trash_facts();
    let ops = fake_change_ops(&[1, 2], CapabilityState::Supported)?;
    let mut raced_identity = change_identity(1);
    raced_identity.file_id = b"replacement-file".to_vec();
    ops.enqueue_move_outcome(TrashMoveOutcome::Moved {
        identity_after_move: raced_identity,
        trash_location: Some(TrashLocationRef::from_adapter("race-trash".into())),
    });
    let app = assemble_with_ports(
        analysis.clone(),
        None,
        ops.clone(),
        Arc::new(crate::session_undo_info::SessionUndoInfoRegistry::default()),
    )?;
    let plan_id = build_approved_trash_plan(&app, &analysis, &[1, 2])?;
    let execute = ok(
        &app.engine,
        "change.execute",
        json!({"requestVersionId":plan_id}),
    )?;
    let job_ref: Id = serde_json::from_value(execute["jobRef"].clone())?;
    wait_for_job(&app.engine, &execute["jobRef"])?;
    wait_for_finished_event(&app.events, &job_ref)?;
    let page = ok(
        &app.engine,
        "change.item_results",
        json!({"jobRef":job_ref}),
    )?;
    assert_eq!(page["counts"]["failed"], 1);
    assert_eq!(page["counts"]["unprocessed"], 1);
    assert_eq!(page["items"][0]["reasonKey"], "change.item.identity_race");
    assert_eq!(ops.simulated_change_call_count(), 1);

    let analysis = FakeAnalysis::real().with_trash_facts();
    let ops = fake_change_ops(&[1], CapabilityState::Supported)?;
    ops.enqueue_move_outcome(TrashMoveOutcome::OutcomeUnknown);
    ops.set_query_outcome(du_core_contracts::platform_os_change::TrashOutcomeQuery::OutcomeUnknown);
    let app = assemble_with_ports(
        analysis.clone(),
        None,
        ops.clone(),
        Arc::new(crate::session_undo_info::SessionUndoInfoRegistry::default()),
    )?;
    let plan_id = build_approved_trash_plan(&app, &analysis, &[1])?;
    let execute = ok(
        &app.engine,
        "change.execute",
        json!({"requestVersionId":plan_id}),
    )?;
    let job_ref: Id = serde_json::from_value(execute["jobRef"].clone())?;
    wait_for_job(&app.engine, &execute["jobRef"])?;
    wait_for_finished_event(&app.events, &job_ref)?;
    let page = ok(
        &app.engine,
        "change.item_results",
        json!({"jobRef":job_ref}),
    )?;
    assert_eq!(page["items"][0]["reasonKey"], "change.item.outcome_unknown");
    assert_eq!(page["items"][0]["outcomeUnknown"], true);
    assert_eq!(
        ops.calls()
            .iter()
            .filter(|call| **call == du_platform_os::fake::FakeChangeCall::Query)
            .count(),
        1,
    );
    Ok(())
}

#[test]
fn cancellation_records_remaining_items_and_history_failure_stops_the_job() -> TestResult {
    let analysis = FakeAnalysis::real().with_trash_facts();
    let ops = fake_change_ops(&[1, 2, 3], CapabilityState::Supported)?;
    ops.enqueue_move_outcome(TrashMoveOutcome::Moved {
        identity_after_move: change_identity(1),
        trash_location: None,
    });
    ops.enqueue_move_outcome(TrashMoveOutcome::Moved {
        identity_after_move: change_identity(2),
        trash_location: None,
    });
    let gate = ops.hold_next_lookup();
    let app = assemble_with_ports(
        analysis.clone(),
        None,
        ops.clone(),
        Arc::new(crate::session_undo_info::SessionUndoInfoRegistry::default()),
    )?;
    let plan_id = build_approved_trash_plan(&app, &analysis, &[1, 2])?;
    let execute = ok(
        &app.engine,
        "change.execute",
        json!({"requestVersionId":plan_id}),
    )?;
    let job_ref: Id = serde_json::from_value(execute["jobRef"].clone())?;
    gate.wait_until_entered(Duration::from_secs(5))?;
    let cancelled = ok(&app.engine, "job.cancel", json!({"jobRef":job_ref}))?;
    assert_eq!(cancelled["outcome"], "accepted");
    gate.release();
    let terminal = wait_for_job(&app.engine, &execute["jobRef"])?;
    assert_eq!(terminal["state"], "cancelled");
    wait_for_finished_event(&app.events, &job_ref)?;
    let page = ok(
        &app.engine,
        "change.item_results",
        json!({"jobRef":job_ref}),
    )?;
    assert_eq!(page["counts"]["completed"], 1);
    assert_eq!(page["counts"]["unprocessed"], 1);
    assert_eq!(page["items"][1]["reasonKey"], "change.item.cancelled");
    assert_eq!(history_item_count(&app, &job_ref)?, 2);

    let analysis = FakeAnalysis::real().with_trash_facts();
    let ops = fake_change_ops(&[1, 2, 3], CapabilityState::Supported)?;
    ops.enqueue_move_outcome(TrashMoveOutcome::Moved {
        identity_after_move: change_identity(1),
        trash_location: None,
    });
    ops.enqueue_move_outcome(TrashMoveOutcome::Moved {
        identity_after_move: change_identity(2),
        trash_location: None,
    });
    let app = assemble_with_ports(
        analysis.clone(),
        None,
        ops,
        Arc::new(crate::session_undo_info::SessionUndoInfoRegistry::default()),
    )?;
    let database = rusqlite::Connection::open(
        app.profile
            .path()
            .join(du_platform_os::application_data::DATABASE_FILE),
    )?;
    database.execute_batch(
        "CREATE TRIGGER fail_change_item BEFORE INSERT ON job_runtime_item_result
         WHEN NEW.item_seq = 1
         BEGIN SELECT RAISE(ABORT, 'injected item history failure'); END;",
    )?;
    drop(database);
    let plan_id = build_approved_trash_plan(&app, &analysis, &[1, 2, 3])?;
    let execute = ok(
        &app.engine,
        "change.execute",
        json!({"requestVersionId":plan_id}),
    )?;
    let job_ref: Id = serde_json::from_value(execute["jobRef"].clone())?;
    let terminal = wait_for_job(&app.engine, &execute["jobRef"])?;
    assert_eq!(terminal["state"], "partial");
    assert_eq!(terminal["reasonKey"], "history.record_failed");
    wait_for_finished_event(&app.events, &job_ref)?;
    let page = ok(
        &app.engine,
        "change.item_results",
        json!({"jobRef":job_ref}),
    )?;
    assert_eq!(page["itemRecording"], "recorded");
    assert_eq!(page["counts"]["completed"], 2);
    assert_eq!(page["counts"]["unprocessed"], 1);
    assert_eq!(page["items"][2]["state"], "unprocessed");
    assert_eq!(history_item_count(&app, &job_ref)?, 1);
    let history_page = ok(&app.engine, "history.items", json!({"jobRef":job_ref}))?;
    assert_eq!(history_page["itemRecording"], "recorded");
    assert_eq!(history_page["counts"]["recorded"], "1");
    Ok(())
}

fn build_approved_trash_plan(
    app: &PlanTestApp,
    analysis: &FakeAnalysis,
    item_numbers: &[u32],
) -> TestResult<Id> {
    let item_refs = item_numbers
        .iter()
        .map(|item_no| analysis.item_ref(*item_no))
        .collect::<Vec<_>>();
    let opened = ok(
        &app.engine,
        "plan.draft_open",
        json!({"source":"manual","itemRefs":item_refs}),
    )?;
    let build = ok(
        &app.engine,
        "plan.build",
        json!({"draftRef":opened["draftRef"],"editSeq":0}),
    )?;
    let fixed = wait_for_fixed(&app.events, &app.engine, &build["jobRef"])?;
    let plan_id: Id = serde_json::from_value(fixed["planVersionId"].clone())?;
    let review = ok(
        &app.engine,
        "approval.review",
        json!({"requestVersionId":plan_id,"requestKind":"plan"}),
    )?;
    ok(
        &app.engine,
        "approval.grant",
        json!({"requestVersionId":plan_id,"presentedPlanHash":review["planHash"]}),
    )?;
    Ok(plan_id)
}

fn fake_change_ops(
    item_numbers: &[u32],
    capability_state: CapabilityState,
) -> TestResult<Arc<du_platform_os::fake::FakeChangeOps>> {
    let items = item_numbers
        .iter()
        .map(|item_no| {
            let name = format!("file-{item_no}");
            let raw = format!("/fixture/{name}").into_bytes();
            (
                OsPath {
                    encoding: PathEncoding::Unix,
                    raw: PathBytes::from_bytes(&raw),
                    display: String::from_utf8_lossy(&raw).into_owned(),
                    lossy: false,
                },
                ChangeItemObservation {
                    identity: change_identity(*item_no),
                    kind: EntryKind::File,
                    size: Some(ByteSize::from(12)),
                    modified_at: Timestamp::from_unix_millis(123),
                },
            )
        })
        .collect();
    let ops = Arc::new(
        du_platform_os::fake::FakeChangeOps::new(items)
            .map_err(|error| format!("fake adapter setup failed: {error:?}"))?,
    );
    ops.set_capability(
        b"fixture-volume".to_vec(),
        Capability {
            state: capability_state,
            reason_key: None,
        },
    );
    Ok(ops)
}

fn change_identity(item_no: u32) -> ChangeIdentity {
    ChangeIdentity {
        volume_id: b"fixture-volume".to_vec(),
        file_id: format!("fixture-file-{item_no}").into_bytes(),
        change_token: None,
    }
}

fn history_item_count(app: &PlanTestApp, job_ref: &Id) -> TestResult<i64> {
    let database = rusqlite::Connection::open(
        app.profile
            .path()
            .join(du_platform_os::application_data::DATABASE_FILE),
    )?;
    let job_bytes = job_ref.v7_bytes().ok_or("invalid job id")?;
    Ok(database.query_row(
        "SELECT COUNT(*) FROM job_runtime_item_result WHERE job_id=?1",
        [job_bytes.as_slice()],
        |row| row.get(0),
    )?)
}

fn wait_for_finished_event(events: &DesktopEvents, job_ref: &Id) -> TestResult<Value> {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if let Some(json) = events.next_json() {
            let event: Value = serde_json::from_str(&json)?;
            if event["topic"] == "job.finished" && event["jobRef"] == job_ref.as_str() {
                return Ok(event);
            }
        } else {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    Err(format!("job.finished not delivered for {job_ref:?}").into())
}
