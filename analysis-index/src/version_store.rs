//! Session result lineages and current-version ownership.
use crate::{BuildError, QueryError, ResultVersion};
use du_core_contracts::Id;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

type Lineage = Option<Id>;

#[derive(Default)]
pub(super) struct State {
    lineages: HashMap<String, Id>,
    current: HashMap<Lineage, Arc<ResultVersion>>,
    focus: Option<Lineage>,
    active: HashSet<Lineage>,
    used_versions: HashSet<Id>,
}
impl State {
    pub(super) fn bind(&mut self, fingerprint: &str, candidate: Id) -> Result<Id, BuildError> {
        if let Some(id) = self.lineages.get(fingerprint) {
            return Ok(id.clone());
        }
        if self.lineages.values().any(|id| id == &candidate) {
            return Err(BuildError::DuplicateVersion);
        }
        self.lineages
            .insert(fingerprint.to_owned(), candidate.clone());
        Ok(candidate)
    }
    pub(super) fn pin(&self) -> Option<Arc<ResultVersion>> {
        self.focus
            .as_ref()
            .and_then(|key| self.current.get(key))
            .cloned()
    }
    pub(super) fn pin_current(&self, expected: &Id) -> Result<Arc<ResultVersion>, QueryError> {
        if let Some(result) = self
            .current
            .values()
            .find(|value| value.version_id == *expected)
        {
            return Ok(Arc::clone(result));
        }
        if self.current.is_empty() {
            Err(QueryError::NotFound)
        } else {
            Err(QueryError::StaleVersion)
        }
    }
    pub(super) fn begin(
        &mut self,
        lineage: &Lineage,
    ) -> Result<Option<Arc<ResultVersion>>, BuildError> {
        if lineage
            .as_ref()
            .is_some_and(|id| !self.lineages.values().any(|bound| bound == id))
        {
            return Err(BuildError::Unavailable);
        }
        if !self.active.insert(lineage.clone()) {
            return Err(BuildError::Busy);
        }
        Ok(self.current.get(lineage).cloned())
    }
    pub(super) fn publish(
        &mut self,
        lineage: &Lineage,
        result: Arc<ResultVersion>,
        finish: bool,
    ) -> Result<(Option<Lineage>, Option<Arc<ResultVersion>>), BuildError> {
        if !self.used_versions.insert(result.version_id.clone()) {
            return Err(BuildError::DuplicateVersion);
        }
        let before = self.focus.replace(lineage.clone());
        let previous = self.current.insert(lineage.clone(), result);
        if finish {
            self.active.remove(lineage);
        }
        Ok((before, previous))
    }
    pub(super) fn abort(
        &mut self,
        lineage: &Lineage,
        baseline: Option<Arc<ResultVersion>>,
        published: bool,
        before: Option<Lineage>,
    ) -> Option<Arc<ResultVersion>> {
        self.active.remove(lineage);
        if !published {
            return None;
        }
        let previous = if let Some(baseline) = baseline {
            self.current.insert(lineage.clone(), baseline)
        } else {
            self.current.remove(lineage)
        };
        if self.focus.as_ref() == Some(lineage) {
            self.focus = before.filter(|key| self.current.contains_key(key));
        }
        previous
    }
}
