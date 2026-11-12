//! Persist recent scopes after the WebView confirms successful receipt delivery.
use std::{
    collections::HashMap,
    io,
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, Sender, TryRecvError, channel},
    },
    thread::{self, Thread},
};

use du_app_db::recent_scope::{RecentError, RecentWrite};
use du_core_contracts::{
    CallEnvelope, ENVELOPE_VERSION, Id, ResultEnvelope, analysis_ipc::ScanStart, ports::Random,
};
use du_engine_api::desktop::{RecentScopeAdmission, RecentScopeAdmissions, RecentScopeDelivery};
use du_platform_os::{
    entropy::OsRandom,
    webview::{ResponseDelivery, ResponseDeliveryObserver},
};

use crate::{
    recent_scope_body::{BodyError, EncodedRecentScope, RecentScopeBody},
    shared_database::SharedDatabase,
};

type RecentScopeEncoder = dyn Fn(&RecentScopeAdmission, &AtomicBool) -> Result<EncodedRecentScope, BodyError>
    + Send
    + Sync;

/// A value-free warning about one accepted scan's recent-scope storage attempt.
///
/// The observer has no trusted window-generation input. Consumers must not expose
/// this callback as a GUI event until they can bind it to the owning window session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecentScopeWriteWarning {
    InvalidEvidence { job_ref: Id, correlation_id: Id },
    Busy { job_ref: Id, correlation_id: Id },
    DiskFull { job_ref: Id, correlation_id: Id },
    StorageUnavailable { job_ref: Id, correlation_id: Id },
}

impl RecentScopeWriteWarning {
    /// Returns the safe, value-free reason key for a future GUI relay.
    pub fn reason_key(&self) -> &'static str {
        match self {
            Self::InvalidEvidence { .. } | Self::StorageUnavailable { .. } => {
                "scope.recent_store_failed"
            }
            Self::Busy { .. } => "db.busy",
            Self::DiskFull { .. } => "db.disk_full",
        }
    }

    /// Returns the accepted scan job associated with this storage attempt.
    pub fn job_ref(&self) -> &Id {
        match self {
            Self::InvalidEvidence { job_ref, .. }
            | Self::Busy { job_ref, .. }
            | Self::DiskFull { job_ref, .. }
            | Self::StorageUnavailable { job_ref, .. } => job_ref,
        }
    }

    /// Returns the correlation ID from the actual GUI request envelope.
    pub fn correlation_id(&self) -> &Id {
        match self {
            Self::InvalidEvidence { correlation_id, .. }
            | Self::Busy { correlation_id, .. }
            | Self::DiskFull { correlation_id, .. }
            | Self::StorageUnavailable { correlation_id, .. } => correlation_id,
        }
    }
}

struct AttemptContext {
    job_ref: Id,
    correlation_id: Id,
}

/// Owns the recent-scope consumer and stops its background work when dropped.
pub(crate) struct RecentScopeWorker {
    delivery: RecentScopeDelivery,
    attempt_context_sender: Sender<AttemptContext>,
    stopped: Arc<AtomicBool>,
    thread: Thread,
}

impl RecentScopeWorker {
    /// Starts one consumer with the composition's storage cap and warning sink.
    pub(crate) fn start(
        admissions: RecentScopeAdmissions,
        database: SharedDatabase,
        cap: NonZeroUsize,
        warning: impl Fn(RecentScopeWriteWarning) + Send + 'static,
    ) -> io::Result<Self> {
        Self::start_inner(
            admissions,
            database,
            cap,
            Arc::new(RecentScopeBody::encode_admission),
            warning,
            || {},
        )
    }

    #[cfg(test)]
    pub(crate) fn start_for_test(
        admissions: RecentScopeAdmissions,
        database: SharedDatabase,
        cap: NonZeroUsize,
        encoder: impl Fn(&RecentScopeAdmission, &AtomicBool) -> Result<EncodedRecentScope, BodyError>
        + Send
        + Sync
        + 'static,
        warning: impl Fn(RecentScopeWriteWarning) + Send + 'static,
        completed: impl Fn() + Send + 'static,
    ) -> io::Result<Self> {
        Self::start_inner(
            admissions,
            database,
            cap,
            Arc::new(encoder),
            warning,
            completed,
        )
    }

