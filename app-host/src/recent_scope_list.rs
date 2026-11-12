//! Serve the GUI's bounded recent-scope summary list.
use std::num::NonZeroUsize;

use du_core_contracts::{
    Id, Timestamp,
    analysis_ipc::{RecentScopeList, RecentScopeListRequest, RecentScopeListRow},
};
use du_engine_api::{
    api::{CallerCtx, Channel, EngineBuilder, UseCase},
    error::EngineError,
};

use crate::shared_database::SharedDatabase;

/// Registers the list on the application-owned GUI command boundary.
pub(crate) fn register(builder: &mut EngineBuilder, database: SharedDatabase, cap: NonZeroUsize) {
    builder.register(List { database, cap });
}

struct List {
    database: SharedDatabase,
    cap: NonZeroUsize,
}

impl UseCase for List {
    const NAME: &'static str = "scope.recent_list";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = RecentScopeListRequest;
    type Response = RecentScopeList;

    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }

    fn execute(&self, _: Self::Request, _: &CallerCtx) -> Result<Self::Response, EngineError> {
        let unavailable = || EngineError::new("support.internal_error", Vec::new());
        let rows = self
            .database
            .recent_scope_summaries(self.cap)
            .map_err(|_| unavailable())?;
        let rows = rows
            .into_iter()
            .map(|row| {
                Ok(RecentScopeListRow {
                    recent_scope_ref: Id::from_v7_bytes(row.id),
                    include_count: row.include_count,
                    last_used_at: Timestamp::from_unix_millis(row.last_used_at_ms)
                        .ok_or_else(unavailable)?,
                })
            })
            .collect::<Result<_, EngineError>>()?;
        Ok(RecentScopeList { rows })
    }
}

#[cfg(test)]
mod tests;
