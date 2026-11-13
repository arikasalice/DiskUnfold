//! Restore a saved scope and recheck its current volumes through GUI IPC.
use std::{sync::Arc, time::Instant};

use du_core_contracts::{
    Timestamp,
    analysis_ipc::{
        RecentScopeOpen, RecentScopeOpenRequest, RecentScopeOpenTarget, RecentScopeTargetState,
    },
    scan_observation::ScopeRootPort,
    volume::RecentVolumePort,
};
use du_engine_api::{
    api::{CallerCtx, Channel, EngineBuilder, UseCase},
    error::EngineError,
    scope::recent::{RecentVolumeMatch, review_matched_scope, review_recent_volume},
};

use crate::{recent_scope_body::RecentScopeBody, shared_database::SharedDatabase};

/// Registers restore with the same root owner used by ordinary scope review.
pub(crate) fn register(
    builder: &mut EngineBuilder,
    database: SharedDatabase,
    roots: Arc<dyn ScopeRootPort>,
    volumes: Arc<dyn RecentVolumePort>,
) {
    builder.register(Open {
        database,
        roots,
        volumes,
    });
}

struct Open {
    database: SharedDatabase,
    roots: Arc<dyn ScopeRootPort>,
    volumes: Arc<dyn RecentVolumePort>,
}

impl UseCase for Open {
    const NAME: &'static str = "scope.recent_open";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = RecentScopeOpenRequest;
    type Response = RecentScopeOpen;

    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }

    fn execute(
        &self,
        request: Self::Request,
        caller: &CallerCtx,
    ) -> Result<Self::Response, EngineError> {
        let reference = request.recent_scope_ref.v7_bytes().ok_or_else(internal)?;
        let row = self
            .database
            .recent_scope(&reference)
            .map_err(|_| internal())?
            .ok_or_else(|| EngineError::new("ref.unavailable", Vec::new()))?;
        Timestamp::from_unix_millis(row.summary.last_used_at_ms).ok_or_else(internal)?;
        let saved =
            RecentScopeBody::decode(&row.body, 1, &row.fingerprint, row.summary.include_count)
                .map_err(|_| internal())?;
        let draft = saved.scope_draft();
        let continue_work = || {
            !caller.is_cancelled()
                && caller
                    .deadline
                    .is_none_or(|deadline| Instant::now() < deadline)
        };
        let mut volumes = Vec::with_capacity(saved.targets().len());
        let mut targets = Vec::with_capacity(saved.targets().len());
        for target in saved.targets() {
            let volume = target.volume_id.bytes().map_err(|_| internal())?;
            let observation =
                review_recent_volume(&*self.volumes, &volume, &target.entry_path, &continue_work);
            volumes.push(volume);
            let (state, proposed_entry) = match observation {
                RecentVolumeMatch::Matched => (RecentScopeTargetState::Matched, None),
                RecentVolumeMatch::Moved { proposed_entry } => {
                    (RecentScopeTargetState::Moved, Some(proposed_entry))
                }
                RecentVolumeMatch::Disconnected => (RecentScopeTargetState::Disconnected, None),
                RecentVolumeMatch::Unverifiable => (RecentScopeTargetState::Unverifiable, None),
            };
            targets.push(RecentScopeOpenTarget {
                path: target.applied_path.clone(),
                state,
                proposed_entry,
            });
        }
        let review = if targets
            .iter()
            .all(|target| target.state == RecentScopeTargetState::Matched)
        {
            review_matched_scope(
                &draft,
                request.request_seq,
                &volumes,
                &*self.roots,
                crate::settings::current_time_ms,
                &continue_work,
            )
        } else {
            None
        };
        Ok(RecentScopeOpen {
            draft,
            targets,
            review,
        })
    }
}

fn internal() -> EngineError {
    EngineError::new("support.internal_error", Vec::new())
}

#[cfg(test)]
mod tests;
