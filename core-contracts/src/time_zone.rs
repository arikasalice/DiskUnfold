//! Observed local time-zone evidence for one UTC instant.

use crate::Timestamp;

/// An OS-specific identifier, without an inferred mapping to another registry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TimeZoneId {
    /// A Windows time-zone registry key, not an IANA name.
    Windows(String),
    /// The zone identifier returned by the Linux system time library.
    Linux(String),
}

/// A successful local offset observation, optionally with an OS identifier.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalTimeZone {
    /// Identifier observed from the OS, absent when only the offset is known.
    pub identity: Option<TimeZoneId>,
    /// Signed seconds east of UTC at the requested instant.
    pub offset_seconds: i32,
}

/// A local time-zone observation could not be established.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TimeZoneUnavailable;

/// Reads local time-zone evidence without changing the system time or settings.
pub trait TimeZonePort: Send + Sync {
    /// Observes the local offset at `instant`; failures must not be treated as UTC.
    ///
    /// # Errors
    /// Returns `TimeZoneUnavailable` if the OS cannot establish the observation.
    fn observe(&self, instant: &Timestamp) -> Result<LocalTimeZone, TimeZoneUnavailable>;
}
