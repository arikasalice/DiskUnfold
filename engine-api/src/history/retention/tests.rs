use super::*;
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

use du_core_contracts::{
    CallEnvelope, ENVELOPE_VERSION, ErrorCode,
    history_retention_ipc::HistoryRetentionPolicy,
    job::{HistoryAppend, HistoryExpiryCandidate, JobHistory, JobSummary},
};
use du_job_runtime::{JobClock, JobEvent, JobEvents, JobIdentity, RuntimeError};
use serde_json::{Value, json};

struct Clock(i64);

impl JobClock for Clock {
    fn now_ms(&self) -> i64 {
        self.0
    }
}

struct Identity(AtomicUsize);

impl JobIdentity for Identity {
    fn next_id(&self) -> Id {
        id(self.0.fetch_add(1, Ordering::SeqCst) as u8)
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

mod fakes;
use fakes::{CandidateReader, MemoryPruneDelete, ReaderState, UndoRegistry};

struct Setup {
    engine: crate::api::Engine,
    runtime: Arc<du_job_runtime::JobRuntime>,
    trigger: HistoryPruneTrigger,
    prune_delete: Arc<MemoryPruneDelete>,
    policy: Arc<Mutex<HistoryRetentionPolicy>>,
    reader: Arc<Mutex<ReaderState>>,
    undo: Arc<UndoRegistry>,
    save_fails: Arc<AtomicBool>,
    saves: Arc<AtomicUsize>,
    wake_count: Arc<AtomicUsize>,
}

fn setup(
    initial_policy: HistoryRetentionPolicy,
    candidates: Vec<HistoryExpiryCandidate>,
) -> Result<Setup, String> {
    let policy_store = Arc::new(Mutex::new(initial_policy));
    let mut ordered = candidates.clone();
    ordered.sort_by_key(|candidate| {
        (
            candidate.reckon_at_ms,
            candidate.job_id.v7_bytes().unwrap_or([0; 16]),
        )
    });
    let positions = ordered
        .into_iter()
        .enumerate()
        .map(|(index, candidate)| (candidate.job_id, index as u64 + 1))
        .collect();
    let reader = Arc::new(Mutex::new(ReaderState {
        delete_generation: 7,
        candidates,
        positions,
        row_counts: HashMap::new(),
        prune_reads: Vec::new(),
        apply_expiry_filter: false,
        fail: false,
    }));
    let undo = Arc::new(UndoRegistry::default());
    let save_fails = Arc::new(AtomicBool::new(false));
    let saves = Arc::new(AtomicUsize::new(0));
    let wake_count = Arc::new(AtomicUsize::new(0));
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
    let wake_sink = wake_count.clone();
    let prune_delete = Arc::new(MemoryPruneDelete {
        reader: reader.clone(),
        policy: policy_store.clone(),
        fail: AtomicBool::new(false),
        fail_after_first_batch: AtomicBool::new(false),
        calls: AtomicUsize::new(0),
        change_policy_after_delete: AtomicBool::new(false),
        cancel_after_delete: Mutex::new(None),
    });
    let mut builder = EngineBuilder::default();
    let trigger = register(
        &mut builder,
        runtime.clone(),
        source,
        saver,
        CandidateReader(reader.clone()),
        undo.clone(),
        prune_delete.clone(),
        Arc::new(move || {
            wake_sink.fetch_add(1, Ordering::SeqCst);
        }),
    )
    .map_err(|error| error.to_string())?;
    let engine = builder.build().map_err(|error| error.to_string())?;
    Ok(Setup {
        engine,
        runtime,
        trigger,
        prune_delete,
        policy: policy_store,
        reader,
        undo,
        save_fails,
        saves,
        wake_count,
    })
}

fn runtime() -> Result<du_job_runtime::JobRuntime, String> {
    du_job_runtime::JobRuntime::new(
        Arc::new(Identity(AtomicUsize::new(240))),
        Arc::new(Clock(10_000)),
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
        policy_fingerprint: format!("policy-{enabled}-{}", specified_days.unwrap_or(0)),
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
mod prune;
mod review;