    fn start_inner(
        admissions: RecentScopeAdmissions,
        database: SharedDatabase,
        cap: NonZeroUsize,
        encoder: Arc<RecentScopeEncoder>,
        warning: impl Fn(RecentScopeWriteWarning) + Send + 'static,
        completed: impl Fn() + Send + 'static,
    ) -> io::Result<Self> {
        let delivery = admissions.delivery();
        // One context belongs to a queued admission or the single candidate
        // currently being consumed; an unbounded handoff avoids rejecting a
        // successfully posted reply when the worker is briefly behind.
        let (attempt_context_sender, attempt_context_receiver) = channel();
        let stopped = Arc::new(AtomicBool::new(false));
        let stop = stopped.clone();
        let worker = thread::Builder::new()
            .name("diskunfold-recent-scope".into())
            .spawn(move || {
                let mut attempt_contexts = HashMap::new();
                while !stop.load(Ordering::Acquire) {
                    collect_attempt_contexts(&attempt_context_receiver, &mut attempt_contexts);
                    match admissions.try_next() {
                        Ok(candidate) => {
                            // The observer queues this context before marking the ticket posted.
                            collect_attempt_contexts(
                                &attempt_context_receiver,
                                &mut attempt_contexts,
                            );
                            let job_ref = candidate.receipt().job_ref.clone();
                            if let Some(correlation_id) = attempt_contexts.remove(&job_ref)
                                && let Err(failure) =
                                    persist(&database, cap, &candidate, &stop, &*encoder)
                                && !stop.load(Ordering::Acquire)
                            {
                                warning(failure.with_context(job_ref, correlation_id));
                            }
                            completed();
                        }
                        Err(TryRecvError::Empty) => thread::park(),
                        Err(TryRecvError::Disconnected) => break,
                    }
                }
            })?;
        Ok(Self {
            delivery,
            attempt_context_sender,
            stopped,
            thread: worker.thread().clone(),
        })
    }

    /// Classifies replies on IPC workers and wakes storage after delivery.
    /// The completion callback only uses a non-blocking handoff.
    pub(crate) fn observer(&self) -> ResponseDeliveryObserver {
        let delivery = self.delivery.clone();
        let attempt_context_sender = self.attempt_context_sender.clone();
        let thread = self.thread.clone();
        Arc::new(move |request, response| {
            let (receipt, correlation_id) = new_scan_receipt(request, response)?;
            let ticket = delivery.claim(&receipt.job_ref)?;
            let attempt_context = AttemptContext {
                job_ref: receipt.job_ref,
                correlation_id,
            };
            let context_sender = attempt_context_sender.clone();
            let thread = thread.clone();
            Some(Box::new(move |outcome| {
                match outcome {
                    ResponseDelivery::Posted => match context_sender.send(attempt_context) {
                        Ok(()) => ticket.posted(),
                        Err(_) => drop(ticket),
                    },
                    ResponseDelivery::NotPosted => drop(ticket),
                }
                thread.unpark();
            }))
        })
    }
}

impl Drop for RecentScopeWorker {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        self.thread.unpark();
    }
}

fn new_scan_receipt(request: &str, response: &str) -> Option<(ScanStart, Id)> {
    let request: serde_json::Value = serde_json::from_str(request).ok()?;
    if request.get("command")?.as_str()? != "du_call" {
        return None;
    }
    let request_envelope: CallEnvelope =
        serde_json::from_value(request.get("args")?.get("envelope")?.clone()).ok()?;
    if request_envelope.name != "scan.start"
        || request_envelope.contract_version != ENVELOPE_VERSION
    {
        return None;
    }
    let envelope: ResultEnvelope = serde_json::from_str(response).ok()?;
    if envelope.name != "scan.start"
        || envelope.error.is_some()
        || envelope.contract_version != ENVELOPE_VERSION
        || envelope.correlation_id != request_envelope.correlation_id
    {
        return None;
    }
    let receipt: ScanStart = serde_json::from_value(envelope.body).ok()?;
    (!receipt.existing).then_some((receipt, request_envelope.correlation_id))
}

