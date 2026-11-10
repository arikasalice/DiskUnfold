//! Live-scan F06 model admission and row paging, without IPC registration.
//! A session index owns only weak source references; source results own row data.
pub mod production;
use du_analysis_index::{IngestSink, ResultVersion, visualization::live_models};
use du_core_contracts::{
    Id,
    result::ResultVersionRead,
    visualization::rows::{VizRow, VizRows, VizRowsRequest},
};
use du_job_runtime::JobIdentity;
use std::{
    collections::{HashMap, VecDeque},
    num::NonZeroUsize,
    sync::{Arc, Mutex, Weak},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidModel,
    InvalidRange,
    InvalidGroup,
    ModelExpired,
    StaleVersion,
    ReferenceCollision,
    Unavailable,
}
/// An already computed model from a pinned live result and normalized condition.
pub struct ModelInput {
    pub version: Arc<ResultVersion>,
    pub condition_fingerprint: String,
    pub rows: Vec<VizRow>,
    pub groups: HashMap<String, live_models::GroupRows>,
}
struct Entry {
    source: Weak<ResultVersion>,
    revision: Id,
    fingerprint: String,
}
#[derive(Default)]
struct State {
    entries: HashMap<String, Entry>,
    conditions: HashMap<(Id, String), String>,
    lru: VecDeque<String>,
}
/// Internal group page; this is not an IPC response type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupPage {
    pub rows: Vec<VizRow>,
    pub total_count: u64,
    pub group_total: du_core_contracts::SizeValue,
    pub size_range: live_models::SizeRange,
    pub result_revision: Id,
    pub observed_at: Option<du_core_contracts::Timestamp>,
    pub completeness: du_core_contracts::result::ResultCompleteness,
}
/// Bounded session index. IDs supplied by the identity port must never be reused in this session.
pub struct ModelPages {
    source: IngestSink,
    capacity: NonZeroUsize,
    ids: Arc<dyn JobIdentity>,
    state: Mutex<State>,
}
impl ModelPages {
    pub fn new(source: IngestSink, capacity: NonZeroUsize, ids: Arc<dyn JobIdentity>) -> Self {
        Self {
            source,
            capacity,
            ids,
            state: Mutex::new(State::default()),
        }
    }
    /// Pins and registers only a still-current live result. Cancelled results must not be supplied.
    pub fn register(&self, input: ModelInput) -> Result<String, Error> {
        validate_input(&input)?;
        let mut state = self.state.lock().map_err(|_| Error::Unavailable)?;
        let current = self
            .source
            .pin_current(input.version.version_id())
            .map_err(source_error)?;
        if !Arc::ptr_eq(&current, &input.version) {
            return Err(Error::StaleVersion);
        }
        let reference = self.ids.next_id().as_str().to_owned();
        let key = (
            input.version.version_id().clone(),
            input.condition_fingerprint.clone(),
        );
        prune(&mut state);
        if let Some(existing) = state.conditions.get(&key).cloned() {
            if !state
                .entries
                .get(&existing)
                .and_then(|entry| entry.source.upgrade())
                .is_some_and(|source| Arc::ptr_eq(&source, &current))
            {
                return Err(Error::StaleVersion);
            }
            touch(&mut state.lru, &existing);
            return Ok(existing);
        }
        if state.entries.contains_key(&reference) {
            return Err(Error::ReferenceCollision);
        }
        while state.entries.len() >= self.capacity.get() {
            let oldest = state.lru.front().cloned().ok_or(Error::Unavailable)?;
            remove(&mut state, &oldest)?;
        }
        current
            .insert_viz_rows(reference.clone(), input.rows, input.groups)
            .map_err(model_error)?;
        state.entries.insert(
            reference.clone(),
            Entry {
                source: Arc::downgrade(&current),
                revision: key.0.clone(),
                fingerprint: key.1.clone(),
            },
        );
        state.conditions.insert(key, reference.clone());
        touch(&mut state.lru, &reference);
        Ok(reference)
    }
    /// Admit the source and model together before copying the page without holding the index lock.
    pub fn rows(&self, request: &VizRowsRequest) -> Result<VizRows, Error> {
        self.rows_at_admission(request, || {})
    }
    fn rows_at_admission(
        &self,
        request: &VizRowsRequest,
        after_pin: impl FnOnce(),
    ) -> Result<VizRows, Error> {
        request.validate().map_err(|_| Error::InvalidRange)?;
        let mut state = self.state.lock().map_err(|_| Error::Unavailable)?;
        let current = self
            .source
            .pin_current(&request.result_revision)
            .map_err(source_error)?;
        let entry = state
            .entries
            .get(&request.model_ref)
            .ok_or(Error::ModelExpired)?;
        if entry.revision != request.result_revision {
            return Err(Error::StaleVersion);
        }
        let pinned = entry.source.upgrade().ok_or(Error::StaleVersion)?;
        if !Arc::ptr_eq(&pinned, &current) {
            return Err(Error::StaleVersion);
        }
        let rows = current
            .pin_viz_rows(&request.model_ref)
            .map_err(model_error)?;
        let start = request.start as usize;
        if start > rows.len() {
            return Err(Error::InvalidRange);
        }
        touch(&mut state.lru, &request.model_ref);
        drop(state);
        after_pin();
        let end = start.saturating_add(request.count as usize).min(rows.len());
        Ok(VizRows {
            rows: rows[start..end].to_vec(),
            total_count: rows.len() as u64,
            result_revision: current.version_id().clone(),
            observed_at: current.observed_at().cloned(),
            completeness: current.completeness(),
        })
    }
    /// Reads a model-local group's ordered members and unchanged aggregate evidence.
    pub fn group_rows(
        &self,
        model_ref: &str,
        group_ref: &str,
        start: u32,
        count: u32,
    ) -> Result<GroupPage, Error> {
        self.group_rows_at_admission(model_ref, group_ref, start, count, || {})
    }
    fn group_rows_at_admission(
        &self,
        model_ref: &str,
        group_ref: &str,
        start: u32,
        count: u32,
        after_pin: impl FnOnce(),
    ) -> Result<GroupPage, Error> {
        if model_ref.is_empty() || group_ref.is_empty() || count == 0 || count > 1000 {
            return Err(Error::InvalidRange);
        }
        let mut state = self.state.lock().map_err(|_| Error::Unavailable)?;
        let entry = state.entries.get(model_ref).ok_or(Error::ModelExpired)?;
        let current = self
            .source
            .pin_current(&entry.revision)
            .map_err(source_error)?;
        let pinned = entry.source.upgrade().ok_or(Error::StaleVersion)?;
        if !Arc::ptr_eq(&pinned, &current) {
            return Err(Error::StaleVersion);
        }
        let group = current
            .pin_viz_group(model_ref, group_ref)
            .map_err(model_error)?
            .ok_or(Error::InvalidGroup)?;
        let start = start as usize;
        if start > group.rows.len() {
            return Err(Error::InvalidRange);
        }
        touch(&mut state.lru, model_ref);
        drop(state);
        after_pin();
        let end = start.saturating_add(count as usize).min(group.rows.len());
        Ok(GroupPage {
            rows: group.rows[start..end].to_vec(),
            total_count: group.rows.len() as u64,
            group_total: group.total.clone(),
            size_range: group.size_range.clone(),
            result_revision: current.version_id().clone(),
            observed_at: current.observed_at().cloned(),
            completeness: current.completeness(),
        })
    }
    /// Evicts this model even if its source remains alive.
    pub fn release_model(&self, reference: &str) -> Result<(), Error> {
        let mut state = self.state.lock().map_err(|_| Error::Unavailable)?;
        remove(&mut state, reference)
    }
}
impl Drop for ModelPages {
    fn drop(&mut self) {
        if let Ok(state) = self.state.get_mut() {
            while let Some(reference) = state.lru.front().cloned() {
                if remove(state, &reference).is_err() {
                    break;
                }
            }
        }
    }
}
fn validate_input(input: &ModelInput) -> Result<(), Error> {
    if input.condition_fingerprint.is_empty() || input.groups.keys().any(String::is_empty) {
        return Err(Error::InvalidModel);
    }
    validate_rows(input, &input.rows)?;
    for group in input.groups.values() {
        validate_rows(input, &group.rows)?;
    }
    Ok(())
}

