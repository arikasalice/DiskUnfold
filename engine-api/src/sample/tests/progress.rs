//! The sample tap relays job progress, so scan progress still reaches the window.

use std::sync::{Arc, Mutex};

use du_core_contracts::job::ProgressUnit;
use du_job_runtime::{JobEvent, JobEvents, JobProgressEvent, JobWaitReason};

use super::job_id;
use crate::sample::SampleEvents;

#[derive(Default)]
struct Recorder(Mutex<Vec<JobProgressEvent>>);

impl JobEvents for Recorder {
    fn publish(&self, _: JobEvent) {}

    fn publish_progress(&self, event: JobProgressEvent) {
        if let Ok(mut events) = self.0.lock() {
            events.push(event);
        }
    }
}

#[test]
fn progress_passes_through_the_sample_tap_unchanged() -> Result<(), String> {
    let downstream = Arc::new(Recorder::default());
    let tap = SampleEvents::new(downstream.clone());
    let event = JobProgressEvent {
        job_id: job_id(7),
        sequence: 3,
        unit: ProgressUnit::Items,
        completed: 40,
        total: Some(100),
        remaining: Some(60),
        wait_reason: JobWaitReason::Unknown,
        observed_at_ms: 0,
    };
    tap.publish_progress(event.clone());
    let relayed = downstream.0.lock().map_err(|_| "recorder lock poisoned")?;
    assert_eq!(relayed.as_slice(), [event]);
    Ok(())
}
