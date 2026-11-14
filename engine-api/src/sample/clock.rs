//! Event timestamps for the sample session, including the fallback for an unreadable clock.

use std::time::{SystemTime, UNIX_EPOCH};

use du_core_contracts::Timestamp;

/// Reads the wall clock, or returns `None` when it is before the epoch or out of range.
pub(super) fn now() -> Option<Timestamp> {
    let elapsed = SystemTime::now().duration_since(UNIX_EPOCH).ok()?;
    Timestamp::from_unix_millis(i64::try_from(elapsed.as_millis()).ok()?)
}

/// Time stamped on an event when the clock could not be read.
///
/// A transition must always publish its event, because a gap in `sequence` would look like a
/// lost event to the window. The event reuses the time of the session's previous event, so
/// times never move backwards within a session. The first event of a session falls back to
/// the Unix epoch, which the window treats as an unknown time.
pub(super) fn fallback(last: Option<&Timestamp>) -> Option<Timestamp> {
    last.cloned().or_else(|| Timestamp::from_unix_millis(0))
}
