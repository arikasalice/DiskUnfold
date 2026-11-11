//! Caller-owned cleanup for bounded model registration.
use super::*;

#[derive(Default)]
pub(super) struct DeferredRows {
    retired: Option<(Option<Entry>, Option<live_models::RetiredRows>)>,
    pub(super) rollback: Option<live_models::RetiredRows>,
    retired_source: Option<Arc<ResultVersion>>,
}

impl ModelPages {
    // This variant borrows the candidate and stores all potentially large retired
    // allocations in caller-owned storage. Its owner must outlive the F24 gate.
    pub(super) fn register_deferred(
        &self,
        input: &mut PreparedModelInput,
        layouts: &(Option<Arc<projection::Model>>, Option<LayoutMetadata>),
        reference: &str,
        cancelled: &mut dyn FnMut() -> bool,
        cleanup: &mut DeferredRows,
    ) -> Result<(String, Option<Arc<projection::Model>>), Error> {
        let (layout, metadata) = layouts;
        let version = &input.version;
        let key = (
            version.version_id().clone(),
            input.condition_fingerprint.clone(),
        );
        let mut state = self.state.lock().map_err(|_| Error::Unavailable)?;
        if cancelled() {
            return Err(Error::Cancelled);
        }
        self.source
            .with_current(version, || {
                if cancelled() {
                    return Err(Error::Cancelled);
                }
                if let Some(existing) = state.conditions.get(&key).cloned() {
                    let entry = state.entries.get(&existing).ok_or(Error::Unavailable)?;
                    cleanup.retired_source = entry.source.upgrade();
                    if !cleanup
                        .retired_source
                        .as_ref()
                        .is_some_and(|source| Arc::ptr_eq(source, version))
                    {
                        return Err(Error::StaleVersion);
                    }
                    if layout.is_some() && entry.layout.is_none() {
                        return Err(Error::InvalidModel);
                    }
                    if metadata.as_deref().map(Any::type_id)
                        != entry.metadata.as_deref().map(Any::type_id)
                    {
                        return Err(Error::InvalidModel);
                    }
                    if entry.scope != input.scope {
                        return Err(Error::InvalidModel);
                    }
                    let layout = entry.layout.clone();
                    touch(&mut state.lru, &existing);
                    return Ok((existing, layout));
                }
                if state.entries.contains_key(reference) {
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
                version
                    .insert_prepared_viz_rows(reference.to_owned(), &mut input.rows)
                    .map_err(model_error)?;
                if state.entries.len() >= self.capacity.get() {
                    let oldest = state
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
                        .ok_or(Error::Unavailable);
                    let removed = oldest.and_then(|oldest| {
                        remove_deferred(&mut state, &oldest, &mut cleanup.retired_source)
                    });
                    match removed {
                        Ok(retired) => cleanup.retired = Some(retired),
                        Err(error) => {
                            cleanup.rollback = version.retire_viz_rows(reference).ok();
                            return Err(error);
                        }
                    }
                }
                state.entries.insert(
                    reference.to_owned(),
                    Entry {
                        source: Arc::downgrade(version),
                        revision: key.0.clone(),
                        fingerprint: key.1.clone(),
                        layout: layout.clone(),
                        metadata: metadata.clone(),
                        scope: input.scope.clone(),
                    },
                );
                state.conditions.insert(key, reference.to_owned());
                touch(&mut state.lru, reference);
                Ok((reference.to_owned(), layout.clone()))
            })
            .map_err(source_error)?
    }

    /// Reads strategy-owned metadata only while its model source is current.
    pub(super) fn layout_metadata<T: Any + Send + Sync>(
        &self,
        reference: &str,
    ) -> Result<Arc<T>, Error> {
        let state = self.state.lock().map_err(|_| Error::Unavailable)?;
        let entry = state.entries.get(reference).ok_or(Error::ModelExpired)?;
        let current = self
            .source
            .pin_current(&entry.revision)
            .map_err(source_error)?;
        if !entry
            .source
            .upgrade()
            .is_some_and(|source| Arc::ptr_eq(&source, &current))
        {
            return Err(Error::StaleVersion);
        }
        entry
            .metadata
            .clone()
            .and_then(|metadata| metadata.downcast().ok())
            .ok_or(Error::InvalidModel)
    }
}
