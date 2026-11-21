use super::*;
use std::{
    collections::HashSet,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering},
    },
};

use du_core_contracts::{
    CallEnvelope, ENVELOPE_VERSION, ErrorCode,
    history_retention_ipc::HistoryRetentionPolicy,
    job::{
        HistoryAppend, HistoryExpiryCandidate, HistoryExpiryCandidates, HistoryExpiryReadError,
        JobHistory, JobSummary,
    },
};
use du_job_runtime::{JobClock, JobEvent, JobEvents, JobIdentity, RuntimeError};
use serde_json::{Value, json};

struct Clock(AtomicI64);

impl JobClock for Clock {
    fn now_ms(&self) -> i64 {
        self.0.fetch_add(1, Ordering::SeqCst)
    }
}

struct Identity;

impl JobIdentity for Identity {
    fn next_id(&self) -> Id {
        id(250)
    }
}

struct History;

impl JobHistory for History {
    fn append(&self, _: &JobSummary) -> Result<HistoryAppend, String> {
        Ok(HistoryAppend::Added)
    }
}

struct Events;

impl JobEvents for Events {
    fn publish(&self, _: JobEvent) {}
}

#[derive(Clone)]
struct CandidateReader(Arc<Mutex<ReaderState>>);

struct ReaderState {
    delete_generation: u64,
    candidates: Vec<HistoryExpiryCandidate>,
    fail: bool,
}

impl HistoryExpiryRead for CandidateReader {
    fn delete_generation(&mut self) -> Result<u64, HistoryExpiryReadError> {
        let state = self
            .0
            .lock()
            .map_err(|_| HistoryExpiryReadError::Unavailable)?;
        if state.fail {
            return Err(HistoryExpiryReadError::Unavailable);
        }
        Ok(state.delete_generation)
    }

    fn expiry_candidates(
        &mut self,
        _: u32,
        _: i64,
    ) -> Result<HistoryExpiryCandidates, HistoryExpiryReadError> {
        let state = self
            .0
            .lock()
            .map_err(|_| HistoryExpiryReadError::Unavailable)?;
        if state.fail {
            return Err(HistoryExpiryReadError::Unavailable);
        }
        let mut candidates = state.candidates.clone();
        candidates.sort_by_key(|candidate| candidate.job_id.v7_bytes().unwrap_or([0; 16]));
        let mut hasher = blake3::Hasher::new();
        for candidate in &candidates {
            let bytes = candidate
                .job_id
                .v7_bytes()
                .ok_or(HistoryExpiryReadError::Unavailable)?;
            hasher.update(&bytes);
        }
        let count =
            u64::try_from(candidates.len()).map_err(|_| HistoryExpiryReadError::Unavailable)?;
        let earliest = candidates
            .iter()
            .map(|candidate| candidate.reckon_at_ms)
            .min();
        let latest = candidates
            .iter()
            .map(|candidate| candidate.reckon_at_ms)
            .max();
        Ok(HistoryExpiryCandidates {
            delete_generation: state.delete_generation,
            count,
            earliest_reckon_at_ms: earliest,
            latest_reckon_at_ms: latest,
            digest: *hasher.finalize().as_bytes(),
            candidates,
        })
    }
}

#[derive(Default)]
struct UndoRegistry {
    present: Mutex<HashSet<Id>>,
    in_use: Mutex<HashSet<Id>>,
    in_use_errors: Mutex<HashSet<Id>>,
    unavailable: AtomicBool,
}

impl SessionUndoInfo for UndoRegistry {
    fn present(
        &self,
        job_ids: &[Id],
    ) -> Result<Vec<Id>, du_core_contracts::job::UndoInfoUnavailable> {
        if self.unavailable.load(Ordering::SeqCst) {
            return Err(du_core_contracts::job::UndoInfoUnavailable);
        }
        let present = self
            .present
            .lock()
            .map_err(|_| du_core_contracts::job::UndoInfoUnavailable)?;
        Ok(job_ids
            .iter()
            .filter(|job| present.contains(*job))
            .cloned()
            .collect())
    }

    fn in_use(&self, job_id: &Id) -> Result<bool, du_core_contracts::job::UndoInfoUnavailable> {
        if self.unavailable.load(Ordering::SeqCst)
            || self
                .in_use_errors
                .lock()
                .map_err(|_| du_core_contracts::job::UndoInfoUnavailable)?
                .contains(job_id)
        {
            return Err(du_core_contracts::job::UndoInfoUnavailable);
        }
        Ok(self
            .in_use
            .lock()
            .map_err(|_| du_core_contracts::job::UndoInfoUnavailable)?
            .contains(job_id))
    }

