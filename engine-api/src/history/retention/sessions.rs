use std::{
    collections::VecDeque,
    sync::{Mutex, MutexGuard},
};

use du_core_contracts::{
    Id,
    history_retention_ipc::{
        HistoryRetentionApplyResponse, HistoryRetentionCancelOutcome,
        HistoryRetentionCancelResponse, HistoryRetentionReviewRequest,
    },
};

use super::{review_not_found, session_evicted};
use crate::error::EngineError;

const MAX_REVIEWS: usize = 8;

enum ReviewState {
    Pending(HistoryRetentionReviewRequest, String),
    Applied(HistoryRetentionApplyResponse),
    Cancelled,
}

struct ReviewRecord {
    id: Id,
    state: ReviewState,
}

pub(super) struct ReviewSessions {
    session_prefix: [u8; 12],
    issued: u32,
    records: VecDeque<ReviewRecord>,
}

impl ReviewSessions {
    pub(super) fn new() -> Self {
        let uuid = uuid::Uuid::now_v7();
        let mut bytes = *uuid.as_bytes();
        bytes[12..].fill(0);
        let mut session_prefix = [0; 12];
        session_prefix.copy_from_slice(&bytes[..12]);
        Self {
            session_prefix,
            issued: 0,
            records: VecDeque::new(),
        }
    }

    pub(super) fn next_ref(&self) -> Result<Id, EngineError> {
        let sequence = self
            .issued
            .checked_add(1)
            .ok_or_else(|| EngineError::reason("support.internal_error"))?;
        let mut bytes = [0; 16];
        bytes[..12].copy_from_slice(&self.session_prefix);
        bytes[12..].copy_from_slice(&sequence.to_be_bytes());
        Ok(Id::from_v7_bytes(bytes))
    }

    pub(super) fn insert(
        &mut self,
        id: Id,
        request: HistoryRetentionReviewRequest,
        fingerprint: String,
    ) {
        if self.records.len() == MAX_REVIEWS {
            self.records.pop_front();
        }
        if let Some(bytes) = id.v7_bytes() {
            self.issued = u32::from_be_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]);
        }
        self.records.push_back(ReviewRecord {
            id,
            state: ReviewState::Pending(request, fingerprint),
        });
    }

    pub(super) fn lookup(&self, id: &Id) -> ReviewLookup {
        if let Some(record) = self.records.iter().find(|record| record.id == *id) {
            return match &record.state {
                ReviewState::Pending(request, fingerprint) => {
                    ReviewLookup::Pending(request.clone(), fingerprint.clone())
                }
                ReviewState::Applied(response) => ReviewLookup::Applied(response.clone()),
                ReviewState::Cancelled => ReviewLookup::Cancelled,
            };
        }
        if self.was_issued(id) {
            ReviewLookup::Evicted
        } else {
            ReviewLookup::Unknown
        }
    }

    fn was_issued(&self, id: &Id) -> bool {
        let Some(bytes) = id.v7_bytes() else {
            return false;
        };
        if bytes[..12] != self.session_prefix {
            return false;
        }
        let sequence = u32::from_be_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]);
        sequence > 0 && sequence <= self.issued
    }

    pub(super) fn mark_applied(
        &mut self,
        id: &Id,
        response: HistoryRetentionApplyResponse,
    ) -> Result<(), EngineError> {
        let record = self
            .records
            .iter_mut()
            .find(|record| record.id == *id)
            .ok_or_else(review_not_found)?;
        record.state = ReviewState::Applied(response);
        Ok(())
    }

    pub(super) fn cancel(
        &mut self,
        id: &Id,
    ) -> Result<HistoryRetentionCancelResponse, EngineError> {
        let Some(record) = self.records.iter_mut().find(|record| record.id == *id) else {
            return Err(match self.lookup(id) {
                ReviewLookup::Evicted => session_evicted(),
                _ => review_not_found(),
            });
        };
        let outcome = match &record.state {
            ReviewState::Pending(_, _) => {
                record.state = ReviewState::Cancelled;
                HistoryRetentionCancelOutcome::Cancelled
            }
            ReviewState::Applied(_) | ReviewState::Cancelled => {
                HistoryRetentionCancelOutcome::NoEffect
            }
        };
        Ok(HistoryRetentionCancelResponse { outcome })
    }
}

pub(super) enum ReviewLookup {
    Pending(HistoryRetentionReviewRequest, String),
    Applied(HistoryRetentionApplyResponse),
    Cancelled,
    Evicted,
    Unknown,
}

pub(super) fn lock_sessions(
    sessions: &Mutex<ReviewSessions>,
) -> Result<MutexGuard<'_, ReviewSessions>, EngineError> {
    sessions
        .lock()
        .map_err(|_| EngineError::reason("support.internal_error"))
}
