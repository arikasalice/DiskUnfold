//! F01 recent-scope policy over an owner-supplied storage adapter.
//!
//! The body format, retention limit, admission binding, and synchronization
//! with setting changes are supplied by the composing owner.
use std::num::NonZeroUsize;

use du_core_contracts::settings::SettingsSnapshot;

/// Body access for an already admitted, owner-defined storage candidate.
pub trait RecentBody {
    /// Returns the serialized body to be checked by the owner-supplied validator.
    fn body(&self) -> &str;
}

/// Storage failure categories preserved for the caller's boundary mapping.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepositoryError {
    /// A database lock could not be obtained.
    Busy,
    /// The storage device reported insufficient space.
    Full,
    /// Stored data is invalid or the selected body failed validation.
    Corrupt,
    /// A storage operation failed for another reason.
    Storage,
}

/// Storage operations owned by the concrete adapter.
///
/// `put` must add or update and enforce `cap` in one transaction. `get` must
/// validate the selected stored body, and failures must not become empty data.
pub trait RecentRepository {
    /// Summary returned by listing.
    type Summary;
    /// Fully validated selected entry.
    type Item;
    /// Candidate supplied by the trusted admission owner.
    type Write: RecentBody;

    /// Lists recent entry summaries.
    fn list(&mut self, cap: NonZeroUsize) -> Result<Vec<Self::Summary>, RepositoryError>;
    /// Reads and validates one entry, or returns `None` if it is absent.
    fn get(
        &mut self,
        id: &[u8; 16],
        cap: NonZeroUsize,
        validate: &dyn Fn(&str) -> bool,
    ) -> Result<Option<Self::Item>, RepositoryError>;
    /// Atomically stores a candidate and enforces the configured cap.
    fn put(
        &mut self,
        item: &Self::Write,
        cap: NonZeroUsize,
        validate: &dyn Fn(&str) -> bool,
    ) -> Result<(), RepositoryError>;
}

/// Policy failure, distinct from a rejected candidate body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecentError {
    /// The supplied candidate body did not pass the owner's validator.
    InvalidBody,
    /// The storage adapter failed, including corruption in an existing entry.
    Repository(RepositoryError),
}

/// User-environment recent-scope service with an injected limit and validator.
pub struct RecentService<R, V> {
    repository: R,
    cap: NonZeroUsize,
    validate: V,
}

impl<R: RecentRepository, V: Fn(&str) -> bool> RecentService<R, V> {
    /// Constructs the service without selecting a body format or a cap.
    pub fn new(repository: R, cap: NonZeroUsize, validate: V) -> Self {
        Self {
            repository,
            cap,
            validate,
        }
    }

    /// Lists stored summaries independently of the remember setting.
    pub fn list(&mut self) -> Result<Vec<R::Summary>, RecentError> {
        self.repository
            .list(self.cap)
            .map_err(RecentError::Repository)
    }

    /// Reads one stored entry independently of the remember setting.
    pub fn get(&mut self, id: &[u8; 16]) -> Result<Option<R::Item>, RecentError> {
        self.repository
            .get(id, self.cap, &self.validate)
            .map_err(RecentError::Repository)
    }

    /// Stores an admitted candidate only if the supplied current setting is on.
    ///
    /// `None` represents an unreadable setting and follows the published on
    /// default. The caller must serialize obtaining this snapshot, concurrent
    /// setting changes, and the write when immediate setting semantics matter.
    /// This method cannot prevent a stale snapshot or establish admission.
    pub fn put(
        &mut self,
        snapshot: Option<&SettingsSnapshot>,
        item: &R::Write,
    ) -> Result<(), RecentError> {
        if snapshot.is_some_and(|value| !super::remember_recent(value)) {
            return Ok(());
        }
        if !(self.validate)(item.body()) {
            return Err(RecentError::InvalidBody);
        }
        self.repository
            .put(item, self.cap, &self.validate)
            .map_err(RecentError::Repository)
    }
}

#[cfg(test)]
mod tests;
