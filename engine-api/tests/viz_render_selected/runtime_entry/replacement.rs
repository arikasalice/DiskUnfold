use super::*;
use du_core_contracts::job::{
    HistoryMode, JobBody, JobContext, JobDescriptor, JobOutcome, ProgressUnit, ResourceClass,
};
use du_job_runtime::{JobPriority, RuntimeError, Submission};

#[test]
fn ledger_capacity_failure_preserves_current_running_subscription() -> Result<(), String> {
    let fixture = Fixture::with_policy([25, 75], Duration::ZERO, NonZeroUsize::MIN)?;
    let view = fixture.open()?;
    let original = job(&fixture.render(&view, 1)?)?;
    fixture
        .entered
        .recv_timeout(Duration::from_secs(3))
        .map_err(|e| e.to_string())?;
    assert_busy_replacement(&fixture, &view)?;
    finish_original(&fixture, &view, &original)
}

#[test]
fn f24_queue_failure_preserves_current_running_subscription() -> Result<(), String> {
    let fixture = Fixture::new([25, 75])?;
    let view = fixture.open()?;
    let original = job(&fixture.render(&view, 1)?)?;
    fixture
        .entered
        .recv_timeout(Duration::from_secs(3))
        .map_err(|e| e.to_string())?;
    fill_f24_queue(&fixture.jobs)?;
    assert_busy_replacement(&fixture, &view)?;
    finish_original(&fixture, &view, &original)
}

#[test]
fn cancelling_key_failure_preserves_completed_subscription() -> Result<(), String> {
    let fixture = Fixture::new([25, 75])?;
    let cancelled_view = fixture.open()?;
    let cancelling = job(&fixture.render(&cancelled_view, 1)?)?;
    fixture
        .entered
        .recv_timeout(Duration::from_secs(3))
        .map_err(|e| e.to_string())?;
    let view = fixture.open()?;
    let mut prior = render_body(&view, 1);
    prior["labelMode"] = json!("name_only");
    let accepted = fixture.ok("viz.render", prior)?;
    if accepted["kind"] == "job" {
        fixture.finished(&job(&accepted)?, JobState::Done)?;
    } else {
        assert_eq!(accepted["kind"], "model");
    }
    assert_eq!(
        fixture.detach(&cancelled_view, 1)?["controlResult"],
        "accepted"
    );
    let error = fixture
        .call("viz.render", render_body(&view, 2))
        .error
        .ok_or("joined cancelling job")?;
    assert_eq!(error.reason_key, "viz.busy");
    assert_eq!(
        fixture.detach(&view, 1)?["controlResult"],
        "already_terminal"
    );
    fixture.release.send(()).map_err(|e| e.to_string())?;
    fixture.finished(&cancelling, JobState::Cancelled)
}

#[test]
fn late_subscription_cancel_preserves_done_delivery_until_close() -> Result<(), String> {
    let fixture = Fixture::new([25, 75])?;
    let view = fixture.open()?;
    let physical = job(&fixture.render(&view, 1)?)?;
    fixture
        .entered
        .recv_timeout(Duration::from_secs(3))
        .map_err(|e| e.to_string())?;
    fixture.release.send(()).map_err(|e| e.to_string())?;
    fixture.finished(&physical, JobState::Done)?;
    for _ in 0..2 {
        assert_eq!(
            fixture.detach(&view, 1)?["controlResult"],
            "already_terminal"
        );
    }
    let model = fixture.render(&view, 2)?;
    assert_eq!(model["kind"], "model");
    assert_eq!(
        fixture.detach(&view, 2)?["controlResult"],
        "already_terminal"
    );
    fixture.ok("viz.view.close", json!({"viewChannel": view}))?;
    let error = fixture
        .call(
            "viz.cancel",
            json!({"target":{"kind":"subscription","viewChannel":view,"requestSeq":2}}),
        )
        .error
        .ok_or("closed view accepted")?;
    assert_eq!(error.reason_key, "ref.unavailable");
    Ok(())
}

fn assert_busy_replacement(fixture: &Fixture, view: &Value) -> Result<(), String> {
    let mut request = render_body(view, 2);
    request["labelMode"] = json!("name_only");
    let error = fixture
        .call("viz.render", request)
        .error
        .ok_or("replacement accepted")?;
    assert_eq!(error.reason_key, "viz.busy");
    Ok(())
}

fn finish_original(fixture: &Fixture, view: &Value, original: &Id) -> Result<(), String> {
    assert_eq!(
        fixture.jobs.state(original).map_err(|e| e.to_string())?,
        JobState::Running
    );
    fixture.release.send(()).map_err(|e| e.to_string())?;
    fixture.finished(original, JobState::Done)?;
    assert_eq!(
        fixture.detach(view, 1)?["controlResult"],
        "already_terminal"
    );
    Ok(())
}

struct QueueFiller;
impl JobBody for QueueFiller {
    fn kind_id(&self) -> &str {
        "test.queue_filler"
    }
    fn run(&self, _: Arc<dyn JobContext>) -> JobOutcome {
        JobOutcome {
            counts: Default::default(),
            completed_scope: true,
            failure_reason_key: None,
            unprocessed_summary: None,
            result_version_id: None,
        }
    }
}

fn fill_f24_queue(jobs: &JobRuntime) -> Result<(), String> {
    jobs.register(JobDescriptor {
        kind_id: "test.queue_filler".into(),
        version: 1,
        display_name_key: "test.queue_filler".into(),
        controls: vec![],
        safe_points: vec![],
        progress_unit: ProgressUnit::Items,
        resource_classes: vec![ResourceClass::Compute],
        history: HistoryMode::None,
    })
    .map_err(|error| error.to_string())?;
    for _ in 0..128 {
        let result = jobs.submit(Submission {
            kind_id: "test.queue_filler".into(),
            deduplication_key: None,
            body: Arc::new(QueueFiller),
            priority: JobPriority::Normal,
            scope_summary: "queue fixture".into(),
            correlation_id: id(50),
            sample: false,
        });
        match result {
            Ok(_) => (),
            Err(RuntimeError::QueueFull | RuntimeError::DeliveryBusy) => return Ok(()),
            Err(error) => return Err(error.to_string()),
        }
    }
    Err("F24 queue never refused work".into())
}
