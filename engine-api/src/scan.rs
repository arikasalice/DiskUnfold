//! Admit a verified scope and submit its read-only metadata job.
use std::sync::{Arc, Mutex};

use du_analysis_index::{IngestSink, query::page::ResultSets};
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

pub use du_core_contracts::settings::SharedTableLimit;

/// Creates in-process scan diagnostics for one submission attempt.
pub type ScanDiagnosticsFactory =
    Arc<dyn Fn() -> Arc<du_scan_engine::scan_job::ScanDiagnostics> + Send + Sync>;

/// Reads an already validated setting snapshot when a new scan is admitted.
pub type SharedTableLimitSource = Arc<dyn Fn() -> SharedTableLimit + Send + Sync>;

/// Registers the metadata job and the query sharing its immutable result store.
pub(crate) fn register_with_limit(
    builder: &mut EngineBuilder,
    workers: Arc<Workers>,
    roots: Arc<dyn ScopeRootPort>,
    ids: Arc<dyn JobIdentity>,
    limit: SharedTableLimitSource,
    diagnostics: Option<ScanDiagnosticsFactory>,
) -> Result<Arc<ResultSets>, RuntimeError> {
    register_with_options(
        builder,
        workers,
        roots,
        ids,
        ScanRegistrationOptions {
            comparison: None,
            limit,
            diagnostics,
            admissions: None,
        },
    )
}

pub(crate) fn register_with_admissions(
    builder: &mut EngineBuilder,
    workers: Arc<Workers>,
    roots: Arc<dyn ScopeRootPort>,
    ids: Arc<dyn JobIdentity>,
    limit: SharedTableLimitSource,
    diagnostics: Option<ScanDiagnosticsFactory>,
    admissions: crate::recent_scope::RecentScopeAdmissionPublisher,
) -> Result<Arc<ResultSets>, RuntimeError> {
    register_with_options(
        builder,
        workers,
        roots,
        ids,
        ScanRegistrationOptions {
            comparison: None,
            limit,
            diagnostics,
            admissions: Some(admissions),
        },
    )
}

#[cfg(test)]
pub(crate) fn register_with_comparison(
    builder: &mut EngineBuilder,
    workers: Arc<Workers>,
    roots: Arc<dyn ScopeRootPort>,
    ids: Arc<dyn JobIdentity>,
    comparison: Option<Arc<dyn scope::exclusion::PathComparison>>,
) -> Result<IngestSink, RuntimeError> {
    register_with_options(
        builder,
        workers,
        roots,
        ids,
        ScanRegistrationOptions {
            comparison,
            limit: Arc::new(SharedTableLimit::default),
            diagnostics: None,
            admissions: None,
        },
    )
    .map(|sets| sets.source())
}

struct ScanRegistrationOptions {
    comparison: Option<Arc<dyn scope::exclusion::PathComparison>>,
    limit: SharedTableLimitSource,
    diagnostics: Option<ScanDiagnosticsFactory>,
    admissions: Option<crate::recent_scope::RecentScopeAdmissionPublisher>,
}

fn register_with_options(
    builder: &mut EngineBuilder,
    workers: Arc<Workers>,
    roots: Arc<dyn ScopeRootPort>,
    ids: Arc<dyn JobIdentity>,
    options: ScanRegistrationOptions,
) -> Result<Arc<ResultSets>, RuntimeError> {
    workers
        .runtime
        .register(du_scan_engine::scan_job::descriptor())?;
    let store = IngestSink::with_version_publisher(workers.runtime.result_version_publisher());
    let sets = Arc::new(ResultSets::new(store.clone()));
    space::register_total_with_queries(builder, sets.clone());
    space::detail::register(builder, store.clone());
    space::volume_gap::register(builder, store.clone());
    browse::register(builder, store.clone());
    builder.register(StartScan {
        roots,
        ids: Arc::new(ScanIds(ids)),
        workers,
        store: store.clone(),
        admitted: Mutex::new(None),
        comparison: options.comparison,
        limit: options.limit,
        diagnostics: options.diagnostics,
        admissions: options.admissions,
    });
    Ok(sets)
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
    comparison: Option<Arc<dyn scope::exclusion::PathComparison>>,
    limit: SharedTableLimitSource,
    diagnostics: Option<ScanDiagnosticsFactory>,
    admissions: Option<crate::recent_scope::RecentScopeAdmissionPublisher>,
}

/// Retains the review identity alongside the reusable receipt while a scan is active.
struct Admitted {
    receipt: ScanStart,
    review_fingerprint: String,
}

struct PendingRecentScope {
    normalized_scope: du_core_contracts::analysis_ipc::ScopeDraft,
    fingerprint_input: Option<du_core_contracts::analysis_ipc::ScopeDraft>,
    observers: Vec<crate::recent_scope::RetainedRoot>,
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
        let verified = scope::verify(
            &request.draft,
            &request.review_fingerprint,
            &*self.roots,
            self.comparison.clone(),
            self.admissions.is_some(),
        )?;
        let recent = self.admissions.as_ref().and_then(|_| {
            Some(PendingRecentScope {
                normalized_scope: verified.normalized_draft,
                fingerprint_input: verified.scope_fingerprint.retained_comparison_input(),
                observers: crate::recent_scope::RetainedRoot::bind_all(
                    &verified.roots,
                    verified.recent_observers,
                )?,
            })
        });
        let scope_fingerprint = verified.scope_fingerprint.resolve()?;
        let scope_version = self.ids.next_id();
        let lineage = self
            .store
            .bind_current_analysis(&scope_fingerprint, self.ids.next_id())
            .map_err(|_| internal())?;
        let root_count = verified.roots.len();
        let mut job = MetadataScan::new_roots(
            verified.roots,
            self.store.clone(),
            scope_version.clone(),
            self.ids.clone(),
        )
        .with_lineage(lineage.clone())
        .with_shared_table_limit((self.limit)().entries());
        if let Some(factory) = &self.diagnostics {
            job = job.with_diagnostics(factory());
        }
        if let Some(filter) = verified.filter {
            job = job.with_pre_metadata_filter(filter);
        }
        let body = Arc::new(job);
        let submission = Submission {
            kind_id: body.kind_id().into(),
            deduplication_key: Some(DeduplicationKey {
                scope: scope_fingerprint.clone(),
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
            scope_fingerprint,
            request.review_fingerprint,
            recent,
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
        recent: Option<PendingRecentScope>,
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
                if let Some(publisher) = &self.admissions {
                    let recent = recent.ok_or_else(internal)?;
                    publisher.enqueue(crate::recent_scope::RecentScopeAdmission {
                        receipt: receipt.clone(),
                        normalized_scope: recent.normalized_scope,
                        fingerprint_input: recent.fingerprint_input,
                        observers: recent.observers,
                        runtime: self.workers.runtime.clone(),
                        queue_slots: None,
                    });
                }
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
