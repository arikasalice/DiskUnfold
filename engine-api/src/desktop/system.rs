//! System identity and clock adapters for the desktop job runtime.

use std::time::{Duration, Instant};

use du_core_contracts::Id;
use du_job_runtime::{JobClock, JobIdentity};

pub(super) struct SystemIdentity;

impl JobIdentity for SystemIdentity {
    fn next_id(&self) -> Id {
        Id::from_v7_bytes(*uuid::Uuid::now_v7().as_bytes())
    }
}

pub(super) struct SystemClock(pub(super) Instant);

impl JobClock for SystemClock {
    fn monotonic_now(&self) -> Option<Duration> {
        Some(self.0.elapsed())
    }

    fn now_ms(&self) -> i64 {
        super::utc_now_ms()
    }
}
