//! Accounting for bytes returned by content reads.

/// Byte evidence for a hash stage, counting bytes returned by successful reads.
///
/// `exact` is false if a read fails, reports an invalid count, or the counter
/// overflows; in that case `observed` is only a lower bound on transferred bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadMeasurement {
    pub observed: u64,
    pub exact: bool,
}

impl Default for ReadMeasurement {
    fn default() -> Self {
        Self {
            observed: 0,
            exact: true,
        }
    }
}

impl ReadMeasurement {
    pub(crate) fn record(&mut self, count: usize, limit: usize) {
        if count > limit {
            self.exact = false;
        } else if let Some(total) = self.observed.checked_add(count as u64) {
            self.observed = total;
        } else {
            self.observed = u64::MAX;
            self.exact = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ReadMeasurement;

    #[test]
    fn count_overflow_saturates_and_is_inexact() {
        let mut measurement = ReadMeasurement {
            observed: u64::MAX,
            exact: true,
        };
        measurement.record(1, 1);
        assert_eq!(measurement.observed, u64::MAX);
        assert!(!measurement.exact);
    }
}
