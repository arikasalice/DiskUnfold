//! Policy for spacing immutable partial result publications.

use std::time::Duration;

/// Tracks whether new observations warrant a timed partial publication.
///
/// Time values must come from one monotonic clock. The first publication is
/// requested when its first batch is ready. Later publications are due after
/// new observations and the greater of one second or twice the preceding
/// publication duration, measured from its completion.
#[derive(Default)]
pub struct PublicationCadence {
    last_end: Option<Duration>,
    interval: Duration,
    dirty: bool,
}

impl PublicationCadence {
    /// Marks newly accumulated observations or gaps.
    pub fn changed(&mut self) {
        self.dirty = true;
    }

    /// Records a successful publication and clears its accumulated changes.
    pub fn published(&mut self, started: Duration, finished: Duration) {
        self.last_end = Some(finished);
        self.interval =
            Duration::from_secs(1).max(finished.saturating_sub(started).saturating_mul(2));
        self.dirty = false;
    }

    /// Returns the wait until a later publication, or `None` when not needed.
    pub fn remaining(&self, now: Duration) -> Option<Duration> {
        let end = self.last_end?;
        if !self.dirty {
            return None;
        }
        Some(self.interval.saturating_sub(now.saturating_sub(end)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn enforces_boundaries_and_requires_new_observations() {
        let mut cadence = PublicationCadence::default();
        cadence.changed();
        assert_eq!(cadence.remaining(Duration::ZERO), None);
        cadence.published(Duration::ZERO, Duration::from_millis(100));
        assert_eq!(cadence.remaining(Duration::from_secs(5)), None);
        cadence.changed();
        assert_eq!(
            cadence.remaining(Duration::from_millis(1099)),
            Some(Duration::from_millis(1))
        );
        assert_eq!(
            cadence.remaining(Duration::from_millis(1100)),
            Some(Duration::ZERO)
        );
        cadence.published(Duration::from_millis(1100), Duration::from_millis(1800));
        cadence.changed();
        assert_eq!(
            cadence.remaining(Duration::from_millis(3199)),
            Some(Duration::from_millis(1))
        );
        assert_eq!(
            cadence.remaining(Duration::from_millis(3200)),
            Some(Duration::ZERO)
        );
    }
}
