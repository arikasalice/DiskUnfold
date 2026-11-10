//! Explicitly retained model for an investigation session.
use std::sync::Arc;

use du_analysis_index::{
    QueryError, ResultVersion,
    visualization::projection::{self, Location},
};
use du_core_contracts::{
    Id, ItemRef,
    result::ResultVersionRead,
    visualization::rows::{VizRow, VizRows, VizRowsRequest},
};

use super::{Error, session::Session, source_error};

pub(super) struct PinnedModel {
    reference: String,
    source: Arc<ResultVersion>,
    rows: Arc<[VizRow]>,
    layout: Arc<projection::Model>,
}

impl Session {
    /// Retains exactly one current rendered model until explicitly released.
    /// An existing pin must be released before another can be retained.
    pub fn pin_model(&self, model_ref: &str, revision: &Id) -> Result<(), Error> {
        if model_ref.is_empty() {
            return Err(Error::InvalidRange);
        }
        let mut slot = self.pinned.lock().map_err(|_| Error::Unavailable)?;
        if slot.is_some() {
            return Err(Error::InvalidModel);
        }
        let state = self.pages.state.lock().map_err(|_| Error::Unavailable)?;
        let entry = state.entries.get(model_ref).ok_or(Error::ModelExpired)?;
        if &entry.revision != revision {
            return Err(Error::StaleVersion);
        }
        let source = self
            .pages
            .source
            .pin_current(revision)
            .map_err(source_error)?;
        let registered = entry.source.upgrade().ok_or(Error::StaleVersion)?;
        if !Arc::ptr_eq(&source, &registered) {
            return Err(Error::StaleVersion);
        }
        let layout = entry.layout.clone().ok_or(Error::InvalidModel)?;
        let rows = source.pin_viz_rows(model_ref).map_err(super::model_error)?;
        *slot = Some(PinnedModel {
            reference: model_ref.to_owned(),
            source,
            rows,
            layout,
        });
        Ok(())
    }

    /// Releases the retained model; an already admitted read can finish independently.
    pub fn release_pinned_model(&self, model_ref: &str) -> Result<(), Error> {
        let mut slot = self.pinned.lock().map_err(|_| Error::Unavailable)?;
        if slot.as_ref().is_none_or(|pin| pin.reference != model_ref) {
            return Err(Error::ModelExpired);
        }
        let retired = slot.take();
        drop(slot);
        drop(retired);
        Ok(())
    }

    /// Reads rows from the explicitly retained model, even after publication advances.
    pub fn pinned_rows(&self, request: &VizRowsRequest) -> Result<VizRows, Error> {
        request.validate().map_err(|_| Error::InvalidRange)?;
        let slot = self.pinned.lock().map_err(|_| Error::Unavailable)?;
        let pin = slot
            .as_ref()
            .filter(|pin| pin.reference == request.model_ref)
            .ok_or(Error::ModelExpired)?;
        if pin.source.version_id() != &request.result_revision {
            return Err(Error::StaleVersion);
        }
        let source = pin.source.clone();
        let rows = pin.rows.clone();
        drop(slot);
        let start = request.start as usize;
        if start > rows.len() {
            return Err(Error::InvalidRange);
        }
        let end = start.saturating_add(request.count as usize).min(rows.len());
        Ok(VizRows {
            rows: rows[start..end].to_vec(),
            total_count: rows.len() as u64,
            result_revision: source.version_id().clone(),
            observed_at: source.observed_at().cloned(),
            completeness: source.completeness(),
        })
    }

    /// Locates an item in the exact retained source and layout.
    pub fn pinned_locate(&self, model_ref: &str, item: &ItemRef) -> Result<Location, Error> {
        let slot = self.pinned.lock().map_err(|_| Error::Unavailable)?;
        let pin = slot
            .as_ref()
            .filter(|pin| pin.reference == model_ref)
            .ok_or(Error::ModelExpired)?;
        if pin.source.version_id() != &item.result_version {
            return Err(Error::StaleVersion);
        }
        let source = pin.source.clone();
        let layout = pin.layout.clone();
        drop(slot);
        layout.locate(&source, item).map_err(|error| match error {
            QueryError::NotFound => Error::ItemUnavailable,
            QueryError::StaleVersion => Error::StaleVersion,
            _ => Error::Unavailable,
        })
    }
}
