use super::*;
use std::sync::{Mutex, Weak, mpsc};

struct PausedDelivery(Mutex<mpsc::Receiver<()>>);

impl JobEvents for PausedDelivery {
    fn publish(&self, _: JobEvent) {
        if let Ok(resume) = self.0.lock() {
            let _ = resume.recv();
        }
    }
}

struct DropProbe {
    gate: Arc<Mutex<Option<Arc<Mutex<()>>>>>,
    table: Weak<Mutex<crate::model::Table>>,
    observation: mpsc::Sender<bool>,
}
impl Drop for DropProbe {
    fn drop(&mut self) {
        let gate_free = self
            .gate
            .lock()
            .is_ok_and(|gate| gate.as_ref().is_none_or(|gate| gate.try_lock().is_ok()));
        let table_free = self
            .table
            .upgrade()
            .is_some_and(|table| table.try_lock().is_ok());
        let _ = self.observation.send(gate_free && table_free);
    }
}
#[test]
fn retained_visualization_disposal_follows_completion_and_queued_cancel()
-> Result<(), Box<dyn std::error::Error>> {
    for mode in ["done", "error", "panic", "cancel", "detach", "reject"] {
        let (resume, paused) = mpsc::channel();
        let (runtime, id) =
            setup_kind("viz.compute", Arc::new(PausedDelivery(Mutex::new(paused))))?;
        // Keep the delivery thread from taking the table lock to record a
        // terminal outcome while the destructor probes the releasing thread.
        // Dropping the sender also releases delivery if an assertion fails.
        {
            let table = runtime.table.lock().map_err(|_| "poisoned table")?;
            crate::delivery::enqueue(
                &table,
                crate::model::Delivery::Event(JobEvent::StateChanged {
                    job_id: id.clone(),
                    state: JobState::Queued,
                    sequence: 1,
                }),
            )?;
        }
        let gate = Arc::new(Mutex::new(None));
        let (sender, receiver) = mpsc::channel();
        let probe = Arc::new(DropProbe {
            gate: gate.clone(),
            table: Arc::downgrade(&runtime.table),
            observation: sender,
        });
        let callback_owner = probe.clone();
        let submitted = runtime.submit_viz_with_retained_admission(
            controlled_submission(
                "viz.compute",
                Arc::new(Barrier::new(1)),
                Arc::new(Barrier::new(1)),
                id.clone(),
            ),
            probe,
            move || {
                drop(callback_owner);
                match mode {
                    "error" => Err("support.internal_error".into()),
                    "panic" => std::panic::resume_unwind(Box::new("commit failed")),
                    "cancel" | "detach" | "reject" => {
                        std::panic::resume_unwind(Box::new("cancelled job published"))
                    }
                    _ => Ok(()),
                }
            },
            |_| {
                if mode == "reject" {
                    Err(RuntimeError::InvalidState)
                } else {
                    Ok(())
                }
            },
        );
        if mode == "reject" {
            assert_eq!(submitted, Err(RuntimeError::InvalidState));
            assert_eq!(runtime.state(&id), Err(RuntimeError::NotFound));
        } else {
            assert_eq!(submitted?, SubmitResult::Created(id.clone()));
            *gate.lock().map_err(|_| "poisoned probe")? = runtime.publication_gate(&id)?;
            match mode {
                "cancel" => {
                    assert_eq!(
                        runtime.cancel(&id)?,
                        CancelResult::Accepted(JobState::Cancelled)
                    );
                }
                "detach" => {
                    assert!(matches!(
                        runtime.cancel_viz_if(&id, || Ok(true))?,
                        Some((CancelResult::Accepted(JobState::Cancelled), _))
                    ));
                }
                _ => {
                    assert_eq!(runtime.run_next()?, Some(id.clone()));
                }
            }
            let expected = match mode {
                "done" => JobState::Done,
                "cancel" | "detach" => JobState::Cancelled,
                _ => JobState::Failed,
            };
            assert_eq!(runtime.state(&id)?, expected);
        }
        assert!(
            receiver.recv_timeout(Duration::from_secs(1))?,
            "destructor held runtime lock: {mode}"
        );
        drop(resume);
        runtime.flush()?;
    }
    Ok(())
}
use std::time::Duration;
