//! A slow subscriber neither blocks reports nor accumulates progress values.
use super::*;
use std::time::Instant;

/// Blocks inside its first progress delivery until the test releases it.
struct Slow {
    recorder: Recorder,
    entered: Mutex<Option<mpsc::Sender<()>>>,
    release: Mutex<mpsc::Receiver<()>>,
}
impl JobEvents for Slow {
    fn publish(&self, event: JobEvent) {
        self.recorder.publish(event);
    }
    fn publish_progress(&self, event: JobProgressEvent) {
        self.recorder.publish_progress(event);
        let entered = self
            .entered
            .lock()
            .ok()
            .and_then(|mut entered| entered.take());
        if let Some(entered) = entered {
            let _ = entered.send(());
            if let Ok(release) = self.release.lock() {
                let _ = release.recv_timeout(WAIT);
            }
        }
    }
}

#[test]
fn a_blocked_subscriber_keeps_at_most_one_queued_value_per_job() -> TestResult {
    let (entered, blocked) = mpsc::channel();
    let (release, released) = mpsc::channel();
    let sink = Arc::new(Slow {
        recorder: Recorder::default(),
        entered: Mutex::new(Some(entered)),
        release: Mutex::new(released),
    });
    let mut job = Harness::start(sink.clone(), ProgressUnit::Items)?;
    job.report(1, None)?;
    job.clock.advance(100);
    job.runtime.tick_progress()?;
    blocked.recv_timeout(WAIT)?;
    let started = Instant::now();
    for completed in 2..=2_000 {
        job.report(completed, None)?;
    }
    for _ in 0..5 {
        job.clock.advance(100);
        job.runtime.tick_progress()?;
    }
    assert!(
        started.elapsed() < WAIT,
        "reports waited for the subscriber"
    );
    release.send(())?;
    // The held value leaves the queue before the next value can be queued.
    job.runtime.flush()?;
    job.next_batch()?;
    let values: Vec<_> = sink
        .recorder
        .progress()
        .iter()
        .map(|event| event.completed)
        .collect();
    assert_eq!(values, vec![1, 2_000]);
    job.finish()?;
    check_order(&sink.recorder.seen())?;
    Ok(())
}
