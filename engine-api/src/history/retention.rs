//! GUI use cases for retention-policy review, application and cancellation.

use std::{
    collections::HashSet,
    sync::{Arc, Mutex, MutexGuard},
};

use du_core_contracts::{
    Id,
    history_retention_ipc::{
        HistoryRetentionApplyOutcome, HistoryRetentionApplyRequest, HistoryRetentionApplyResponse,
        HistoryRetentionCancelRequest, HistoryRetentionCancelResponse, HistoryRetentionChangeKind,
        HistoryRetentionPolicy, HistoryRetentionReview, HistoryRetentionReviewRequest,
    },
    job::{HistoryExpiryRead, SessionUndoInfo},
};
use du_job_runtime::JobRuntime;

use crate::{
    api::{CallerCtx, Channel, EngineBuilder, UseCase},
    error::EngineError,
};
mod sessions;
use sessions::{ReviewLookup, ReviewSessions, lock_sessions};

/// Reads the authoritative stored policy when a use case starts.
pub type HistoryRetentionPolicySource = Arc<dyn Fn() -> HistoryRetentionPolicy + Send + Sync>;

/// Failure to save a reviewed policy through its owner settings adapter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HistoryRetentionPolicySaveError {
    /// The host settings owner could not commit the policy.
    Unavailable,
}

/// Saves a policy through the owner of the retention settings.
pub type HistoryRetentionPolicySaver = Arc<
    dyn Fn(bool, Option<u32>) -> Result<HistoryRetentionPolicy, HistoryRetentionPolicySaveError>
        + Send
        + Sync,
>;

/// Registers the retention commands with a runtime clock and host-owned ports.
pub fn register<R>(
    builder: &mut EngineBuilder,
    runtime: Arc<JobRuntime>,
    policy_source: HistoryRetentionPolicySource,
    policy_saver: HistoryRetentionPolicySaver,
    expiry_reader: R,
    undo_info: Arc<dyn SessionUndoInfo>,
) where
    R: HistoryExpiryRead + 'static,
{
    let sessions = Arc::new(Mutex::new(ReviewSessions::new()));
    let ports = Arc::new(RetentionPorts {
        runtime,
        policy_source: policy_source.clone(),
        policy_saver,
        expiry_reader: Mutex::new(expiry_reader),
        undo_info,
        sessions: sessions.clone(),
    });
    builder.register(GetRetention(policy_source));
    builder.register(ReviewRetention(ports.clone()));
    builder.register(ApplyRetention(ports));
    builder.register(CancelRetention(sessions));
}

struct GetRetention(HistoryRetentionPolicySource);

impl UseCase for GetRetention {
    const NAME: &'static str = "history.retention_get";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = du_core_contracts::history_retention_ipc::HistoryRetentionGetRequest;
    type Response = HistoryRetentionPolicy;

    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }

    fn execute(&self, _: Self::Request, _: &CallerCtx) -> Result<Self::Response, EngineError> {
        Ok((self.0)())
    }
}

struct RetentionPorts<R> {
    runtime: Arc<JobRuntime>,
    policy_source: HistoryRetentionPolicySource,
    policy_saver: HistoryRetentionPolicySaver,
    expiry_reader: Mutex<R>,
    undo_info: Arc<dyn SessionUndoInfo>,
    sessions: Arc<Mutex<ReviewSessions>>,
}

struct ReviewRetention<R>(Arc<RetentionPorts<R>>);

impl<R: HistoryExpiryRead + 'static> UseCase for ReviewRetention<R> {
    const NAME: &'static str = "history.retention_review";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = HistoryRetentionReviewRequest;
    type Response = HistoryRetentionReview;

    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }

    fn execute(
        &self,
        request: Self::Request,
        _: &CallerCtx,
    ) -> Result<Self::Response, EngineError> {
        validate_request(&request)?;
        let mut sessions = lock_sessions(&self.0.sessions)?;
        let review_ref = sessions.next_ref()?;
        let evaluation = evaluate(&self.0, &request, review_ref.clone())?;
        sessions.insert(review_ref, request, evaluation.review.fingerprint.clone());
        Ok(evaluation.review)
    }
}

struct ApplyRetention<R>(Arc<RetentionPorts<R>>);

impl<R: HistoryExpiryRead + 'static> UseCase for ApplyRetention<R> {
    const NAME: &'static str = "history.retention_apply";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = HistoryRetentionApplyRequest;
    type Response = HistoryRetentionApplyResponse;

    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }

    fn execute(
        &self,
        request: Self::Request,
        _: &CallerCtx,
    ) -> Result<Self::Response, EngineError> {
        apply(&self.0, request)
    }
}

struct CancelRetention(Arc<Mutex<ReviewSessions>>);

impl UseCase for CancelRetention {
    const NAME: &'static str = "history.retention_cancel";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = HistoryRetentionCancelRequest;
    type Response = HistoryRetentionCancelResponse;

    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }

    fn execute(
        &self,
        request: Self::Request,
        _: &CallerCtx,
    ) -> Result<Self::Response, EngineError> {
        let mut sessions = lock_sessions(&self.0)?;
        sessions.cancel(&request.review_ref)
    }
}

