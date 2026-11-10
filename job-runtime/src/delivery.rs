use crate::model::*;
use du_core_contracts::job::{
    HistoryAppendOutcome, HistoryChangeReason, HistoryChanged, JobHistory,
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
    mpsc,
};

pub(super) fn enqueue(table: &Table, delivery: Delivery) -> Result<(), RuntimeError> {
    table
        .sender
        .as_ref()
        .ok_or(RuntimeError::DeliveryUnavailable)?
        .send(delivery)
        .map_err(|_| RuntimeError::DeliveryUnavailable)
}
pub(super) fn consume(
    receiver: mpsc::Receiver<Delivery>,
    history: &dyn JobHistory,
    events: &dyn JobEvents,
    outstanding: &AtomicUsize,
    table: std::sync::Weak<Mutex<Table>>,
    clock: Arc<dyn JobClock>,
) {
    loop {
        let delivery = match receiver.recv_timeout(std::time::Duration::from_secs(60)) {
            Ok(delivery) => delivery,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if let Some(table) = table.upgrade() {
                    let now = clock.now_ms();
                    if let Ok(mut table) = table.lock() {
                        expire(&mut table, now);
                    }
                }
                continue;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        match delivery {
            Delivery::Stop(sender) => {
                let _ = sender.send(());
                break;
            }
            #[cfg(test)]
            Delivery::Fence(sender) => {
                let _ = sender.send(());
            }
            Delivery::Event(event) => events.publish(event),
            Delivery::Item(event, pending) => {
                events.publish(event);
                pending.fetch_sub(1, Ordering::AcqRel);
            }
            Delivery::Terminal {
                summary,
                sequence,
                record,
            } => {
                let (history_recorded, warning, receipt) = if record {
                    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        history.append_with_receipt(&summary)
                    })) {
                        Ok(Ok(HistoryAppendOutcome::Added(receipt))) => (true, None, receipt),
                        Ok(Ok(HistoryAppendOutcome::Duplicate)) => (true, None, None),
                        Ok(Err(error)) => (false, Some(error), None),
                        Err(_) => (false, Some("history.record_failed".into()), None),
                    }
                } else {
                    (false, None, None)
                };
                events.publish(JobEvent::StateChanged {
                    job_id: summary.job_id.clone(),
                    state: summary.state,
                    sequence,
                });
                events.publish(JobEvent::Finished {
                    summary,
                    sequence: sequence + 1,
                    history_recorded,
                    warning,
                });
                if let Some(receipt) = receipt {
                    events.publish_history_changed(HistoryChanged {
                        revision: receipt.revision,
                        reason: HistoryChangeReason::Appended,
                        count: 1,
                        observed_at_ms: receipt.observed_at_ms,
                        deleted_job_ids: Vec::new(),
                    });
                }
                outstanding.fetch_sub(1, Ordering::AcqRel);
            }
        }
    }
}
