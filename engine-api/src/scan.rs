//! Admit a verified scope and submit its read-only metadata job.
use std::sync::{Arc, Mutex};

use du_analysis_index::IngestSink;
use du_core_contracts::{
    Id,
    analysis_ipc::{ScanStart, ScanStartRequest},
    job::JobBody,
    scan_observation::ScopeRootPort,
};
use du_job_runtime::{
    DeduplicationKey, JobIdentity, JobPriority, JobRuntime, RuntimeError, Submission, SubmitResult,
};
use du_scan_engine::{scan::ResultIds, scan_job::MetadataScan};

use crate::{
    api::{CallerCtx, Channel, EngineBuilder, UseCase},
    error::EngineError,
    scope, space,
    workers::Workers,
};

/// Registers the metadata job and the query sharing its immutable result store.
pub fn register(
    builder: &mut EngineBuilder,
    runtime: Arc<JobRuntime>,
    roots: Arc<dyn ScopeRootPort>,
    ids: Arc<dyn JobIdentity>,
) -> Result<(), Box<dyn std::error::Error>> {
    runtime.register(du_scan_engine::scan_job::descriptor())?;
    let workers = Workers::start(runtime)?;
    let store = IngestSink::default();
    space::register_total(builder, store.clone());
    builder.register(StartScan {
        roots,
        ids: Arc::new(ScanIds(ids)),
        workers,
        store,
        admitted: Mutex::new(None),
    });
    Ok(())
}

struct ScanIds(Arc<dyn JobIdentity>);

impl ResultIds for ScanIds {
    fn next_id(&self) -> Id {
        self.0.next_id()
    }
}

struct StartScan {
    roots: Arc<dyn ScopeRootPort>,
    ids: Arc<ScanIds>,
    workers: Workers,
    store: IngestSink,
    admitted: Mutex<Option<ScanStart>>,
}

impl UseCase for StartScan {
    const NAME: &'static str = "scan.start";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = ScanStartRequest;
    type Response = ScanStart;

    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        // Execution opens and verifies the exact handle retained for collection.
        Ok(())
    }

    fn execute(
        &self,
        request: Self::Request,
        caller: &CallerCtx,
    ) -> Result<Self::Response, EngineError> {
        let root = scope::verify(&request.draft, &request.review_fingerprint, &*self.roots)?;
        let scope_version = self.ids.next_id();
        let body = Arc::new(MetadataScan::new(
            root.directory,
            self.store.clone(),
            scope_version.clone(),
            self.ids.clone(),
        ));
        let submission = Submission {
            kind_id: body.kind_id().into(),
            deduplication_key: Some(DeduplicationKey {
                scope: request.review_fingerprint.clone(),
                input: "metadata-default".into(),
                channel: "gui".into(),
                key: String::new(),
            }),
            body,
            priority: JobPriority::Normal,
            scope_summary: root.canonical_path.display,
            correlation_id: caller.correlation_id.clone(),
            sample: false,
        };
        self.admit(submission, scope_version, request.review_fingerprint)
    }
}

impl StartScan {
    fn admit(
        &self,
        submission: Submission,
        scope_version: Id,
        scope_fingerprint: String,
    ) -> Result<ScanStart, EngineError> {
        let mut admitted = self.admitted.lock().map_err(|_| internal())?;
        // The current store permits one active builder. A retry for the same
        // fingerprint is admitted so the runtime can return its existing job.
        if let Some(active) = admitted.as_ref()
            && active.scope_fingerprint != scope_fingerprint
            && self.active(&active.job_ref)?
        {
            return Err(EngineError::reason("job.queue_full"));
        }
        match self
            .workers
            .runtime
            .submit(submission)
            .map_err(submit_error)?
        {
            SubmitResult::Created(job_ref) => {
                let receipt = ScanStart {
                    job_ref,
                    existing: false,
                    scope_version,
                    scope_fingerprint,
                };
                *admitted = Some(receipt.clone());
                self.workers.notify();
                Ok(receipt)
            }
            SubmitResult::Existing(job_ref) => {
                let mut receipt = admitted
                    .as_ref()
                    .filter(|receipt| receipt.job_ref == job_ref)
                    .ok_or_else(internal)?
                    .clone();
                receipt.existing = true;
                Ok(receipt)
            }
        }
    }

    fn active(&self, job_ref: &Id) -> Result<bool, EngineError> {
        match self.workers.runtime.state(job_ref) {
            Ok(state) => Ok(!state.is_terminal()),
            Err(RuntimeError::NotFound) => Ok(false),
            Err(_) => Err(internal()),
        }
    }
}

fn submit_error(error: RuntimeError) -> EngineError {
    match error {
        RuntimeError::QueueFull | RuntimeError::DeliveryBusy => {
            EngineError::reason("job.queue_full")
        }
        RuntimeError::ShuttingDown => EngineError::reason("job.shutting_down"),
        _ => internal(),
    }
}

fn internal() -> EngineError {
    EngineError::reason("support.internal_error")
}
