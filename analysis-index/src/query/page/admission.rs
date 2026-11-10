use super::*;

/// Proof that this registry pinned a source version at request admission.
#[derive(Clone)]
pub struct SourceAdmission {
    owner: Arc<()>,
    version: Arc<crate::ResultVersion>,
}
impl SourceAdmission {
    /// The immutable source pinned by this admission.
    pub fn version(&self) -> Arc<crate::ResultVersion> {
        self.version.clone()
    }
}

impl ResultSets {
    /// Pins the then-current source for one request before evaluation starts.
    pub fn admit_source(&self, expected: Option<&Id>) -> Result<SourceAdmission, PageError> {
        let state = self.state.lock().map_err(|_| PageError::Unavailable)?;
        let version = match expected {
            Some(id) => self.source.pin_current(id).map_err(|error| match error {
                QueryError::NotFound => PageError::NoSource,
                other => source_error(other),
            })?,
            None => self
                .source
                .pin()
                .map_err(|_| PageError::Unavailable)?
                .ok_or(PageError::NoSource)?,
        };
        if state.released_sources.contains(version.version_id()) {
            return Err(PageError::StaleVersion);
        }
        Ok(SourceAdmission {
            owner: self.identity.clone(),
            version,
        })
    }

    /// Publishes a completed admitted query and returns its already-admitted first page.
    /// Later page requests still check the current source and may be stale.
    pub fn register_admitted_folder(
        &self,
        admission: SourceAdmission,
        key: MultiFolderKey,
        id: Id,
        result: QueryResult,
        count: Option<u32>,
    ) -> Result<(Id, Page), PageError> {
        validate_result(&result)?;
        if !Arc::ptr_eq(&admission.owner, &self.identity)
            || !Arc::ptr_eq(&admission.version, &result.version)
            || result.row_kind != RowKind::Folder
            || result.version.version_id() != &key.version
            || result.folder != Some(key.folder)
            || result.condition != key.condition
            || result.sorts != key.sorts
        {
            return Err(PageError::InvalidResult);
        }
        let count = count.unwrap_or(DEFAULT_PAGE_SIZE);
        if count == 0 || count > MAX_PAGE_SIZE {
            return Err(PageError::InvalidRange);
        }
        let mut state = self.state.lock().map_err(|_| PageError::Unavailable)?;
        let current = if state.released_sources.contains(&key.version) {
            false
        } else {
            match self.source.pin_current(&key.version) {
                Ok(version) => Arc::ptr_eq(&version, &admission.version),
                Err(QueryError::Unavailable) => return Err(PageError::Unavailable),
                Err(_) => false,
            }
        };
        if !current {
            if !state.used.insert(id.clone()) {
                return Err(PageError::DuplicateId);
            }
            return Ok((id.clone(), first_page(id, Arc::new(result), count)));
        }
        if let Some(existing) = state.folder_id(&key) {
            let result = state
                .entries
                .get(&existing)
                .ok_or(PageError::Unavailable)?
                .result
                .clone();
            if !Arc::ptr_eq(&result.version, &admission.version) {
                return Err(PageError::InvalidResult);
            }
            touch(&mut state.lru, &existing);
            return Ok((existing.clone(), first_page(existing, result, count)));
        }
        if state.used.contains(&id) {
            return Err(PageError::DuplicateId);
        }
        let result = state.insert(id.clone(), result, Some(key));
        Ok((id.clone(), first_page(id, result, count)))
    }
}

fn first_page(query: Id, result: Arc<QueryResult>, count: u32) -> Page {
    let end = usize::try_from(count)
        .unwrap_or(usize::MAX)
        .min(result.items.len());
    Page {
        query,
        result,
        range: 0..end,
    }
}

/// One registry-bound admission of a source and optional parent search.
/// Both snapshots remain immutable even if the source is subsequently replaced.
#[derive(Clone)]
pub struct SearchAdmission {
    source: SourceAdmission,
    parent: Option<Arc<QueryResult>>,
}

impl SearchAdmission {
    pub(crate) fn belongs_to(&self, sets: &ResultSets) -> bool {
        Arc::ptr_eq(&self.source.owner, &sets.identity)
    }
    /// Returns the pinned source selected for the search.
    pub fn version(&self) -> Arc<crate::ResultVersion> {
        self.source.version()
    }
    /// Returns the admitted parent, if refinement was requested.
    pub fn parent(&self) -> Option<&Arc<QueryResult>> {
        self.parent.as_ref()
    }
}

