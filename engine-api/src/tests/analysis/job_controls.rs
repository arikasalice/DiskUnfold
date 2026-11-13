use super::*;
use du_job_runtime::{JobClock, JobEvents, JobIdentity, JobRuntime, Submission};
use std::sync::Arc;

struct JobFixture(Id);

impl JobIdentity for JobFixture {
    fn next_id(&self) -> Id {
        self.0.clone()
    }
}

impl JobClock for JobFixture {
    fn now_ms(&self) -> i64 {
        1
    }
}

impl JobEvents for JobFixture {
    fn publish(&self, _: du_job_runtime::JobEvent) {}
}

impl du_core_contracts::job::JobHistory for JobFixture {
    fn append(
        &self,
        _: &du_core_contracts::job::JobSummary,
    ) -> Result<du_core_contracts::job::HistoryAppend, String> {
        Err("test fixture has no history".into())
    }
}

impl du_core_contracts::job::JobBody for JobFixture {
    fn kind_id(&self) -> &str {
        "scan.metadata"
    }

    fn run(
        &self,
        _: Arc<dyn du_core_contracts::job::JobContext>,
    ) -> du_core_contracts::job::JobOutcome {
        du_core_contracts::job::JobOutcome {
            counts: Default::default(),
            completed_scope: true,
            failure_reason_key: None,
            unprocessed_summary: None,
            result_version_id: Some(self.0.clone()),
        }
    }
}

#[test]
fn job_commands_report_the_committed_state_and_reject_unknown_references()
-> Result<(), Box<dyn std::error::Error>> {
    use du_core_contracts::job::{HistoryMode, JobControl, JobDescriptor, ProgressUnit};
    let job = Id::parse("0195f856-ab83-7a59-9343-10d29775c9a9")?;
    let result = Id::parse("0195f856-ab83-7a59-9343-10d29775c9aa")?;
    let fixture = Arc::new(JobFixture(job.clone()));
    let runtime = Arc::new(JobRuntime::new(
        fixture.clone(),
        fixture.clone(),
        fixture.clone(),
        fixture,
    )?);
    runtime.register(JobDescriptor {
        kind_id: "scan.metadata".into(),
        version: 1,
        display_name_key: "scan.metadata".into(),
        controls: vec![JobControl::Cancel],
        safe_points: vec!["item".into()],
        progress_unit: ProgressUnit::Items,
        resource_classes: vec![],
        history: HistoryMode::None,
    })?;
    let mut builder = EngineBuilder::default();
    crate::jobs::register(&mut builder, runtime.clone());
    let engine = builder.build()?;
    let command = |name: &str, id: &Id| {
        call(
            &engine,
            name,
            Channel::Gui,
            ENVELOPE_VERSION,
            json!({ "jobRef": id }),
        )
    };

    assert_eq!(
        command("job.status", &job)?
            .error
            .ok_or("missing error")?
            .reason_key,
        "job.not_found"
    );
    runtime.submit(Submission {
        kind_id: "scan.metadata".into(),
        deduplication_key: None,
        body: Arc::new(JobFixture(result.clone())),
        priority: du_job_runtime::JobPriority::Normal,
        scope_summary: "fixture".into(),
        correlation_id: job.clone(),
        sample: false,
    })?;
    let queued = command("job.status", &job)?.body;
    assert_eq!(queued["state"], "queued");
    assert_eq!(queued["sequence"], 1);
    assert!(queued["resultVersion"].is_null());
    assert!(queued.get("scanResult").is_none());

    verify_suspension_rejections(&engine, &job, &result)?;

    assert_eq!(runtime.run_next()?, Some(job.clone()));
    let finished = command("job.status", &job)?.body;
    assert_eq!(finished["state"], "done");
    assert_eq!(finished["resultVersion"], json!(result));
    for name in ["job.pause", "job.resume", "job.cancel"] {
        let control = command(name, &job)?.body;
        assert_eq!(control["outcome"], "already_terminal");
        assert_eq!(control["state"], finished["state"]);
        assert_eq!(control["sequence"], finished["sequence"]);
    }
    Ok(())
}

fn verify_suspension_rejections(
    engine: &Engine,
    job: &Id,
    unknown: &Id,
) -> Result<(), Box<dyn std::error::Error>> {
    for name in ["job.pause", "job.resume"] {
        let unsupported = call(
            engine,
            name,
            Channel::Gui,
            ENVELOPE_VERSION,
            json!({"jobRef": job}),
        )?
        .body;
        assert_eq!(unsupported["outcome"], "unsupported");
        assert_eq!(unsupported["reasonKey"], "job.control_unsupported");
        assert_eq!(unsupported["sequence"], 1);
        let invalid = call(
            engine,
            name,
            Channel::Gui,
            ENVELOPE_VERSION,
            json!({"jobRef": job, "extra": true}),
        )?
        .error
        .ok_or("invalid job control accepted")?;
        assert_eq!(invalid.reason_key, "command.body_invalid");
        for channel in [Channel::Cli, Channel::Mcp] {
            let denied = call(
                engine,
                name,
                channel,
                ENVELOPE_VERSION,
                json!({"jobRef": job}),
            )?
            .error
            .ok_or("non-GUI job control accepted")?;
            assert_eq!(denied.reason_key, "command.not_provided");
        }
        let missing = call(
            engine,
            name,
            Channel::Gui,
            ENVELOPE_VERSION,
            json!({"jobRef": unknown}),
        )?
        .error
        .ok_or("unknown job accepted")?;
        assert_eq!(missing.reason_key, "job.not_found");
    }

    Ok(())
}
