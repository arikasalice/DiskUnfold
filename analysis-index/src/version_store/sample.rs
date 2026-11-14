//! Sample lineages inside the result store: registration, publication gate and release.
use super::{Entry, Lineage, State};
use crate::{BuildError, ResultVersion, SampleSessionKey};
use du_core_contracts::Id;

impl State {
    /// Returns true for a lineage bound to a scope fingerprint or owned by a sample session.
    pub(super) fn is_bound(&self, id: &Id) -> bool {
        self.lineages.values().any(|bound| bound == id) || self.sample_lineages.contains_key(id)
    }

    /// A sample result may publish only while its lineage still belongs to a live session.
    pub(super) fn admits_publication(&self, lineage: &Lineage, result: &ResultVersion) -> bool {
        result.data_source.is_real()
            || lineage
                .as_ref()
                .is_some_and(|id| self.sample_lineages.contains_key(id))
    }

    /// Registers a fresh lineage owned by `session` and returns its identifier.
    ///
    /// The identifier is a UUID v7 built from a store-local counter, so it is unique within
    /// this store and distinct from every bound analysis lineage.
    pub(crate) fn register_sample_lineage(
        &mut self,
        session: &SampleSessionKey,
    ) -> Result<Id, BuildError> {
        loop {
            let next = self
                .next_sample
                .checked_add(1)
                .ok_or(BuildError::CapacityExceeded)?;
            self.next_sample = next;
            let mut bytes = [0_u8; 16];
            bytes[..4].copy_from_slice(b"SMPL");
            bytes[8..].copy_from_slice(&next.to_be_bytes());
            let id = Id::from_v7_bytes(bytes);
            if !self.is_bound(&id) {
                self.sample_lineages.insert(id.clone(), session.clone());
                return Ok(id);
            }
        }
    }

    /// Drops a lineage registered by `register_sample_lineage` whose build never started.
    pub(crate) fn unregister_sample_lineage(&mut self, lineage: &Id) {
        self.sample_lineages.remove(lineage);
        self.active.remove(&Some(lineage.clone()));
    }

    /// Detaches every lineage of `session`, returning the entries that were current.
    ///
    /// A build still holding a detached lineage can no longer publish.
    pub(crate) fn release_sample_session(&mut self, session: &SampleSessionKey) -> Vec<Entry> {
        let lineages: Vec<Id> = self
            .sample_lineages
            .iter()
            .filter(|(_, owner)| *owner == session)
            .map(|(lineage, _)| lineage.clone())
            .collect();
        let mut released = Vec::new();
        for lineage in lineages {
            self.sample_lineages.remove(&lineage);
            self.event_sequences.remove(&lineage);
            let key = Some(lineage);
            self.active.remove(&key);
            released.extend(self.current.remove(&key));
        }
        released
    }
}
