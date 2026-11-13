//! Bounded execution of target observations and delivery as F01 item events.
use du_core_contracts::{
    OsPath, PathBytes,
    analysis_ipc::AnalysisTarget,
    job::{Checkpoint, JobBody, JobContext, JobCounts, JobFlow, JobFlowError, JobOutcome},
    volume::{Capability, CapabilityState, VolumePort},
};
use du_job_runtime::JobClock;
use du_scan_engine::{
    target::{self, AccessObservation, AccessResult, AccessStatus, ProbeResult},
    target_probe::{self, TARGET_PROBE_KIND},
};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

type Flow = Arc<Mutex<Option<Box<dyn JobFlow>>>>;

/// A target probe whose deadline is supplied by the composition point.
pub(crate) struct TargetProbe {
    pub targets: Vec<ProbeTarget>,
    pub volumes: Arc<dyn VolumePort>,
    pub capacity: Capability,
    pub path_access: Capability,
    pub timeout: Duration,
    pub clock: Arc<dyn JobClock>,
}

/// The path and confirmed identity from the same discovery observation.
#[derive(Clone)]
pub(crate) struct ProbeTarget {
    path: OsPath,
    volume_id: Option<Vec<u8>>,
}

impl ProbeTarget {
    pub(crate) fn from_discovery(
        target: &AnalysisTarget,
    ) -> Result<Self, crate::error::EngineError> {
        let volume_id = target
            .volume_id
            .as_deref()
            .map(|value| {
                PathBytes::parse(value).and_then(|id| id.bytes().map_err(|e| e.to_string()))
            })
            .transpose()
            .map_err(|_| crate::error::EngineError::reason("support.internal_error"))?;
        Ok(Self {
            path: target.path.clone(),
            volume_id,
        })
    }
}

struct Pending {
    path: OsPath,
    flow: Flow,
    io_finished: Arc<AtomicBool>,
    deadline: Instant,
    receiver: mpsc::Receiver<(Instant, ProbeResult)>,
    ack: mpsc::SyncSender<()>,
}

struct TimedOut {
    flow: Flow,
    io_finished: Arc<AtomicBool>,
}

fn observation(
    port: &dyn VolumePort,
    target: &ProbeTarget,
    capacity: &Capability,
    path_access: &Capability,
    context: &dyn JobContext,
    deadline: Instant,
) -> ProbeResult {
    if context.checkpoint() == Checkpoint::Cancel {
        return unavailable("job.cancelled", AccessStatus::Unknown);
    }
    if Instant::now() >= deadline {
        return unavailable("os.call_slow", AccessStatus::Unresponsive);
    }
    if path_access.state == CapabilityState::Unsupported {
        return unavailable(
            path_access.reason_key.unwrap_or("os.unsupported"),
            AccessStatus::Unknown,
        );
    }
    let Ok(raw) = target.path.raw.bytes() else {
        return unavailable("path.unresolvable", AccessStatus::Unknown);
    };
    let observation = port.observe(&raw, target.volume_id.as_deref(), &|| {
        capacity.state != CapabilityState::Unsupported
            && context.checkpoint() != Checkpoint::Cancel
            && Instant::now() < deadline
    });
    if context.checkpoint() == Checkpoint::Cancel {
        return unavailable("job.cancelled", AccessStatus::Unknown);
    }
    if Instant::now() >= deadline {
        return unavailable("os.call_slow", AccessStatus::Unresponsive);
    }
    let (mut access, capacity_result) = match observation {
        Ok(observation) => (
            target::access_result(AccessObservation::Completed(Ok(()))),
            observation.capacity,
        ),
        Err(error) => (
            target::access_result(AccessObservation::Completed(Err(error))),
            None,
        ),
    };
    let capacity = if let Some(result) = capacity_result {
        result.map_err(|error| {
            let reason = error.reason_key.clone();
            let latest = target::access_result(AccessObservation::Completed(Err(error)));
            if latest.status == AccessStatus::Disconnected {
                access = latest;
            }
            reason
        })
    } else if access.status != AccessStatus::Accessible {
        Err(access
            .reason
            .clone()
            .unwrap_or_else(|| "os.query_failed".into()))
    } else {
        Err(capacity.reason_key.unwrap_or("os.unsupported").into())
    };
    ProbeResult {
        access,
        capacity,
        aggregation_uncertain: true,
        observed_at: None,
    }
}

fn unavailable(reason: &str, status: AccessStatus) -> ProbeResult {
    ProbeResult {
        access: AccessResult {
            status,
            reason: Some(reason.into()),
        },
        capacity: Err(reason.into()),
        aggregation_uncertain: true,
        observed_at: None,
    }
}

fn report(flow: &Flow, path: OsPath, mut result: ProbeResult, clock: &dyn JobClock) -> bool {
    result.observed_at.get_or_insert_with(|| clock.now_ms());
    let Some(status) = target_probe::status(path, result) else {
        return false;
    };
    let Ok(payload) = serde_json::to_vec(&status) else {
        return false;
    };
    flow.lock()
        .ok()
        .and_then(|guard| {
            guard
                .as_ref()
                .map(|flow| flow.report_item(&payload).is_ok())
        })
        .unwrap_or(false)
}

