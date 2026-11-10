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
    DeduplicationKey, JobIdentity, JobPriority, ResultBinding, RuntimeError, Submission,
    SubmitResult,
};
use du_scan_engine::{scan::ResultIds, scan_job::MetadataScan};

use crate::{
    api::{CallerCtx, Channel, EngineBuilder, UseCase},
    browse,
    error::EngineError,
    scope, space,
    workers::Workers,
};

/// Registers the metadata job and the query sharing its immutable result store.
pub(crate) fn register(
    builder: &mut EngineBuilder,
    workers: Arc<Workers>,
    roots: Arc<dyn ScopeRootPort>,
    ids: Arc<dyn JobIdentity>,
) -> Result<IngestSink, RuntimeError> {
    workers
        .runtime
        .register(du_scan_engine::scan_job::descriptor())?;
    let store = IngestSink::with_version_publisher(workers.runtime.result_version_publisher());
    space::register_total(builder, store.clone());
    browse::register(builder, store.clone());
    builder.register(StartScan {
        roots,
        ids: Arc::new(ScanIds(ids)),
        workers,
        store: store.clone(),
        admitted: Mutex::new(None),
    });
    Ok(store)
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
    workers: Arc<Workers>,
    store: IngestSink,
    admitted: Mutex<Option<Admitted>>,
}

/// Retains the review identity alongside the reusable receipt while a scan is active.
struct Admitted {
    receipt: ScanStart,
    review_fingerprint: String,
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
        let verified = scope::verify(&request.draft, &request.review_fingerprint, &*self.roots)?;
        let scope_version = self.ids.next_id();
        let lineage = self
            .store
            .bind_current_analysis(&verified.scope_fingerprint, self.ids.next_id())
            .map_err(|_| internal())?;
        let root_count = verified.roots.len();
        let body = Arc::new(
            MetadataScan::new_roots(
                verified.roots,
                self.store.clone(),
                scope_version.clone(),
                self.ids.clone(),
            )
            .with_lineage(lineage.clone()),
        );
        let submission = Submission {
            kind_id: body.kind_id().into(),
            deduplication_key: Some(DeduplicationKey {
                scope: verified.scope_fingerprint.clone(),
                input: "metadata-default".into(),
                channel: "gui".into(),
                key: String::new(),
            }),
            body,
            priority: JobPriority::Normal,
            scope_summary: if root_count == 1 {
                "metadata: 1 root".into()
            } else {
                format!("metadata: {root_count} roots")
            },
            correlation_id: caller.correlation_id.clone(),
            sample: false,
        };
        self.admit(
            submission,
            scope_version,
            lineage,
            verified.scope_fingerprint,
            request.review_fingerprint,
        )
    }
}

impl StartScan {
    fn admit(
        &self,
        submission: Submission,
        scope_version: Id,
        lineage: Id,
        scope_fingerprint: String,
        review_fingerprint: String,
    ) -> Result<ScanStart, EngineError> {
        let mut admitted = self.admitted.lock().map_err(|_| internal())?;
        // A stable scope key can outlive a replaced root. Reuse is allowed only
        // if the caller reviewed the same actual roots as the active job.
        if let Some(active) = admitted.as_ref()
            && (active.receipt.scope_fingerprint != scope_fingerprint
                || active.review_fingerprint != review_fingerprint)
            && self.active(&active.receipt.job_ref)?
        {
            return Err(EngineError::reason("job.queue_full"));
        }
        match self
            .workers
            .runtime
            .submit_with_result_binding(
                submission,
                ResultBinding {
                    lineage_id: lineage.clone(),
                    scope_version: scope_version.clone(),
                },
            )
            .map_err(submit_error)?
        {
            SubmitResult::Created(job_ref) => {
                let receipt = ScanStart {
                    job_ref,
                    existing: false,
                    scope_version,
                    scope_fingerprint,
                    lineage_id: Some(lineage),
                };
                *admitted = Some(Admitted {
                    receipt: receipt.clone(),
                    review_fingerprint,
                });
                self.workers.notify();
                Ok(receipt)
            }
            SubmitResult::Existing(job_ref) => {
                let mut receipt = admitted
                    .as_ref()
                    .filter(|active| active.receipt.job_ref == job_ref)
                    .ok_or_else(internal)?
                    .receipt
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
