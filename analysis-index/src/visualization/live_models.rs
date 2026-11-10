//! Row models owned by their immutable live scan result, never by the session index.
use crate::ResultVersion;
use du_core_contracts::{SizeValue, visualization::rows::VizRow};
use std::{collections::HashMap, sync::Arc};

#[derive(Debug, Default)]
pub(crate) struct OwnedModels {
    entries: HashMap<String, Arc<ModelRows>>,
}
/// An observed size range; unknown is not an exact zero.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SizeRange {
    Known { min: SizeValue, max: SizeValue },
    Unknown,
}
/// Ordered members and aggregate evidence produced with the same pinned model.
/// The producer must provide only members of this group; values are not recomputed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupRows {
    pub rows: Vec<VizRow>,
    pub total: SizeValue,
    pub size_range: SizeRange,
}
#[derive(Debug)]
struct ModelRows {
    rows: Arc<[VizRow]>,
    groups: HashMap<String, Arc<GroupRows>>,
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
    pub fn insert_viz_rows(
        &self,
        reference: String,
        rows: Vec<VizRow>,
        groups: HashMap<String, GroupRows>,
    ) -> Result<(), Error> {
        let mut cache = self.viz_models.lock().map_err(|_| Error::Unavailable)?;
        if cache.entries.contains_key(&reference) {
            return Err(Error::Duplicate);
        }
        cache.entries.insert(
            reference,
            Arc::new(ModelRows {
                rows: rows.into(),
                groups: groups
                    .into_iter()
                    .map(|(key, group)| (key, Arc::new(group)))
                    .collect(),
            }),
        );
        Ok(())
    }
    /// Pins the model rows so an admitted read can finish after independent eviction.
    pub fn pin_viz_rows(&self, reference: &str) -> Result<Arc<[VizRow]>, Error> {
        let cache = self.viz_models.lock().map_err(|_| Error::Unavailable)?;
        cache
            .entries
            .get(reference)
            .map(|model| model.rows.clone())
            .ok_or(Error::Missing)
    }
    /// Pins members and the exact producer-supplied group evidence in one model.
    pub fn pin_viz_group(
        &self,
        reference: &str,
        group: &str,
    ) -> Result<Option<Arc<GroupRows>>, Error> {
        let cache = self.viz_models.lock().map_err(|_| Error::Unavailable)?;
        let model = cache.entries.get(reference).ok_or(Error::Missing)?;
        Ok(model.groups.get(group).cloned())
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
