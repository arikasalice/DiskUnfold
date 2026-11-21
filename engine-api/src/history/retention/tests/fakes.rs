use super::policy;
use du_core_contracts::{
    Id,
    history_retention_ipc::HistoryRetentionPolicy,
    job::{
        HistoryExpiryCandidate, HistoryExpiryCandidates, HistoryExpiryRead, HistoryExpiryReadError,
        HistoryPruneCandidate, HistoryPruneDelete, HistoryPruneDeleteError,
        HistoryPruneDeleteReceipt, HistoryPrunePosition, HistoryPruneRead, HistoryPruneReadError,
        HistoryRevision, SessionUndoInfo,
    },
};
use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

#[derive(Clone)]
pub(super) struct CandidateReader(pub(super) Arc<Mutex<ReaderState>>);

pub(super) struct ReaderState {
    pub(super) delete_generation: u64,
    pub(super) candidates: Vec<HistoryExpiryCandidate>,
    pub(super) positions: HashMap<Id, u64>,
    pub(super) row_counts: HashMap<Id, u64>,
    pub(super) prune_reads: Vec<(u32, i64, Option<HistoryPrunePosition>, usize)>,
    pub(super) apply_expiry_filter: bool,
    pub(super) fail: bool,
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
        days: u32,
        reference_ms: i64,
    ) -> Result<HistoryExpiryCandidates, HistoryExpiryReadError> {
        let state = self
            .0
            .lock()
            .map_err(|_| HistoryExpiryReadError::Unavailable)?;
        if state.fail {
            return Err(HistoryExpiryReadError::Unavailable);
        }
        let mut candidates = state.candidates.clone();
        if state.apply_expiry_filter {
            let cutoff = reference_ms.saturating_sub(i64::from(days).saturating_mul(86_400_000));
            candidates.retain(|candidate| {
                candidate.reckon_at_ms <= cutoff && candidate.reckon_at_ms <= reference_ms
            });
        }
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

impl HistoryPruneRead for CandidateReader {
    fn expired_candidates(
        &mut self,
        days: u32,
        reference_ms: i64,
        after: Option<HistoryPrunePosition>,
        limit: usize,
    ) -> Result<Vec<HistoryPruneCandidate>, HistoryPruneReadError> {
        let mut state = self
            .0
            .lock()
            .map_err(|_| HistoryPruneReadError::Unavailable)?;
        if state.fail {
            return Err(HistoryPruneReadError::Unavailable);
        }
        state.prune_reads.push((days, reference_ms, after, limit));
        let cutoff = reference_ms.saturating_sub(i64::from(days).saturating_mul(86_400_000));
        let apply_expiry_filter = state.apply_expiry_filter;
        let mut candidates = state
            .candidates
            .iter()
            .filter_map(|candidate| {
                let position = HistoryPrunePosition {
                    reckon_at_ms: candidate.reckon_at_ms,
                    record_seq: *state.positions.get(&candidate.job_id)?,
                };
                let expired = !apply_expiry_filter
                    || (candidate.reckon_at_ms <= cutoff && candidate.reckon_at_ms <= reference_ms);
                (expired && after.is_none_or(|after| position > after)).then_some(
                    HistoryPruneCandidate {
                        job_id: candidate.job_id.clone(),
                        position,
                        has_nonterminal_follow_up: candidate.has_nonterminal_follow_up,
                        row_count: state
                            .row_counts
                            .get(&candidate.job_id)
                            .copied()
                            .unwrap_or(1),
                    },
                )
            })
            .collect::<Vec<_>>();
        candidates.sort_by_key(|candidate| candidate.position);
        Ok(candidates.into_iter().take(limit).collect())
    }
}

pub(super) struct MemoryPruneDelete {
    pub(super) reader: Arc<Mutex<ReaderState>>,
    pub(super) policy: Arc<Mutex<HistoryRetentionPolicy>>,
    pub(super) fail: AtomicBool,
    pub(super) fail_after_first_batch: AtomicBool,
    pub(super) calls: AtomicUsize,
    pub(super) change_policy_after_delete: AtomicBool,
    pub(super) cancel_after_delete: Mutex<Option<(Arc<du_job_runtime::JobRuntime>, Id)>>,
}

impl HistoryPruneDelete for MemoryPruneDelete {
    fn delete_expired(
        &self,
        expected_fingerprint: &str,
        days: u32,
        reference_ms: i64,
        candidates: &[HistoryPruneCandidate],
        observed_at_ms: i64,
    ) -> Result<Option<HistoryPruneDeleteReceipt>, HistoryPruneDeleteError> {
        let current = self
            .policy
            .lock()
            .map_err(|_| HistoryPruneDeleteError::Unavailable)?
            .clone();
        if !current.enabled
            || current.unavailable_reason.is_some()
            || current.policy_fingerprint != expected_fingerprint
        {
            return Ok(None);
        }
        let call_index = self.calls.fetch_add(1, Ordering::SeqCst);
        if self.fail.load(Ordering::SeqCst)
            || (call_index > 0 && self.fail_after_first_batch.load(Ordering::SeqCst))
        {
            return Err(HistoryPruneDeleteError::Unavailable);
        }
        let cutoff = reference_ms.saturating_sub(i64::from(days).saturating_mul(86_400_000));
        let mut state = self
            .reader
            .lock()
            .map_err(|_| HistoryPruneDeleteError::Unavailable)?;
        let deleted_job_ids = candidates
            .iter()
            .filter(|candidate| {
                !candidate.has_nonterminal_follow_up
                    && candidate.position.reckon_at_ms <= cutoff
                    && candidate.position.reckon_at_ms <= reference_ms
                    && state.candidates.iter().any(|stored| {
                        stored.job_id == candidate.job_id
                            && !stored.has_nonterminal_follow_up
                            && stored.reckon_at_ms == candidate.position.reckon_at_ms
                    })
            })
            .map(|candidate| candidate.job_id.clone())
            .collect::<Vec<_>>();
        let deleted_set = deleted_job_ids.iter().cloned().collect::<HashSet<_>>();
        state
            .candidates
            .retain(|candidate| !deleted_set.contains(&candidate.job_id));
        let deleted_row_count = deleted_job_ids
            .iter()
            .map(|job_id| state.row_counts.get(job_id).copied().unwrap_or(1))
            .sum();
        if deleted_row_count > 0 {
            state.delete_generation = state.delete_generation.saturating_add(1);
        }
        let receipt = HistoryPruneDeleteReceipt {
            revision: HistoryRevision {
                delete_generation: state.delete_generation,
                max_record_seq: state.positions.values().copied().max().unwrap_or(0),
            },
            observed_at_ms,
            deleted_job_ids,
            deleted_row_count,
        };
        drop(state);
        if self
            .change_policy_after_delete
            .swap(false, Ordering::SeqCst)
        {
            *self
                .policy
                .lock()
                .map_err(|_| HistoryPruneDeleteError::Unavailable)? = policy(false, None);
        }
        if let Some((runtime, job_id)) = self
            .cancel_after_delete
            .lock()
            .map_err(|_| HistoryPruneDeleteError::Unavailable)?
            .take()
        {
            let _ = runtime.cancel(&job_id);
        }
        Ok(Some(receipt))
    }
}

#[derive(Default)]
pub(super) struct UndoRegistry {
    pub(super) present: Mutex<HashSet<Id>>,
    pub(super) in_use: Mutex<HashSet<Id>>,
    pub(super) in_use_errors: Mutex<HashSet<Id>>,
    pub(super) reserved: Mutex<HashSet<Id>>,
    pub(super) reserve_failures: Mutex<HashSet<Id>>,
    pub(super) unavailable: AtomicBool,
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

    fn reserve_removal(&self, job_ids: &[Id]) -> Vec<Id> {
        let failures = self
            .reserve_failures
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let present = self
            .present
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut reserved = self
            .reserved
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        job_ids
            .iter()
            .filter(|job_id| {
                present.contains(*job_id)
                    && !failures.contains(*job_id)
                    && reserved.insert((*job_id).clone())
            })
            .cloned()
            .collect()
    }

    fn release(&self, job_ids: &[Id]) {
        if let Ok(mut reserved) = self.reserved.lock() {
            for job_id in job_ids {
                reserved.remove(job_id);
            }
        }
    }

    fn remove(&self, job_ids: &[Id]) {
        self.release(job_ids);
        if let Ok(mut present) = self.present.lock() {
            for job_id in job_ids {
                present.remove(job_id);
            }
        }
    }
}
