//! Live-scan F06 model admission and row paging, without IPC registration.
//! A session index owns only weak source references; source results own row data.
pub mod channels;
pub mod commands;
mod deferred;
pub mod histogram;
mod histogram_fingerprint;
use deferred::DeferredRows;
mod locate;
pub mod production;
pub mod render;
pub mod session;
use du_analysis_index::{
    IngestSink, ResultVersion,
    visualization::{group_page, live_models, projection},
};
use du_core_contracts::{
    Id, ItemRef,
    result::ResultVersionRead,
    visualization::rows::{MAX_PAGE_COUNT, VizRow, VizRows, VizRowsRequest},
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
    ItemUnavailable,
    ModelExpired,
    StaleVersion,
    ReferenceCollision,
    Unavailable,
    Cancelled,
}
/// An already computed model from a pinned live result and normalized condition.
pub struct ModelInput {
    pub version: Arc<ResultVersion>,
    pub condition_fingerprint: String,
    pub rows: Vec<VizRow>,
    pub groups: HashMap<String, live_models::GroupRows>,
}
struct PreparedModelInput {
    version: Arc<ResultVersion>,
    condition_fingerprint: String,
    rows: Option<live_models::PreparedRows>,
    scope: Option<ItemRef>,
}

fn prepare_model_input(input: ModelInput) -> Result<PreparedModelInput, Error> {
    validate_input(&input)?;
    Ok(PreparedModelInput {
        version: input.version,
        condition_fingerprint: input.condition_fingerprint,
        rows: Some(live_models::PreparedRows::new(input.rows, input.groups)),
        scope: None,
    })
}
fn prepare_scoped_model_input(
    input: ModelInput,
    scope: &ItemRef,
) -> Result<PreparedModelInput, Error> {
    let mut result = prepare_model_input(input)?;
    result.scope = Some(scope.clone());
    Ok(result)
}
struct Entry {
    source: Weak<ResultVersion>,
    revision: Id,
    fingerprint: String,
    layout: Option<Arc<projection::Model>>,
    scope: Option<ItemRef>,
}
#[derive(Default)]
struct State {
    entries: HashMap<String, Entry>,
    conditions: HashMap<(Id, String), String>,
    lru: VecDeque<String>,
}
pub use group_page::Page as GroupPage;
/// Bounded session index. IDs supplied by the identity port must never be reused in this session.
pub struct ModelPages {
    source: IngestSink,
    identity: Arc<()>,
    capacity: NonZeroUsize,
    ids: Arc<dyn JobIdentity>,
    state: Mutex<State>,
}
impl ModelPages {
    pub fn new(source: IngestSink, capacity: NonZeroUsize, ids: Arc<dyn JobIdentity>) -> Self {
        Self {
            source,
            identity: Arc::new(()),
            capacity,
            ids,
            state: Mutex::new(State::default()),
        }
    }
    /// Pins and registers only a still-current live result. Cancelled results must not be supplied.
    pub fn register(&self, input: ModelInput) -> Result<String, Error> {
        self.register_with_layout(input, None, &mut || false)
            .map(|(reference, _)| reference)
    }

