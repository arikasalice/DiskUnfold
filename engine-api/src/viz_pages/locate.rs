//! Admit one live model for a read-only item-location lookup.
use std::sync::Arc;

use du_analysis_index::{QueryError, visualization::projection::Location};
use du_core_contracts::ItemRef;

use super::{Error, ModelPages, source_error, touch};

impl ModelPages {
    /// Locates an item in the exact live model without changing the selection.
    ///
    /// Raw table registrations do not carry projection scope or geometry.
    /// Those registrations cannot answer this lookup.
    pub fn locate(&self, model_ref: &str, item: &ItemRef) -> Result<Location, Error> {
        if model_ref.is_empty() {
            return Err(Error::InvalidRange);
        }
        let mut state = self.state.lock().map_err(|_| Error::Unavailable)?;
        let entry = state.entries.get(model_ref).ok_or(Error::ModelExpired)?;
        if entry.revision != item.result_version {
            return Err(Error::StaleVersion);
        }
        let version = self
            .source
            .pin_current(&entry.revision)
            .map_err(source_error)?;
        let pinned = entry.source.upgrade().ok_or(Error::StaleVersion)?;
        if !Arc::ptr_eq(&pinned, &version) {
            return Err(Error::StaleVersion);
        }
        let layout = entry.layout.clone().ok_or(Error::InvalidModel)?;
        touch(&mut state.lru, model_ref);
        drop(state);
        layout.locate(&version, item).map_err(|error| match error {
            QueryError::NotFound => Error::ItemUnavailable,
            QueryError::StaleVersion => Error::StaleVersion,
            _ => Error::Unavailable,
        })
    }
}
