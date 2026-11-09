use super::*;
use du_analysis_index::{IngestSink, Observation};
use du_core_contracts::{
    ByteSize, SizeAmount, SizeBasis, SizeEvidence, SizeValue, result::ResultCompleteness,
};
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

    assert_eq!(runtime.run_next()?, Some(job.clone()));
    let finished = command("job.status", &job)?.body;
    assert_eq!(finished["state"], "done");
    assert_eq!(finished["resultVersion"], json!(result));
    let cancel = command("job.cancel", &job)?.body;
    assert_eq!(cancel["outcome"], "already_terminal");
    assert_eq!(cancel["state"], finished["state"]);
    assert_eq!(cancel["sequence"], finished["sequence"]);
    Ok(())
}

#[test]
fn space_total_honors_admission_version_and_preserves_unknown_basis()
-> Result<(), Box<dyn std::error::Error>> {
    let scope = Id::parse("0195f856-ab83-7a59-9343-10d29775c9a9")?;
    let version = Id::parse("0195f856-ab83-7a59-9343-10d29775c9aa")?;
    let store = IngestSink::default();
    let mut builder = EngineBuilder::default();
    crate::space::register_total(&mut builder, store.clone());
    let engine = builder.build()?;
    let read = |result: &Id| {
        call(
            &engine,
            "space.total",
            Channel::Gui,
            ENVELOPE_VERSION,
            json!({
                "resultVersion": result,
                "target": {"kind": "scope_root"},
                "requestSeq": 9
            }),
        )
    };
    assert_eq!(
        read(&version)?.error.ok_or("missing error")?.reason_key,
        "ref.unavailable"
    );

    let mut ingest = store.open(scope.clone()).map_err(|e| format!("{e:?}"))?;
    ingest
        .add_structure(Vec::new())
        .map_err(|e| format!("{e:?}"))?;
    ingest
        .add(Observation {
            path: vec![b"file".to_vec()],
            size: Some(SizeValue {
                basis: SizeBasis::Logical,
                evidence: SizeEvidence::OsReported,
                amount: SizeAmount::Exact {
                    bytes: ByteSize::from(47),
                },
            }),
        })
        .map_err(|e| format!("{e:?}"))?;
    ingest
        .seal(version.clone(), ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;

    let response = read(&version)?;
    assert!(response.error.is_none());
    assert_eq!(response.body["resultVersion"], json!(version));
    assert_eq!(response.body["requestSeq"], 9);
    assert_eq!(response.body["sizes"][0]["size"]["bytes"], "47");
    assert_eq!(response.body["sizes"][1]["size"]["state"], "unknown");
    assert_eq!(response.body["sizes"][2]["size"]["state"], "unknown");
    assert_eq!(
        read(&scope)?.error.ok_or("missing stale error")?.reason_key,
        "result.requery_required"
    );
    Ok(())
}
