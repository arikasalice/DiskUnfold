//! Pins the result a classification reads, through the channel exposure wrappers.
use std::sync::Arc;

use du_analysis_index::{IngestSink, QueryError, ResultVersion};
use du_core_contracts::Id;

use super::internal;
use crate::{api::Channel, error::EngineError, source_exposure};

/// Pins the requested (or latest) result for `channel`. A source hidden from the channel
/// reads as a missing version, so the caller learns nothing about it.
pub(super) fn pin(
    store: &IngestSink,
    id: Option<&Id>,
    channel: Channel,
) -> Result<Arc<ResultVersion>, EngineError> {
    match id {
        Some(id) => {
            source_exposure::pin_current_for(store, id, channel).map_err(|error| {
                match error.query() {
                    QueryError::StaleVersion => EngineError::reason("result.version_stale"),
                    QueryError::NotFound => EngineError::reason("result.version_not_found"),
                    _ => internal(),
                }
            })
        }
        None => source_exposure::pin_latest_for(store, channel)
            .map_err(|_| internal())?
            .ok_or_else(|| EngineError::reason("result.version_not_found")),
    }
}