    // Lock order is model session, result publication store, then model rows.
    // Row validation and storage preparation happen before these locks; detached
    // rows and layouts are destroyed after them. Publication still waits for
    // lookups, allocations/rehashing, at most one eviction and O(capacity) LRU
    // updates; source-wide table and lineage counts have no configured bound.
    // The cancellation callback also runs under the publication lock just before
    // any cache mutation. It must be nonblocking and must not reenter either the
    // source store or model session. The identity port runs before these locks.
    fn register_with_layout(
        &self,
        input: ModelInput,
        layout: Option<Arc<projection::Model>>,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<(String, Option<Arc<projection::Model>>), Error> {
        self.register_after_prepare(input, layout, cancelled, || {})
    }

    fn register_after_prepare(
        &self,
        input: ModelInput,
        layout: Option<Arc<projection::Model>>,
        cancelled: &mut dyn FnMut() -> bool,
        during_prepare: impl FnOnce(),
    ) -> Result<(String, Option<Arc<projection::Model>>), Error> {
        let input = prepare_model_input(input)?;
        during_prepare();
        self.register_prepared(input, layout, cancelled)
    }

    fn register_prepared(
        &self,
        input: PreparedModelInput,
        layout: Option<Arc<projection::Model>>,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<(String, Option<Arc<projection::Model>>), Error> {
        let reference = self.ids.next_id().as_str().to_owned();
        self.register_prepared_with_reference(input, layout, cancelled, reference)
    }

    // The reference is reserved before any outer job-publication gate is held.
    fn register_prepared_with_reference(
        &self,
        input: PreparedModelInput,
        layout: Option<Arc<projection::Model>>,
        cancelled: &mut dyn FnMut() -> bool,
        reference: String,
    ) -> Result<(String, Option<Arc<projection::Model>>), Error> {
        let PreparedModelInput {
            version,
            condition_fingerprint,
            rows,
            scope,
        } = input;
        let key = (version.version_id().clone(), condition_fingerprint);
        let mut rows = rows;
        let mut state = self.state.lock().map_err(|_| Error::Unavailable)?;
        if cancelled() {
            return Err(Error::Cancelled);
        }
        let mut retired = None;
        let mut rollback = None;
        let mut retired_source = None;
        let result = self
            .source
            .with_current(&version, || {
                if cancelled() {
                    return Err(Error::Cancelled);
                }
                let current = &version;
                if let Some(existing) = state.conditions.get(&key).cloned() {
                    let entry = state.entries.get(&existing).ok_or(Error::Unavailable)?;
                    if !entry
                        .source
                        .upgrade()
                        .is_some_and(|source| Arc::ptr_eq(&source, current))
                    {
                        return Err(Error::StaleVersion);
                    }
                    if layout.is_some() && entry.layout.is_none() {
                        return Err(Error::InvalidModel);
                    }
                    if entry.scope != scope {
                        return Err(Error::InvalidModel);
                    }
                    let layout = entry.layout.clone();
                    touch(&mut state.lru, &existing);
                    return Ok((existing, layout));
                }
                if state.entries.contains_key(&reference) {
                    return Err(Error::ReferenceCollision);
                }
                if state.entries.len() > self.capacity.get() {
                    return Err(Error::Unavailable);
                }
                state
                    .entries
                    .try_reserve(1)
                    .map_err(|_| Error::Unavailable)?;
                state
                    .conditions
                    .try_reserve(1)
                    .map_err(|_| Error::Unavailable)?;
                state.lru.try_reserve(1).map_err(|_| Error::Unavailable)?;
                current
                    .insert_prepared_viz_rows(reference.clone(), &mut rows)
                    .map_err(model_error)?;
                while state.entries.len() >= self.capacity.get() {
                    let result = state
                        .lru
                        .iter()
                        .find(|reference| {
                            state
                                .entries
                                .get(*reference)
                                .is_some_and(|entry| entry.source.strong_count() == 0)
                        })
                        .or_else(|| state.lru.front())
                        .cloned()
                        .ok_or(Error::Unavailable)
                        .and_then(|oldest| {
                            remove_deferred(&mut state, &oldest, &mut retired_source)
                        });
                    match result {
                        Ok(old) => retired = Some(old),
                        Err(error) => {
                            rollback = current.retire_viz_rows(&reference).ok();
                            return Err(error);
                        }
                    }
                }
                state.entries.insert(
                    reference.clone(),
                    Entry {
                        source: Arc::downgrade(current),
                        revision: key.0.clone(),
                        fingerprint: key.1.clone(),
                        layout: layout.clone(),
                        scope: scope.clone(),
                    },
                );
                state.conditions.insert(key.clone(), reference.clone());
                touch(&mut state.lru, &reference);
                Ok((reference.clone(), layout.clone()))
            })
            .map_err(source_error);
        drop(state);
        drop(retired);
        drop(retired_source);
        drop(rollback);
        drop(rows);
        result?
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
        self.group_rows_cancellable(model_ref, group_ref, start, count, &mut || false)
    }

    /// Reads a fixed group page with cooperative cancellation during row copying.
    pub fn group_rows_cancellable(
        &self,
        model_ref: &str,
        group_ref: &str,
        start: u32,
        count: u32,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<GroupPage, Error> {
        self.group_rows_at_admission(model_ref, group_ref, start, count, || {}, cancelled)
    }
    fn group_rows_at_admission(
        &self,
        model_ref: &str,
        group_ref: &str,
        start: u32,
        count: u32,
        after_pin: impl FnOnce(),
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<GroupPage, Error> {
        if model_ref.is_empty() || group_ref.is_empty() || count == 0 || count > MAX_PAGE_COUNT {
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
        let group = group_page::PinnedGroup::from_admitted(current, model_ref, group_ref)
            .map_err(group_error)?;
        if start as usize > group.row_count() {
            return Err(Error::InvalidRange);
        }
        touch(&mut state.lru, model_ref);
        drop(state);
        after_pin();
        group.page(start, count, cancelled).map_err(group_error)
    }
    /// Evicts this model even if its source remains alive.
    pub fn release_model(&self, reference: &str) -> Result<(), Error> {
        let mut source = None;
        let mut state = self.state.lock().map_err(|_| Error::Unavailable)?;
        let retired = remove_deferred(&mut state, reference, &mut source);
        drop(state);
        drop(source);
        retired.map(drop)
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
fn group_error(error: group_page::Error) -> Error {
    match error {
        group_page::Error::ModelUnavailable => Error::Unavailable,
        group_page::Error::ModelExpired => Error::ModelExpired,
        group_page::Error::GroupMissing => Error::InvalidGroup,
        group_page::Error::InvalidRange => Error::InvalidRange,
        group_page::Error::Cancelled => Error::Cancelled,
    }
}
fn touch(lru: &mut VecDeque<String>, key: &str) {
    lru.retain(|item| item != key);
    lru.push_back(key.to_owned());
}
fn remove(state: &mut State, key: &str) -> Result<(), Error> {
    let mut source = None;
    drop(remove_deferred(state, key, &mut source)?);
    Ok(())
}
fn remove_deferred(
    state: &mut State,
    key: &str,
    retained_source: &mut Option<Arc<ResultVersion>>,
) -> Result<(Option<Entry>, Option<live_models::RetiredRows>), Error> {
    let mut rows = None;
    *retained_source = state
        .entries
        .get(key)
        .and_then(|entry| entry.source.upgrade());
    if let Some(source) = retained_source.as_ref() {
        rows = Some(source.retire_viz_rows(key).map_err(model_error)?);
    }
    let entry = state.entries.remove(key);
    if let Some(entry) = &entry {
        state
            .conditions
            .remove(&(entry.revision.clone(), entry.fingerprint.clone()));
    }
    state.lru.retain(|item| item != key);
    Ok((entry, rows))
}

#[cfg(test)]
mod tests;
