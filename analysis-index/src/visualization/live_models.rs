//! Row models owned by their immutable live scan result, never by the session index.
use crate::ResultVersion;
use du_core_contracts::{
    result::ResultVersionRead,
    visualization::rows::{VizRow, VizRows},
};
use std::collections::HashMap;

#[derive(Debug, Default)]
pub(crate) struct OwnedModels {
    entries: HashMap<String, Vec<VizRow>>,
}
/// Internal source-owned model storage failures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Unavailable,
    Duplicate,
    Missing,
    InvalidRange,
}
impl ResultVersion {
    /// Stores already validated rows in this version. The reference must be session-unique.
    pub fn insert_viz_rows(&self, reference: String, rows: Vec<VizRow>) -> Result<(), Error> {
        let mut cache = self.viz_models.lock().map_err(|_| Error::Unavailable)?;
        if cache.entries.contains_key(&reference) {
            return Err(Error::Duplicate);
        }
        cache.entries.insert(reference, rows);
        Ok(())
    }
    /// Copies one admitted page while retaining this pinned source for the call.
    pub fn viz_rows(&self, reference: &str, start: u32, count: u32) -> Result<VizRows, Error> {
        let cache = self.viz_models.lock().map_err(|_| Error::Unavailable)?;
        let rows = cache.entries.get(reference).ok_or(Error::Missing)?;
        let start = start as usize;
        if start > rows.len() {
            return Err(Error::InvalidRange);
        }
        let end = start.saturating_add(count as usize).min(rows.len());
        Ok(VizRows {
            rows: rows[start..end].to_vec(),
            total_count: rows.len() as u64,
            result_revision: self.version_id().clone(),
            observed_at: self.observed_at().cloned(),
            completeness: self.completeness(),
        })
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
