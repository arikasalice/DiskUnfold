//! F09 terminal-gated duplicate verification producer.
//!
//! One sequential flow reads only the injected targets. The application must
//! supply an identity-bound reader, pinned scope evidence and a session store.
use crate::{
    RegisteredHash,
    candidates::FileIdentity,
    fingerprint_cache::{FingerprintCache, ReuseCapabilities, ReuseInputs},
    result_store::{PreparedResult, ResultStore, ScopeEvidence},
    verification::{OpenFile, ReadError, ReadPort, State, VerificationTarget, VerifyError},
};
use du_core_contracts::{
    Id,
    job::{
        Checkpoint, HistoryMode, JobBody, JobContext, JobControl, JobCounts, JobDescriptor,
        JobFlowError, JobOutcome, ProgressUnit, ResourceClass,
    },
};
use du_job_runtime::DuplicateTerminal;
use std::sync::{Arc, Mutex};

/// The published duplicate.check job descriptor, revision 1.
pub fn descriptor() -> JobDescriptor {
    JobDescriptor {
        kind_id: "duplicate.check".into(),
        version: 1,
        display_name_key: "duplicate.check".into(),
        controls: vec![JobControl::Pause, JobControl::Resume, JobControl::Cancel],
        safe_points: vec!["file".into()],
        progress_unit: ProgressUnit::Bytes,
        resource_classes: vec![ResourceClass::ContentIo],
        history: HistoryMode::Summary,
    }
}

/// Inputs selected from one pinned source; `reader` must enforce the selected set.
pub struct Input<P> {
    /// Pinned scan result identity.
    pub source: Id,
    /// New duplicate result identity, supplied by the composition point.
    pub result: Id,
    /// Scope and source observation evidence.
    pub scope: ScopeEvidence,
    /// Approved target observations.
    pub targets: Vec<VerificationTarget>,
    /// Identity-bound reader scoped to these targets and source.
    pub reader: P,
}

/// Input validation or candidate preparation failed before submission.
#[derive(Debug)]
pub enum PrepareError {
    /// Scope references do not belong to the selected source or roots are empty.
    InvalidScope,
    /// The injected target set or selected strategy is invalid.
    Verification(VerifyError),
    /// Target count cannot be represented in the job summary.
    TooManyTargets,
}

struct Candidates {
    queued: Option<PreparedResult>,
    running: Option<PreparedResult>,
}
struct Body<P> {
    input: Mutex<Option<(Vec<VerificationTarget>, P)>>,
    hash: RegisteredHash,
    scope_proven: bool,
    scope: ScopeEvidence,
    candidates: Arc<Mutex<Candidates>>,
    reuse: Option<JobReuse>,
}

/// Explicit session-scoped sources supplied by the composition point.
pub type CapabilitySource = dyn Fn(&FileIdentity) -> Option<ReuseCapabilities> + Send + Sync;
/// UTC milliseconds when available.
pub type VerificationClock = dyn Fn() -> Option<i64> + Send + Sync;

/// Explicit session-scoped sources supplied by the composition point.
pub struct JobReuse {
    pub cache: Arc<FingerprintCache>,
    pub capabilities: Arc<CapabilitySource>,
    pub clock: Arc<VerificationClock>,
}

/// Prepared job components for `submit_duplicate_terminal`.
pub struct PreparedJob {
    /// Job body to put into the runtime submission.
    pub body: Arc<dyn JobBody>,
    /// Conservative precomputed outcome for a queued cancellation.
    pub queued: JobOutcome,
    /// One-shot bounded in-memory publication callback for the runtime gate.
    pub commit: Box<dyn FnOnce(DuplicateTerminal) -> Result<Id, String> + Send>,
}

/// Prepares the queued candidate without opening content and binds one job body.
///
/// The caller must keep the source pinned and ensure the injected reader cannot
/// open outside the selected set. The store must already contain the live source.
/// No IPC registration or product source admission is performed here.
///
/// # Errors
/// Returns `PrepareError` before any reads when scope or target evidence is invalid.
pub fn prepare<P: ReadPort + Send + 'static>(
    input: Input<P>,
    hash: RegisteredHash,
    store: Arc<ResultStore>,
) -> Result<PreparedJob, PrepareError> {
    prepare_inner(input, hash, store, None)
}

/// Prepares a job with an explicitly supplied, session-scoped reuse source.
/// # Errors
/// Returns an error if scope or verification inputs are invalid.
pub fn prepare_with_reuse<P: ReadPort + Send + 'static>(
    input: Input<P>,
    hash: RegisteredHash,
    store: Arc<ResultStore>,
    reuse: JobReuse,
) -> Result<PreparedJob, PrepareError> {
    prepare_inner(input, hash, store, Some(reuse))
}

