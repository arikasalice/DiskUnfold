//! Row models owned by their immutable live scan result, never by the session index.
use crate::ResultVersion;
use du_core_contracts::{SizeAmount, SizeEvidence, SizeValue, visualization::rows::VizRow};
use std::{collections::HashMap, sync::Arc};

#[derive(Debug, Default)]
pub(crate) struct OwnedModels {
    entries: HashMap<String, ModelRows>,
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
/// Complete row storage built without holding the result cache lock.
pub struct PreparedRows(ModelRows);
/// Detached row storage; its destruction may occur after all cache locks are released.
pub struct RetiredRows {
    _rows: Option<ModelRows>,
}
impl PreparedRows {
    /// Builds the immutable row and group representation outside the result lock.
    pub fn new(rows: Vec<VizRow>, groups: HashMap<String, GroupRows>) -> Self {
        Self(ModelRows {
            rows: rows.into(),
            groups: groups
                .into_iter()
                .map(|(key, mut group)| {
                    guard_group_size_range(&mut group);
                    (key, Arc::new(group))
                })
                .collect(),
        })
    }
}

/// Preserve producer range values, but withhold a known range when the owned
/// members themselves show an unobserved size or mixed size basis.
fn guard_group_size_range(group: &mut GroupRows) {
    let SizeRange::Known { min, max } = &group.size_range else {
        return;
    };
    let expected_basis = &min.basis;
    let all_observed_in_one_basis = min.basis == max.basis
        && group.rows.iter().all(|row| {
            row.size.basis == *expected_basis
                && row.size.evidence != SizeEvidence::Unknown
                && matches!(row.size.amount, SizeAmount::Exact { .. })
        });
    if !all_observed_in_one_basis {
        group.size_range = SizeRange::Unknown;
    }
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
        let mut prepared = Some(PreparedRows::new(rows, groups));
        self.insert_prepared_viz_rows(reference, &mut prepared)
    }
    /// Installs prepared rows, leaving them with the caller on failure.
    pub fn insert_prepared_viz_rows(
        &self,
        reference: String,
        prepared: &mut Option<PreparedRows>,
    ) -> Result<(), Error> {
        let mut cache = self.viz_models.lock().map_err(|_| Error::Unavailable)?;
        if cache.entries.contains_key(&reference) {
            return Err(Error::Duplicate);
        }
        cache
            .entries
            .try_reserve(1)
            .map_err(|_| Error::Unavailable)?;
        let rows = prepared.take().ok_or(Error::Missing)?;
        cache.entries.insert(reference, rows.0);
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
        drop(self.retire_viz_rows(reference)?);
        Ok(())
    }
    /// Detaches rows without destroying them while the result cache is locked.
    pub fn retire_viz_rows(&self, reference: &str) -> Result<RetiredRows, Error> {
        let rows = self
            .viz_models
            .lock()
            .map_err(|_| Error::Unavailable)?
            .entries
            .remove(reference);
        Ok(RetiredRows { _rows: rows })
    }
}
