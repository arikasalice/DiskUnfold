//! Read retention expiry candidates using the runtime's clock.

use du_core_contracts::job::HistoryExpiryRead;

pub use du_core_contracts::job::{HistoryExpiryCandidates, HistoryExpiryReadError};

use crate::JobRuntime;

impl JobRuntime {
    /// Reads the reference time once, then asks the history port for candidates.
    pub fn history_expiry_candidates(
        &self,
        history: &mut dyn HistoryExpiryRead,
        days: u32,
    ) -> Result<HistoryExpiryCandidates, HistoryExpiryReadError> {
        let reference_ms = self.clock.now_ms();
        history.expiry_candidates(days, reference_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{JobClock, JobEvent, JobEvents, JobIdentity, JobRuntime, RuntimeError};
    use du_core_contracts::{
        Id,
        job::{HistoryAppend, HistoryExpiryRead, JobHistory, JobSummary},
    };
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    struct Clock(Arc<AtomicUsize>);

    impl JobClock for Clock {
        fn now_ms(&self) -> i64 {
            self.0.fetch_add(1, Ordering::SeqCst);
            1234
        }
    }

    struct Identity;

    impl JobIdentity for Identity {
        fn next_id(&self) -> Id {
            Id::from_v7_bytes([1; 16])
        }
    }

    struct History;

    impl JobHistory for History {
        fn append(&self, _: &JobSummary) -> Result<HistoryAppend, String> {
            Ok(HistoryAppend::Added)
        }
    }

    struct Events;

    impl JobEvents for Events {
        fn publish(&self, _: JobEvent) {}
    }

    #[derive(Default)]
    struct Reader(Mutex<Vec<(u32, i64)>>);

    impl HistoryExpiryRead for Reader {
        fn delete_generation(&mut self) -> Result<u64, HistoryExpiryReadError> {
            Ok(0)
        }

        fn expiry_candidates(
            &mut self,
            days: u32,
            reference_ms: i64,
        ) -> Result<HistoryExpiryCandidates, HistoryExpiryReadError> {
            self.0
                .lock()
                .map_err(|_| HistoryExpiryReadError::Unavailable)?
                .push((days, reference_ms));
            Ok(HistoryExpiryCandidates {
                delete_generation: 0,
                count: 0,
                earliest_reckon_at_ms: None,
                latest_reckon_at_ms: None,
                digest: [0; 32],
                candidates: Vec::new(),
            })
        }
    }

    #[test]
    fn expiry_query_reads_the_clock_once_and_passes_its_reference_time() -> Result<(), RuntimeError>
    {
        let clock_reads = Arc::new(AtomicUsize::new(0));
        let runtime = JobRuntime::new(
            Arc::new(Identity),
            Arc::new(Clock(clock_reads.clone())),
            Arc::new(History),
            Arc::new(Events),
        )?;
        let mut reader = Reader::default();
        let result = runtime.history_expiry_candidates(&mut reader, 7);
        assert_eq!(clock_reads.load(Ordering::SeqCst), 1);
        assert_eq!(
            *reader.0.lock().map_err(|_| RuntimeError::Poisoned)?,
            [(7, 1234)]
        );
        assert_eq!(result.map(|value| value.count), Ok(0));
        Ok(())
    }
}