fn prepare_inner<P: ReadPort + Send + 'static>(
    input: Input<P>,
    hash: RegisteredHash,
    store: Arc<ResultStore>,
    reuse: Option<JobReuse>,
) -> Result<PreparedJob, PrepareError> {
    if input.scope.roots.is_empty()
        || input
            .scope
            .roots
            .iter()
            .chain(&input.scope.exclusions)
            .any(|item| item.result_version != input.source)
    {
        return Err(PrepareError::InvalidScope);
    }
    let count = u64::try_from(input.targets.len()).map_err(|_| PrepareError::TooManyTargets)?;
    let scope_proven = input.scope.source_completeness
        == du_core_contracts::result::ResultCompleteness::Complete
        && input.scope.unavailable == 0;
    let queued = hash
        .verify_for_scope(&input.targets, &mut NoReads, scope_proven, || true)
        .map_err(PrepareError::Verification)?;
    let candidates = Arc::new(Mutex::new(Candidates {
        queued: Some(PreparedResult::new(queued, input.scope.clone())),
        running: None,
    }));
    let body = Arc::new(Body {
        input: Mutex::new(Some((input.targets, input.reader))),
        hash,
        scope_proven,
        scope: input.scope,
        candidates: Arc::clone(&candidates),
        reuse,
    });
    let summary = summary(count);
    let queued = JobOutcome {
        counts: JobCounts {
            unprocessed: count,
            ..Default::default()
        },
        completed_scope: false,
        failure_reason_key: None,
        unprocessed_summary: Some(summary),
        result_version_id: None,
    };
    let commit = Box::new(move |terminal| {
        let result = {
            let mut state = candidates
                .lock()
                .map_err(|_| "support.internal_error".to_owned())?;
            match terminal {
                DuplicateTerminal::Completed => state.running.take(),
                DuplicateTerminal::Cancelled => {
                    state.running.take().or_else(|| state.queued.take())
                }
            }
            .ok_or_else(|| "support.internal_error".to_owned())?
        };
        store
            .publish_prepared(
                &input.source,
                input.result.clone(),
                result,
                terminal == DuplicateTerminal::Cancelled,
            )
            .map_err(|_| "support.internal_error".to_owned())?;
        Ok(input.result)
    });
    Ok(PreparedJob {
        body,
        queued,
        commit,
    })
}

struct NoReads;
impl ReadPort for NoReads {
    fn open_bound<'a>(
        &'a mut self,
        _: u32,
        _: &FileIdentity,
    ) -> Result<Box<dyn OpenFile + 'a>, ReadError> {
        Err(ReadError::Failed)
    }
}

impl<P: ReadPort + Send> JobBody for Body<P> {
    fn kind_id(&self) -> &str {
        "duplicate.check"
    }
    fn run(&self, context: Arc<dyn JobContext>) -> JobOutcome {
        let Some((targets, mut reader)) = self.input.lock().ok().and_then(|mut value| value.take())
        else {
            return failure();
        };
        let flow = if targets.is_empty() {
            None
        } else {
            match context.wait_start_flow() {
                Ok(flow) => Some(flow),
                Err(JobFlowError::Cancelled) => None,
                Err(_) => return failure(),
            }
        };
        let result = self.verify(&targets, &mut reader, || match flow.as_ref() {
            Some(flow) => flow.safe_point() == Checkpoint::Cancel,
            None => context.checkpoint() == Checkpoint::Cancel,
        });
        if let Some(flow) = flow {
            flow.finish(JobCounts::default());
        }
        let Ok(result) = result else {
            return failure();
        };
        let cancelled = result.evidence().cancelled;
        let counts = counts(result.evidence());
        // Traversal and content certainty are independent.
        let complete = !cancelled;
        // The terminal gate can observe cancellation after verification returns.
        // Keep an explicit zero summary available for that race as well.
        let unprocessed_summary = Some(summary(counts.unprocessed));
        let prepared = PreparedResult::new(result, self.scope.clone());
        let obsolete = {
            let Ok(mut state) = self.candidates.lock() else {
                return failure();
            };
            state.running = Some(prepared);
            state.queued.take()
        };
        drop(obsolete);
        JobOutcome {
            counts,
            completed_scope: complete,
            failure_reason_key: None,
            unprocessed_summary,
            result_version_id: None,
        }
    }
}

impl<P: ReadPort + Send> Body<P> {
    fn verify(
        &self,
        targets: &[VerificationTarget],
        reader: &mut P,
        cancelled: impl FnMut() -> bool,
    ) -> Result<crate::RegisteredOutcome, VerifyError> {
        if let Some(reuse) = &self.reuse {
            let inputs = ReuseInputs {
                capabilities: reuse.capabilities.as_ref(),
                clock: reuse.clock.as_ref(),
                scope_proven: self.scope_proven,
            };
            reuse
                .cache
                .verify_with_reuse(targets, reader, &self.hash, &inputs, cancelled)
        } else {
            self.hash
                .verify_for_scope(targets, reader, self.scope_proven, cancelled)
        }
    }
}
fn counts(outcome: &crate::verification::Outcome) -> JobCounts {
    let mut counts = JobCounts::default();
    for state in outcome.states.values() {
        match state {
            State::Verified | State::Distinct => counts.completed += 1,
            State::Changed | State::ReadFailed => counts.failed += 1,
            State::Unchecked | State::PlaceholderDeferred => counts.skipped += 1,
            // No final content disposition, even if an attempted read completed.
            State::Candidate | State::SampleCandidate => counts.unprocessed += 1,
        }
    }
    counts
}
fn summary(count: u64) -> String {
    format!("{count} selected items lack final disposition")
}
fn failure() -> JobOutcome {
    JobOutcome {
        counts: JobCounts::default(),
        completed_scope: false,
        failure_reason_key: Some("support.internal_error".into()),
        unprocessed_summary: None,
        result_version_id: None,
    }
}
