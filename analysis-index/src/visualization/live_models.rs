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
    /// The child rows of each expanded folder, by the shape reference of the folder.
    tables: HashMap<String, Arc<[VizRow]>>,
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
        let mut prepared_groups = HashMap::with_capacity(groups.len());
        for (key, mut group) in groups {
            let _ = guard_group_size_range(&mut group, &mut None);
            prepared_groups.insert(key, Arc::new(group));
        }
        Self(ModelRows {
            rows: rows.into(),
            groups: prepared_groups,
            tables: HashMap::new(),
        })
    }

    /// Builds row storage while allowing a caller to abandon a large preparation.
    pub fn new_cancellable(
        rows: Vec<VizRow>,
        groups: HashMap<String, GroupRows>,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<Self, PrepareError> {
        build_model_rows(rows, groups, Some(cancelled)).map(Self)
    }

    /// Adds a group built after the rows were prepared, such as the group of a folder
    /// opened before publication. The key must be new to this model; the group's size
    /// range is checked against its members exactly as for prepared groups.
    pub fn add_group(&mut self, key: String, mut group: GroupRows) -> Result<(), Error> {
        if key.is_empty() || self.0.groups.contains_key(&key) {
            return Err(Error::Duplicate);
        }
        let _ = guard_group_size_range(&mut group, &mut None);
        self.0.groups.insert(key, Arc::new(group));
        Ok(())
    }

    /// Adds the table of an expanded folder: its drawn children and the row of its
    /// group, if it has one, in the order they are shown. The key is the shape
    /// reference of the folder and must be new to this model.
    pub fn add_table(&mut self, key: String, rows: Vec<VizRow>) -> Result<(), Error> {
        if key.is_empty() || self.0.tables.contains_key(&key) {
            return Err(Error::Duplicate);
        }
        self.0.tables.insert(key, rows.into());
        Ok(())
    }
}

fn build_model_rows(
    rows: Vec<VizRow>,
    groups: HashMap<String, GroupRows>,
    mut cancelled: Option<&mut dyn FnMut() -> bool>,
) -> Result<ModelRows, PrepareError> {
    let mut prepared_groups = HashMap::with_capacity(groups.len());
    for (key, mut group) in groups {
        if cancelled.as_mut().is_some_and(|check| check()) {
            return Err(PrepareError::Cancelled);
        }
        if guard_group_size_range(&mut group, &mut cancelled) {
            return Err(PrepareError::Cancelled);
        }
        prepared_groups.insert(key, Arc::new(group));
    }
    Ok(ModelRows {
        rows: rows.into(),
        groups: prepared_groups,
        tables: HashMap::new(),
    })
}

/// Preserve producer range values, but withhold a known range when the owned
/// members themselves show an unobserved size or mixed size basis.
fn guard_group_size_range(
    group: &mut GroupRows,
    cancelled: &mut Option<&mut dyn FnMut() -> bool>,
) -> bool {
    let SizeRange::Known { min, max } = &group.size_range else {
        // No member traversal is needed when the producer has no range claim.
        return false;
    };
    let expected_basis = &min.basis;
    let mut observed_min = None;
    let mut observed_max = None;
    let (low, high) = (exact_bytes(min), exact_bytes(max));
    let mut valid = min.basis == max.basis
        && min.basis == group.total.basis
        && min.evidence != SizeEvidence::Unknown
        && max.evidence != SizeEvidence::Unknown
        && matches!((low, high), (Some(low), Some(high)) if low <= high)
        && !group.rows.is_empty();
    for row in &group.rows {
        if cancelled.as_mut().is_some_and(|check| check()) {
            return true;
        }
        if row.size.basis != *expected_basis || row.size.evidence == SizeEvidence::Unknown {
            valid = false;
            continue;
        }
        let Some(bytes) = exact_bytes(&row.size) else {
            valid = false;
            continue;
        };
        observed_min = Some(observed_min.map_or(bytes, |current: u64| current.min(bytes)));
        observed_max = Some(observed_max.map_or(bytes, |current: u64| current.max(bytes)));
    }
    if low.is_some_and(|low| observed_min != Some(low))
        || high.is_some_and(|high| observed_max != Some(high))
    {
        valid = false;
    }
    if !valid {
        group.size_range = SizeRange::Unknown;
    }
    false
}

fn exact_bytes(size: &SizeValue) -> Option<u64> {
    match &size.amount {
        SizeAmount::Exact { bytes } => Some(bytes.get()),
        SizeAmount::LowerBound { .. } | SizeAmount::Unknown => None,
    }
}

/// Preparation failures are separate from errors while storing a prepared model.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrepareError {
    Cancelled,
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
    /// Pins the table of an expanded folder, or none when the folder has no table.
    pub fn pin_viz_table(
        &self,
        reference: &str,
        folder_shape: &str,
    ) -> Result<Option<Arc<[VizRow]>>, Error> {
        let cache = self.viz_models.lock().map_err(|_| Error::Unavailable)?;
        let model = cache.entries.get(reference).ok_or(Error::Missing)?;
        Ok(model.tables.get(folder_shape).cloned())
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
