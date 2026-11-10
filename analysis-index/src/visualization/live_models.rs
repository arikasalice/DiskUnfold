//! Row models owned by their immutable live scan result, never by the session index.
use crate::ResultVersion;
use du_core_contracts::visualization::rows::VizRow;
use std::{collections::HashMap, sync::Arc};

#[derive(Debug, Default)]
pub(crate) struct OwnedModels {
    entries: HashMap<String, Arc<[VizRow]>>,
}
/// Internal source-owned model storage failures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Unavailable,
    Duplicate,
    Missing,
}
impl ResultVersion {
    /// Stores already validated rows in this version. The reference must be session-unique.
    pub fn insert_viz_rows(&self, reference: String, rows: Vec<VizRow>) -> Result<(), Error> {
        let mut cache = self.viz_models.lock().map_err(|_| Error::Unavailable)?;
        if cache.entries.contains_key(&reference) {
            return Err(Error::Duplicate);
        }
        cache.entries.insert(reference, rows.into());
        Ok(())
    }
    /// Pins the model rows so an admitted read can finish after independent eviction.
    pub fn pin_viz_rows(&self, reference: &str) -> Result<Arc<[VizRow]>, Error> {
        let cache = self.viz_models.lock().map_err(|_| Error::Unavailable)?;
        cache.entries.get(reference).cloned().ok_or(Error::Missing)
    }
    /// Independently expires one model without releasing the source result.
    pub fn remove_viz_rows(&self, reference: &str) -> Result<(), Error> {
        self.viz_models
            .lock()
            .map_err(|_| Error::Unavailable)?
            .entries
            .remove(reference);
        Ok(())
    }
}
