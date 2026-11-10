use super::*;

/// Proof that this registry pinned a source version at request admission.
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
            return Ok((id, first_page(Arc::new(result), count)));
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
            return Ok((existing, first_page(result, count)));
        }
        if state.used.contains(&id) {
            return Err(PageError::DuplicateId);
        }
        let result = state.insert(id.clone(), result, Some(key));
        Ok((id, first_page(result, count)))
    }
}

fn first_page(result: Arc<QueryResult>, count: u32) -> Page {
    let end = usize::try_from(count)
        .unwrap_or(usize::MAX)
        .min(result.items.len());
    Page {
        result,
        range: 0..end,
    }
}