    fn reserve_removal(&self, _: &[Id]) -> Vec<Id> {
        Vec::new()
    }

    fn release(&self, _: &[Id]) {}

    fn remove(&self, _: &[Id]) {}
}

struct Setup {
    engine: crate::api::Engine,
    policy: Arc<Mutex<HistoryRetentionPolicy>>,
    reader: Arc<Mutex<ReaderState>>,
    undo: Arc<UndoRegistry>,
    save_fails: Arc<AtomicBool>,
    saves: Arc<AtomicUsize>,
}

fn setup(
    initial_policy: HistoryRetentionPolicy,
    candidates: Vec<HistoryExpiryCandidate>,
) -> Result<Setup, String> {
    let policy_store = Arc::new(Mutex::new(initial_policy));
    let reader = Arc::new(Mutex::new(ReaderState {
        delete_generation: 7,
        candidates,
        fail: false,
    }));
    let undo = Arc::new(UndoRegistry::default());
    let save_fails = Arc::new(AtomicBool::new(false));
    let saves = Arc::new(AtomicUsize::new(0));
    let policy_source = policy_store.clone();
    let source = Arc::new(move || {
        policy_source
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    });
    let policy_saver = policy_store.clone();
    let fail_saver = save_fails.clone();
    let save_count = saves.clone();
    let saver = Arc::new(move |enabled, days| {
        if fail_saver.load(Ordering::SeqCst) {
            return Err(HistoryRetentionPolicySaveError::Unavailable);
        }
        save_count.fetch_add(1, Ordering::SeqCst);
        let next = policy(enabled, days);
        *policy_saver
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = next.clone();
        Ok(next)
    });
    let runtime = Arc::new(runtime()?);
    let mut builder = EngineBuilder::default();
    register(
        &mut builder,
        runtime,
        source,
        saver,
        CandidateReader(reader.clone()),
        undo.clone(),
    );
    let engine = builder.build().map_err(|error| error.to_string())?;
    Ok(Setup {
        engine,
        policy: policy_store,
        reader,
        undo,
        save_fails,
        saves,
    })
}

fn runtime() -> Result<du_job_runtime::JobRuntime, String> {
    du_job_runtime::JobRuntime::new(
        Arc::new(Identity),
        Arc::new(Clock(AtomicI64::new(10_000))),
        Arc::new(History),
        Arc::new(Events),
    )
    .map_err(|error: RuntimeError| error.to_string())
}

fn policy(enabled: bool, specified_days: Option<u32>) -> HistoryRetentionPolicy {
    HistoryRetentionPolicy {
        enabled,
        specified_days,
        suggested_days: 90,
        policy_fingerprint: "0123456789abcdef0123456789abcdef".into(),
        unavailable_reason: None,
    }
}

fn id(seed: u8) -> Id {
    let mut bytes = [seed; 16];
    bytes[6] = 0x70;
    bytes[8] = 0x80;
    Id::from_v7_bytes(bytes)
}

fn candidate(seed: u8, reckon_at_ms: i64, follow_up: bool) -> HistoryExpiryCandidate {
    HistoryExpiryCandidate {
        job_id: id(seed),
        has_nonterminal_follow_up: follow_up,
        reckon_at_ms,
    }
}

fn call(
    engine: &crate::api::Engine,
    channel: Channel,
    name: &str,
    body: Value,
) -> du_core_contracts::ResultEnvelope {
    let correlation_id = id(240);
    engine.call(
        CallEnvelope {
            name: name.into(),
            contract_version: ENVELOPE_VERSION,
            correlation_id: correlation_id.clone(),
            body,
        },
        &CallerCtx::new(channel, correlation_id, None),
    )
}

fn review(setup: &Setup, body: Value) -> Result<Value, String> {
    let result = call(
        &setup.engine,
        Channel::Gui,
        "history.retention_review",
        body,
    );
    if result.error.is_some() {
        return Err(result
            .error
            .map_or_else(String::new, |error| error.reason_key));
    }
    Ok(result.body)
}

fn review_ref(body: &Value) -> Result<Id, String> {
    serde_json::from_value(body["reviewRef"].clone()).map_err(|error| error.to_string())
}

fn apply(setup: &Setup, review: &Value, fingerprint: &str) -> du_core_contracts::ResultEnvelope {
    call(
        &setup.engine,
        Channel::Gui,
        "history.retention_apply",
        json!({"reviewRef": review["reviewRef"], "fingerprint": fingerprint}),
    )
}

mod apply;
mod protocol;
mod review;