struct Evaluation {
    review: HistoryRetentionReview,
    current_policy: HistoryRetentionPolicy,
    target_enabled: bool,
    target_days: Option<u32>,
}

struct ReviewCounts {
    delete_generation: u64,
    target_ids: Vec<Id>,
    held_count: u64,
    undo_count: u64,
    earliest_at_ms: Option<i64>,
    latest_at_ms: Option<i64>,
}

fn validate_request(request: &HistoryRetentionReviewRequest) -> Result<(), EngineError> {
    if request.days == Some(0) || (request.enabled && request.days.is_none()) {
        return Err(EngineError::reason("command.body_invalid"));
    }
    Ok(())
}

fn evaluate<R: HistoryExpiryRead>(
    ports: &RetentionPorts<R>,
    request: &HistoryRetentionReviewRequest,
    review_ref: Id,
) -> Result<Evaluation, EngineError> {
    let current_policy = (ports.policy_source)();
    if current_policy.unavailable_reason.is_some() {
        return Err(EngineError::reason("history.policy_store_failed"));
    }
    let target_days = if request.enabled {
        request.days
    } else {
        request.days.or(current_policy.specified_days)
    };
    let change_kind = classify(&current_policy, request.enabled, target_days);
    let cleanup_after_apply = matches!(
        change_kind,
        HistoryRetentionChangeKind::Enable | HistoryRetentionChangeKind::Shorten
    );
    // F21 HLD table 6: only enabling and shortening need confirmation; both start cleanup.
    let confirmation_required = cleanup_after_apply;
    let counts = if cleanup_after_apply {
        review_counts(ports, target_days.ok_or_else(body_invalid)?)?
    } else {
        let delete_generation = lock_reader(&ports.expiry_reader)?
            .delete_generation()
            .map_err(|_| count_unavailable())?;
        ReviewCounts {
            delete_generation,
            target_ids: Vec::new(),
            held_count: 0,
            undo_count: 0,
            earliest_at_ms: None,
            latest_at_ms: None,
        }
    };
    let targets_digest = digest_ids(&counts.target_ids)?;
    let fingerprint = review_fingerprint(
        change_kind,
        confirmation_required,
        target_days,
        counts.delete_generation,
        request.enabled,
        &targets_digest,
    );
    let target_count = u64::try_from(counts.target_ids.len()).map_err(|_| count_unavailable())?;
    Ok(Evaluation {
        review: HistoryRetentionReview {
            review_ref,
            change_kind,
            confirmation_required,
            cleanup_after_apply,
            target_count: target_count.to_string(),
            target_earliest_at_ms: counts.earliest_at_ms,
            target_latest_at_ms: counts.latest_at_ms,
            held_count: counts.held_count.to_string(),
            undo_info_removal_count: counts.undo_count.to_string(),
            fingerprint,
        },
        current_policy,
        target_enabled: request.enabled,
        target_days,
    })
}

fn review_counts<R: HistoryExpiryRead>(
    ports: &RetentionPorts<R>,
    days: u32,
) -> Result<ReviewCounts, EngineError> {
    let mut reader = lock_reader(&ports.expiry_reader)?;
    let candidates = ports
        .runtime
        .history_expiry_candidates(&mut *reader, days)
        .map_err(|_| count_unavailable())?;
    if u64::try_from(candidates.candidates.len()).ok() != Some(candidates.count) {
        return Err(count_unavailable());
    }
    let ids: Vec<Id> = candidates
        .candidates
        .iter()
        .map(|candidate| candidate.job_id.clone())
        .collect();
    let ids_set: HashSet<Id> = ids.iter().cloned().collect();
    if ids_set.len() != ids.len() {
        return Err(count_unavailable());
    }
    let present = ports.undo_info.present(&ids);
    let (present_ids, port_unavailable) = match present {
        Ok(values) => {
            let set: HashSet<Id> = values.into_iter().collect();
            if !set.is_subset(&ids_set) {
                return Err(count_unavailable());
            }
            (set, false)
        }
        Err(_) => (HashSet::new(), true),
    };
    let mut target_ids = Vec::new();
    let mut held_count = 0_u64;
    let mut undo_count = 0_u64;
    let mut earliest = None;
    let mut latest = None;
    for candidate in &candidates.candidates {
        let has_undo = present_ids.contains(&candidate.job_id);
        let in_use = if has_undo && !port_unavailable && !candidate.has_nonterminal_follow_up {
            ports.undo_info.in_use(&candidate.job_id)
        } else {
            Ok(false)
        };
        let held = candidate.has_nonterminal_follow_up
            || port_unavailable
            || matches!(in_use, Ok(true) | Err(_));
        if held {
            held_count += 1;
            continue;
        }
        if has_undo {
            undo_count += 1;
        }
        target_ids.push(candidate.job_id.clone());
        earliest = Some(earliest.map_or(candidate.reckon_at_ms, |value: i64| {
            value.min(candidate.reckon_at_ms)
        }));
        latest = Some(latest.map_or(candidate.reckon_at_ms, |value: i64| {
            value.max(candidate.reckon_at_ms)
        }));
    }
    Ok(ReviewCounts {
        delete_generation: candidates.delete_generation,
        target_ids,
        held_count,
        undo_count,
        earliest_at_ms: earliest,
        latest_at_ms: latest,
    })
}