fn persist(
    database: &SharedDatabase,
    cap: NonZeroUsize,
    candidate: &RecentScopeAdmission,
    stopped: &AtomicBool,
    encoder: &RecentScopeEncoder,
) -> Result<(), PersistFailure> {
    if !database
        .resolve_recent_scope_remember(cap, stopped)
        .map_err(PersistFailure::from_recent_error)?
    {
        return Ok(());
    }
    if stopped.load(Ordering::Acquire) {
        return Ok(());
    }

    let now_ms = crate::settings::current_time_ms();
    let encoded = match encoder(candidate, stopped) {
        Ok(encoded) => encoded,
        Err(BodyError::EvidenceUnavailable) => return Ok(()),
        Err(_) => {
            return resolve_preparation_failure(
                database,
                cap,
                stopped,
                PersistFailure::InvalidEvidence,
            );
        }
    };
    let id = match recent_id(now_ms) {
        Ok(id) => id,
        Err(failure) => return resolve_preparation_failure(database, cap, stopped, failure),
    };
    let item = RecentWrite {
        id,
        fingerprint: encoded.fingerprint,
        body: &encoded.body,
        include_count: encoded.include_count,
        now_ms,
    };
    database
        .put_recent_scope(cap, &item, stopped)
        .map_err(PersistFailure::from_recent_error)
}

fn resolve_preparation_failure(
    database: &SharedDatabase,
    cap: NonZeroUsize,
    stopped: &AtomicBool,
    failure: PersistFailure,
) -> Result<(), PersistFailure> {
    if database
        .resolve_recent_scope_remember(cap, stopped)
        .map_err(PersistFailure::from_recent_error)?
    {
        Err(failure)
    } else {
        Ok(())
    }
}

#[derive(Clone, Copy)]
enum PersistFailure {
    InvalidEvidence,
    Busy,
    DiskFull,
    StorageUnavailable,
}

impl PersistFailure {
    fn from_recent_error(error: RecentError) -> Self {
        match error {
            RecentError::Busy => Self::Busy,
            RecentError::DiskFull => Self::DiskFull,
            RecentError::Invalid => Self::InvalidEvidence,
            RecentError::Corrupt | RecentError::Storage => Self::StorageUnavailable,
        }
    }

    fn with_context(self, job_ref: Id, correlation_id: Id) -> RecentScopeWriteWarning {
        match self {
            Self::InvalidEvidence => RecentScopeWriteWarning::InvalidEvidence {
                job_ref,
                correlation_id,
            },
            Self::Busy => RecentScopeWriteWarning::Busy {
                job_ref,
                correlation_id,
            },
            Self::DiskFull => RecentScopeWriteWarning::DiskFull {
                job_ref,
                correlation_id,
            },
            Self::StorageUnavailable => RecentScopeWriteWarning::StorageUnavailable {
                job_ref,
                correlation_id,
            },
        }
    }
}

fn recent_id(now_ms: i64) -> Result<[u8; 16], PersistFailure> {
    let mut bytes = [0; 16];
    OsRandom
        .fill(&mut bytes)
        .map_err(|_| PersistFailure::StorageUnavailable)?;
    bytes[..6].copy_from_slice(&now_ms.to_be_bytes()[2..]);
    bytes[6] = (bytes[6] & 0x0f) | 0x70;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Ok(bytes)
}

fn collect_attempt_contexts(receiver: &Receiver<AttemptContext>, pending: &mut HashMap<Id, Id>) {
    while let Ok(context) = receiver.try_recv() {
        pending.insert(context.job_ref, context.correlation_id);
    }
}

#[cfg(test)]
mod tests;