/// A fully checked immutable candidate. Its contents cannot be changed after validation.
pub struct ValidatedSearch {
    admission: SearchAdmission,
    result: QueryResult,
    count: u32,
}

impl ResultSets {
    /// Atomically admits a current source and an optional search parent from this registry.
    /// An explicit expected version must agree with the parent's version.
    pub fn admit_search(
        &self,
        expected: Option<&Id>,
        parent: Option<&Id>,
    ) -> Result<SearchAdmission, PageError> {
        let mut state = self.state.lock().map_err(|_| PageError::Unavailable)?;
        let parent_result = if let Some(id) = parent {
            let entry = state.entries.get(id).ok_or(PageError::StaleVersion)?;
            let version = entry.result.version.version_id();
            if expected.is_some_and(|expected| expected != version) {
                return Err(PageError::StaleVersion);
            }
            let result = self.pin_entry(&state, id, version)?;
            if result.row_kind != RowKind::Search {
                return Err(PageError::InvalidResult);
            }
            touch(&mut state.lru, id);
            Some(result)
        } else {
            None
        };
        let version = if let Some(parent) = &parent_result {
            Arc::clone(&parent.version)
        } else {
            match expected {
                Some(id) => self.source.pin_current(id).map_err(|e| match e {
                    QueryError::NotFound => PageError::NoSource,
                    other => source_error(other),
                })?,
                None => self
                    .source
                    .pin()
                    .map_err(|_| PageError::Unavailable)?
                    .ok_or(PageError::NoSource)?,
            }
        };
        if state.released_sources.contains(version.version_id()) {
            return Err(PageError::StaleVersion);
        }
        Ok(SearchAdmission {
            source: SourceAdmission {
                owner: self.identity.clone(),
                version,
            },
            parent: parent_result,
        })
    }

    /// Checks the entire candidate before entering a region's publication lock.
    /// This also prevents a refinement from adding rows outside its admitted parent.
    pub fn validate_search_candidate(
        &self,
        admission: SearchAdmission,
        result: QueryResult,
        count: Option<u32>,
    ) -> Result<ValidatedSearch, PageError> {
        if !Arc::ptr_eq(&admission.source.owner, &self.identity)
            || !Arc::ptr_eq(&admission.source.version, &result.version)
            || result.row_kind != RowKind::Search
        {
            return Err(PageError::InvalidResult);
        }
        let count = count.unwrap_or(DEFAULT_PAGE_SIZE);
        if count == 0 || count > MAX_PAGE_SIZE {
            return Err(PageError::InvalidRange);
        }
        validate_result(&result)?;
        if let Some(parent) = &admission.parent {
            let allowed: HashSet<_> = parent.items.iter().map(|item| item.item_no).collect();
            if result
                .items
                .iter()
                .any(|item| !allowed.contains(&item.item_no))
            {
                return Err(PageError::InvalidResult);
            }
        }
        Ok(ValidatedSearch {
            admission,
            result,
            count,
        })
    }

    /// Publishes a previously checked candidate and returns its already-admitted first page.
    /// The caller must serialize this operation with region replacement, close and cancellation.
    /// If the source has retired, the page remains readable but is not registered; subsequent
    /// page admissions for its reference fail as stale. This operation does not deduplicate.
    pub fn register_admitted_search(
        &self,
        id: Id,
        candidate: ValidatedSearch,
    ) -> Result<Page, PageError> {
        let ValidatedSearch {
            admission,
            result,
            count,
        } = candidate;
        if !Arc::ptr_eq(&admission.source.owner, &self.identity) {
            return Err(PageError::InvalidResult);
        }
        let mut state = self.state.lock().map_err(|_| PageError::Unavailable)?;
        if state.used.contains(&id) {
            return Err(PageError::DuplicateId);
        }
        let version = admission.source.version.version_id();
        let current = if state.released_sources.contains(version) {
            false
        } else {
            match self.source.pin_current(version) {
                Ok(current) => Arc::ptr_eq(&current, &admission.source.version),
                Err(QueryError::Unavailable) => return Err(PageError::Unavailable),
                Err(_) => false,
            }
        };
        let result = if current {
            state.insert(id.clone(), result, None)
        } else {
            state.used.insert(id.clone());
            Arc::new(result)
        };
        Ok(first_page(id, result, count))
    }
}