fn classify(
    current: &HistoryRetentionPolicy,
    enabled: bool,
    days: Option<u32>,
) -> HistoryRetentionChangeKind {
    match (current.enabled, enabled) {
        (false, true) => HistoryRetentionChangeKind::Enable,
        (true, false) => HistoryRetentionChangeKind::Disable,
        (false, false) if current.specified_days != days => HistoryRetentionChangeKind::SetPeriod,
        (false, false) => HistoryRetentionChangeKind::None,
        (true, true) => {
            let current_days = current.specified_days.unwrap_or(current.suggested_days);
            match days.unwrap_or(current_days).cmp(&current_days) {
                std::cmp::Ordering::Less => HistoryRetentionChangeKind::Shorten,
                std::cmp::Ordering::Greater => HistoryRetentionChangeKind::Extend,
                std::cmp::Ordering::Equal => HistoryRetentionChangeKind::None,
            }
        }
    }
}

fn digest_ids(ids: &[Id]) -> Result<String, EngineError> {
    let mut bytes: Vec<[u8; 16]> = ids
        .iter()
        .map(|id| id.v7_bytes().ok_or_else(count_unavailable))
        .collect::<Result<_, _>>()?;
    bytes.sort_unstable();
    let mut hasher = blake3::Hasher::new();
    for id in bytes {
        hasher.update(&id);
    }
    Ok(lower_hex(hasher.finalize().as_bytes()))
}

fn review_fingerprint(
    change_kind: HistoryRetentionChangeKind,
    confirmation_required: bool,
    days: Option<u32>,
    delete_generation: u64,
    enabled: bool,
    targets_digest: &str,
) -> String {
    let days = days.map_or_else(|| "null".to_owned(), |value| value.to_string());
    let canonical = format!(
        "{{\"changeKind\":\"{}\",\"confirmationRequired\":{},\"days\":{},\"deleteGeneration\":\"{}\",\"enabled\":{},\"targets\":\"{}\"}}",
        change_kind.as_str(),
        confirmation_required,
        days,
        delete_generation,
        enabled,
        targets_digest,
    );
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"diskunfold.history.retention-review.v1\0");
    hasher.update(canonical.as_bytes());
    lower_hex(&hasher.finalize().as_bytes()[..16])
}

fn lower_hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(output, "{byte:02x}");
    }
    output
}

fn apply<R: HistoryExpiryRead>(
    ports: &RetentionPorts<R>,
    request: HistoryRetentionApplyRequest,
) -> Result<HistoryRetentionApplyResponse, EngineError> {
    let mut sessions = lock_sessions(&ports.sessions)?;
    let (review_request, old_fingerprint) = match sessions.lookup(&request.review_ref) {
        ReviewLookup::Pending(review, fingerprint) => (review, fingerprint),
        ReviewLookup::Applied(response) => return Ok(response),
        ReviewLookup::Cancelled => return Err(review_not_found()),
        ReviewLookup::Evicted => return Err(session_evicted()),
        ReviewLookup::Unknown => return Err(review_not_found()),
    };
    if request.fingerprint != old_fingerprint {
        return Err(review_stale());
    }
    let evaluation = evaluate(ports, &review_request, request.review_ref.clone())?;
    if evaluation.review.fingerprint != request.fingerprint {
        return Err(review_stale());
    }
    let (outcome, policy) = if evaluation.review.change_kind == HistoryRetentionChangeKind::None {
        (
            HistoryRetentionApplyOutcome::NoEffect,
            evaluation.current_policy,
        )
    } else {
        let policy = (ports.policy_saver)(evaluation.target_enabled, evaluation.target_days)
            .map_err(|_| EngineError::reason("history.policy_store_failed"))?;
        (HistoryRetentionApplyOutcome::Applied, policy)
    };
    let response = HistoryRetentionApplyResponse {
        outcome,
        policy,
        cleanup_job_ref: None,
    };
    sessions.mark_applied(&request.review_ref, response.clone())?;
    Ok(response)
}

fn lock_reader<R>(reader: &Mutex<R>) -> Result<MutexGuard<'_, R>, EngineError> {
    reader.lock().map_err(|_| count_unavailable())
}

fn body_invalid() -> EngineError {
    EngineError::reason("command.body_invalid")
}

fn count_unavailable() -> EngineError {
    EngineError::reason("history.count_unavailable")
}

fn review_stale() -> EngineError {
    EngineError::reason("history.review_stale")
}

fn review_not_found() -> EngineError {
    EngineError::reason("history.review_not_found")
}

fn session_evicted() -> EngineError {
    EngineError::reason("session.evicted")
}

#[cfg(test)]
mod tests;
