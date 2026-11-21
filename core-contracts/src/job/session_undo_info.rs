use crate::Id;

/// The session could not determine the availability of undo information.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UndoInfoUnavailable;

/// Session-only undo information used when reviewing history retention.
///
/// Removal callers reserve candidate IDs first, then call [`Self::in_use`] for
/// every reservation. They commit the history transaction before calling
/// [`Self::remove`]. If any pre-commit step fails, they call [`Self::release`].
/// Reservations prevent new restore use while this sequence is in progress.
/// `release` and `remove` must be idempotent. An error from `present` or
/// `in_use` means the affected history is held.
pub trait SessionUndoInfo: Send + Sync {
    /// Returns the requested IDs whose undo information is present.
    fn present(&self, job_ids: &[Id]) -> Result<Vec<Id>, UndoInfoUnavailable>;

    /// Reports whether a restore currently uses the undo information.
    fn in_use(&self, job_id: &Id) -> Result<bool, UndoInfoUnavailable>;

    /// Reserves present IDs against new restore use and returns acquired IDs.
    fn reserve_removal(&self, job_ids: &[Id]) -> Vec<Id>;

    /// Releases removal reservations. Repeated calls have no additional effect.
    fn release(&self, job_ids: &[Id]);

    /// Removes undo information after its owning history transaction commits.
    /// Repeated calls have no additional effect.
    fn remove(&self, job_ids: &[Id]);
}