fn start(
    context: &Arc<dyn JobContext>,
    body: &TargetProbe,
    target: ProbeTarget,
) -> Result<Pending, JobFlowError> {
    let flow = Arc::new(Mutex::new(Some(context.try_start_flow()?)));
    let (send, receiver) = mpsc::sync_channel(1);
    let (ack, done) = mpsc::sync_channel(1);
    let thread_flow = flow.clone();
    let io_finished = Arc::new(AtomicBool::new(false));
    let thread_finished = io_finished.clone();
    let port = body.volumes.clone();
    let capability = body.capacity.clone();
    let path_access = body.path_access.clone();
    let clock = body.clock.clone();
    let path = target.path.clone();
    let thread_context = context.clone();
    let now = Instant::now();
    let deadline = now.checked_add(body.timeout).unwrap_or(now);
    let spawned = std::thread::Builder::new()
        .name("target-probe-io".into())
        .spawn(move || {
            let mut result = observation(
                &*port,
                &target,
                &capability,
                &path_access,
                &*thread_context,
                deadline,
            );
            let finished = Instant::now();
            result.observed_at = Some(clock.now_ms());
            thread_finished.store(true, Ordering::Release);
            let _ = send.send((finished, result));
            let _ = done.recv();
            if let Ok(mut guard) = thread_flow.lock()
                && let Some(flow) = guard.take()
            {
                flow.finish(JobCounts::default());
            }
        });
    if spawned.is_err() {
        if let Ok(mut guard) = flow.lock()
            && let Some(flow) = guard.take()
        {
            flow.finish(JobCounts::default());
        }
        return Err(JobFlowError::DeliveryUnavailable);
    }
    Ok(Pending {
        path,
        flow,
        io_finished,
        deadline,
        receiver,
        ack,
    })
}

impl JobBody for TargetProbe {
    fn kind_id(&self) -> &str {
        TARGET_PROBE_KIND
    }
    fn run(&self, context: Arc<dyn JobContext>) -> JobOutcome {
        let mut pending: Vec<Pending> = Vec::new();
        let mut timed_out: Vec<TimedOut> = Vec::new();
        let mut waiting = self
            .targets
            .iter()
            .cloned()
            .collect::<std::collections::VecDeque<_>>();
        let mut failed = false;
        let mut no_flow_since = None;
        let mut no_flow_timeout = false;
        while !pending.is_empty() || !waiting.is_empty() {
            timed_out.retain(|task| !task.io_finished.load(Ordering::Acquire));
            let cancelled = context.checkpoint() == Checkpoint::Cancel;
            if cancelled {
                if let Some(flow) = pending
                    .first()
                    .map(|task| &task.flow)
                    .or_else(|| timed_out.first().map(|task| &task.flow))
                {
                    for target in waiting.drain(..) {
                        failed |= !report(
                            flow,
                            target.path,
                            unavailable("job.cancelled", AccessStatus::Unknown),
                            &*self.clock,
                        );
                    }
                } else {
                    waiting.clear();
                }
            } else if let Some(target) = waiting.front().cloned() {
                match start(&context, self, target) {
                    Ok(task) => {
                        waiting.pop_front();
                        pending.push(task);
                        no_flow_since = None;
                        continue;
                    }
                    Err(JobFlowError::Busy) if !pending.is_empty() => {
                        no_flow_since = None;
                    }
                    Err(JobFlowError::Busy) if !timed_out.is_empty() => {
                        for target in waiting.drain(..) {
                            failed |= !report(
                                &timed_out[0].flow,
                                target.path,
                                unavailable("os.call_slow", AccessStatus::Unresponsive),
                                &*self.clock,
                            );
                        }
                    }
                    Err(JobFlowError::Busy) => {
                        let started = *no_flow_since.get_or_insert_with(Instant::now);
                        if started.elapsed() >= self.timeout {
                            no_flow_timeout = true;
                            waiting.clear();
                        }
                    }
                    Err(JobFlowError::Cancelled) => continue,
                    Err(_) => {
                        failed = true;
                        waiting.clear();
                    }
                }
            }
            failed |= process_pending(&*context, &mut pending, &mut timed_out, &*self.clock);
            if !pending.is_empty() || !waiting.is_empty() {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        JobOutcome {
            counts: JobCounts::default(),
            completed_scope: !failed && !no_flow_timeout,
            failure_reason_key: if no_flow_timeout {
                Some("os.call_slow".into())
            } else {
                failed.then(|| "support.internal_error".into())
            },
            unprocessed_summary: None,
            result_version_id: None,
        }
    }
}

fn process_pending(
    context: &dyn JobContext,
    pending: &mut Vec<Pending>,
    timed_out: &mut Vec<TimedOut>,
    clock: &dyn JobClock,
) -> bool {
    let mut failed = false;
    let mut index = 0;
    while index < pending.len() {
        let task = &pending[index];
        let result = if context.checkpoint() == Checkpoint::Cancel {
            Some(unavailable("job.cancelled", AccessStatus::Unknown))
        } else if let Ok((finished, result)) = task.receiver.try_recv() {
            if finished <= task.deadline {
                Some(result)
            } else {
                Some(unavailable("os.call_slow", AccessStatus::Unresponsive))
            }
        } else if Instant::now() >= task.deadline {
            Some(unavailable("os.call_slow", AccessStatus::Unresponsive))
        } else {
            None
        };
        if let Some(result) = result {
            let task = pending.swap_remove(index);
            let timed_out_result = result.access.status == AccessStatus::Unresponsive;
            failed |= !report(&task.flow, task.path, result, clock);
            // A late observation is still a timeout, but a completed OS call no
            // longer justifies skipping other targets while its permit is released.
            if timed_out_result && !task.io_finished.load(Ordering::Acquire) {
                timed_out.push(TimedOut {
                    flow: task.flow.clone(),
                    io_finished: task.io_finished.clone(),
                });
            }
            let _ = task.ack.try_send(());
        } else {
            index += 1;
        }
    }
    failed
}

#[cfg(test)]
mod tests;
