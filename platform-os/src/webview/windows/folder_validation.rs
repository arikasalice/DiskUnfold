//! Worker and queue limits for native folder validation.

pub(super) const WORKER_COUNT: usize = 4;
pub(super) const WAITING_LIMIT: usize = 4;

pub(super) type FolderVerifier = dyn Fn(u64, &[u16]) -> String + Send + Sync;