fn validate_rows(input: &ModelInput, rows: &[VizRow]) -> Result<(), Error> {
    if rows.len() as u64 >= (1_u64 << 53) {
        return Err(Error::InvalidModel);
    }
    for row in rows {
        row.validate_revision(input.version.version_id())
            .map_err(|_| Error::InvalidModel)?;
        if let Some(item) = &row.item_ref
            && input
                .version
                .items()
                .get(item.item_no as usize)
                .is_none_or(|found| found.item_no != item.item_no)
        {
            return Err(Error::InvalidModel);
        }
        if row
            .group_ref
            .as_ref()
            .is_some_and(|group| !input.groups.contains_key(group))
        {
            return Err(Error::InvalidModel);
        }
    }
    Ok(())
}
fn source_error(error: du_analysis_index::QueryError) -> Error {
    if error == du_analysis_index::QueryError::Unavailable {
        Error::Unavailable
    } else {
        Error::StaleVersion
    }
}
fn model_error(error: live_models::Error) -> Error {
    match error {
        live_models::Error::Unavailable => Error::Unavailable,
        live_models::Error::Duplicate => Error::ReferenceCollision,
        live_models::Error::Missing => Error::ModelExpired,
    }
}
fn touch(lru: &mut VecDeque<String>, key: &str) {
    lru.retain(|item| item != key);
    lru.push_back(key.to_owned());
}
fn prune(state: &mut State) {
    let dead: Vec<_> = state
        .entries
        .iter()
        .filter(|(_, entry)| entry.source.strong_count() == 0)
        .map(|(key, _)| key.clone())
        .collect();
    for key in dead {
        let _ = remove(state, &key);
    }
}
fn remove(state: &mut State, key: &str) -> Result<(), Error> {
    if let Some(entry) = state.entries.get(key)
        && let Some(source) = entry.source.upgrade()
    {
        source.remove_viz_rows(key).map_err(model_error)?;
    }
    if let Some(entry) = state.entries.remove(key) {
        state
            .conditions
            .remove(&(entry.revision, entry.fingerprint));
    }
    state.lru.retain(|item| item != key);
    Ok(())
}

#[cfg(test)]
mod tests;
