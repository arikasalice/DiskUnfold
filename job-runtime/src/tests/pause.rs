use super::*;
use du_core_contracts::job::{JobFlow, JobFlowError};
use std::{
    sync::mpsc,
    time::{Duration, Instant},
};

struct HoldingBody {
    send: mpsc::Sender<Arc<dyn JobContext>>,
    release: Mutex<mpsc::Receiver<()>>,
    completed_scope: bool,
}
impl JobBody for HoldingBody {
    fn kind_id(&self) -> &str {
        "scan.metadata"
    }
    fn run(&self, context: Arc<dyn JobContext>) -> JobOutcome {
        let _ = self.send.send(context);
        if let Ok(receiver) = self.release.lock() {
            let _ = receiver.recv();
        }
        JobOutcome {
            counts: JobCounts::default(),
            completed_scope: self.completed_scope,
            failure_reason_key: None,
            unprocessed_summary: None,
            result_version_id: None,
        }
    }
}
type Setup = (
    Arc<JobRuntime>,
    Arc<Events>,
    Id,
    Arc<dyn JobContext>,
    mpsc::Sender<()>,
    std::thread::JoinHandle<Result<Option<Id>, RuntimeError>>,
);
fn setup() -> Result<Setup, Box<dyn std::error::Error>> {
    setup_with_completion(true)
}

fn setup_with_completion(completed_scope: bool) -> Result<Setup, Box<dyn std::error::Error>> {
    let id = Id::parse("00000000-0000-7000-8000-000000000001")?;
    let events = Arc::new(Events::default());
    let runtime = Arc::new(JobRuntime::new_with_limits(
        Arc::new(Identity(id)),
        Arc::new(Clock),
        Arc::new(History::default()),
        events.clone(),
        FlowLimits {
            metadata_io: 3,
            queued_items: 4,
            item_bytes: 32,
        },
    )?);
    let mut kind = descriptor();
    kind.controls = vec![JobControl::Pause, JobControl::Resume, JobControl::Cancel];
    runtime.register(kind)?;
    let (send, recv) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    let id = created(runtime.submit(sample_submission(Arc::new(HoldingBody {
        send,
        release: Mutex::new(wait),
        completed_scope,
    }))?)?)?;
    let worker = {
        let runtime = runtime.clone();
        std::thread::spawn(move || runtime.run_next())
    };
    let context = recv.recv_timeout(Duration::from_secs(2))?;
    Ok((runtime, events, id, context, release, worker))
}

#[test]
fn internal_failure_wakes_parked_flow_without_cancelling_the_job()
-> Result<(), Box<dyn std::error::Error>> {
    let (runtime, events, id, context, release, worker) = setup_with_completion(false)?;
    let first = context
        .try_start_flow()
        .map_err(|error| format!("{error:?}"))?;
    let in_flight = context
        .try_start_flow()
        .map_err(|error| format!("{error:?}"))?;
    runtime.pause(&id)?;
    let (parked, result) = start(first);
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let parked_count = runtime
            .table
            .lock()
            .map_err(|_| "table poisoned")?
            .jobs
            .get(&id)
            .ok_or("missing job")?
            .paused_flows;
        if parked_count == 1 {
            break;
        }
        if Instant::now() >= deadline {
            return Err("flow did not park".into());
        }
        std::thread::yield_now();
    }

    context.abort_flows();
    assert_eq!(
        result.recv_timeout(Duration::from_secs(2))?,
        Checkpoint::Cancel
    );
    parked.join().map_err(|_| "thread panic")?;
    assert_eq!(context.checkpoint(), Checkpoint::Continue);
    assert!(matches!(
        context.try_start_flow(),
        Err(JobFlowError::Closed)
    ));
    release.send(())?;
    assert_eq!(runtime.state(&id)?, JobState::PauseRequested);
    assert_eq!(in_flight.safe_point(), Checkpoint::Cancel);
    in_flight.finish(JobCounts::default());
    worker.join().map_err(|_| "worker panic")??;
    runtime.flush()?;

    assert_eq!(runtime.state(&id)?, JobState::Failed);
    let events = events.0.lock().map_err(|_| "events poisoned")?;
    assert!(!events.iter().any(|event| matches!(
        event,
        JobEvent::StateChanged {
            state: JobState::Paused | JobState::CancelRequested | JobState::Cancelled,
            ..
        }
    )));
    assert!(
        matches!(events.last(), Some(JobEvent::Finished { summary, .. }) if summary.state == JobState::Failed)
    );
    Ok(())
}
fn wait_state(
    runtime: &JobRuntime,
    id: &Id,
    desired: JobState,
) -> Result<(), Box<dyn std::error::Error>> {
    let deadline = Instant::now() + Duration::from_secs(2);
    while runtime.state(id)? != desired {
        if Instant::now() >= deadline {
            return Err("state deadline expired".into());
        }
        std::thread::yield_now();
    }
    Ok(())
}
fn start(flow: Box<dyn JobFlow>) -> (std::thread::JoinHandle<()>, mpsc::Receiver<Checkpoint>) {
    let (send, recv) = mpsc::channel();
    (
        std::thread::spawn(move || {
            let result = flow.safe_point();
            let _ = send.send(result);
            flow.finish(JobCounts::default());
        }),
        recv,
    )
}
#[test]
fn pause_waits_for_every_live_flow_then_resume_preserves_job()
-> Result<(), Box<dyn std::error::Error>> {
    let (runtime, _, id, context, release, worker) = setup()?;
    let a = context.try_start_flow().map_err(|e| format!("{e:?}"))?;
    let b = context.try_start_flow().map_err(|e| format!("{e:?}"))?;
    let (result, seq) = runtime.pause(&id)?;
    assert_eq!(result, CancelResult::Accepted(JobState::PauseRequested));
    assert!(matches!(context.try_start_flow(), Err(JobFlowError::Busy)));
    let (a_thread, a_result) = start(a);
    assert_eq!(runtime.state(&id)?, JobState::PauseRequested);
    let (b_thread, b_result) = start(b);
    wait_state(&runtime, &id, JobState::Paused)?;
    assert_eq!(runtime.snapshot(&id)?.sequence, seq + 1);
    assert_eq!(
        runtime.pause(&id)?.0,
        CancelResult::NoEffect(JobState::Paused)
    );
    assert_eq!(
        runtime.resume(&id)?.0,
        CancelResult::Accepted(JobState::Running)
    );
    assert_eq!(
        a_result.recv_timeout(Duration::from_secs(2))?,
        Checkpoint::Continue
    );
    assert_eq!(
        b_result.recv_timeout(Duration::from_secs(2))?,
        Checkpoint::Continue
    );
    a_thread.join().map_err(|_| "thread panic")?;
    b_thread.join().map_err(|_| "thread panic")?;
    release.send(())?;
    worker.join().map_err(|_| "worker panic")??;
    assert_eq!(runtime.state(&id)?, JobState::Done);
    assert_eq!(
        runtime.resume(&id)?.0,
        CancelResult::AlreadyTerminal(JobState::Done)
    );
    Ok(())
}
#[test]
fn zero_flows_pause_blocks_registration_until_resume() -> Result<(), Box<dyn std::error::Error>> {
    let (runtime, _, id, context, release, worker) = setup()?;
    let (_, seq) = runtime.pause(&id)?;
    assert_eq!(runtime.state(&id)?, JobState::Paused);
    assert_eq!(runtime.snapshot(&id)?.sequence, seq + 1);
    assert!(matches!(context.try_start_flow(), Err(JobFlowError::Busy)));
    runtime.resume(&id)?;
    context
        .try_start_flow()
        .map_err(|e| format!("{e:?}"))?
        .finish(JobCounts::default());
    release.send(())?;
    worker.join().map_err(|_| "worker panic")??;
    Ok(())
}
#[test]
fn cancel_wakes_paused_flow_and_waits_for_inflight_flow() -> Result<(), Box<dyn std::error::Error>>
{
    let (runtime, _, id, context, release, worker) = setup()?;
    let paused = context.try_start_flow().map_err(|e| format!("{e:?}"))?;
    let inflight = context.try_start_flow().map_err(|e| format!("{e:?}"))?;
    runtime.pause(&id)?;
    let (thread, result) = start(paused);
    assert_eq!(
        runtime.cancel(&id)?,
        CancelResult::Accepted(JobState::CancelRequested)
    );
    assert!(matches!(
        context.try_start_flow(),
        Err(JobFlowError::Cancelled)
    ));
    assert_eq!(
        result.recv_timeout(Duration::from_secs(2))?,
        Checkpoint::Cancel
    );
    thread.join().map_err(|_| "thread panic")?;
    release.send(())?;
    assert_eq!(runtime.state(&id)?, JobState::CancelRequested);
    assert_eq!(inflight.safe_point(), Checkpoint::Cancel);
    inflight.finish(JobCounts::default());
    worker.join().map_err(|_| "worker panic")??;
    assert_eq!(runtime.state(&id)?, JobState::Cancelled);
    Ok(())
}

#[test]
fn flow_exit_completes_pause_and_registration_is_serialized()
-> Result<(), Box<dyn std::error::Error>> {
    let (runtime, _, id, context, release, worker) = setup()?;
    let first = context.try_start_flow().map_err(|e| format!("{e:?}"))?;
    let second = context.try_start_flow().map_err(|e| format!("{e:?}"))?;
    runtime.pause(&id)?;
    let (thread, result) = start(first);
    second.finish(JobCounts::default());
    wait_state(&runtime, &id, JobState::Paused)?;
    assert!(matches!(context.try_start_flow(), Err(JobFlowError::Busy)));
    runtime.resume(&id)?;
    assert_eq!(
        result.recv_timeout(Duration::from_secs(2))?,
        Checkpoint::Continue
    );
    thread.join().map_err(|_| "thread panic")?;
    release.send(())?;
    worker.join().map_err(|_| "worker panic")??;
    Ok(())
}

#[test]
fn pause_registration_race_never_admits_unaccounted_flow() -> Result<(), Box<dyn std::error::Error>>
{
    for _ in 0..30 {
        let (runtime, _, id, context, release, worker) = setup()?;
        let barrier = Arc::new(Barrier::new(2));
        let flow_thread = {
            let barrier = barrier.clone();
            let context = context.clone();
            std::thread::spawn(move || {
                barrier.wait();
                context.try_start_flow()
            })
        };
        barrier.wait();
        runtime.pause(&id)?;
        match flow_thread.join().map_err(|_| "thread panic")? {
            Ok(flow) => {
                assert_eq!(runtime.state(&id)?, JobState::PauseRequested);
                flow.finish(JobCounts::default());
                wait_state(&runtime, &id, JobState::Paused)?;
            }
            Err(JobFlowError::Busy) => assert_eq!(runtime.state(&id)?, JobState::Paused),
            Err(other) => return Err(format!("unexpected registration: {other:?}").into()),
        }
        runtime.resume(&id)?;
        release.send(())?;
        worker.join().map_err(|_| "worker panic")??;
    }
    Ok(())
}

#[test]
fn resume_before_the_safe_point_keeps_control_events_ordered()
-> Result<(), Box<dyn std::error::Error>> {
    let (runtime, events, id, context, release, worker) = setup()?;
    let flow = context
        .try_start_flow()
        .map_err(|error| format!("{error:?}"))?;
    let (_, requested) = runtime.pause(&id)?;
    let (resumed, running) = runtime.resume(&id)?;
    assert_eq!(resumed, CancelResult::Accepted(JobState::Running));
    assert_eq!(running, requested + 1);
    assert_eq!(flow.safe_point(), Checkpoint::Continue);

    let (_, requested_again) = runtime.pause(&id)?;
    let (thread, result) = start(flow);
    wait_state(&runtime, &id, JobState::Paused)?;
    let (_, cancelling) = runtime.cancel_with_sequence(&id)?;
    assert_eq!(
        result.recv_timeout(Duration::from_secs(2))?,
        Checkpoint::Cancel
    );
    thread.join().map_err(|_| "thread panic")?;
    release.send(())?;
    worker.join().map_err(|_| "worker panic")??;
    runtime.flush()?;

    let events = events.0.lock().map_err(|_| "events poisoned")?;
    let transitions = events
        .iter()
        .filter_map(|event| match event {
            JobEvent::StateChanged {
                job_id,
                state,
                sequence,
            } if job_id == &id && *sequence >= requested => Some((*state, *sequence)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        transitions,
        [
            (JobState::PauseRequested, requested),
            (JobState::Running, running),
            (JobState::PauseRequested, requested_again),
            (JobState::Paused, requested_again + 1),
            (JobState::CancelRequested, cancelling),
            (JobState::Cancelled, cancelling + 1),
        ]
    );
    assert!(
        matches!(events.last(), Some(JobEvent::Finished { sequence, .. }) if *sequence == cancelling + 2)
    );
    Ok(())
}

#[test]
fn failed_resume_delivery_leaves_parked_flow_resumable() -> Result<(), Box<dyn std::error::Error>> {
    let (runtime, _, id, context, release, worker) = setup()?;
    let flow = context
        .try_start_flow()
        .map_err(|error| format!("{error:?}"))?;
    runtime.pause(&id)?;
    let (thread, result) = start(flow);
    wait_state(&runtime, &id, JobState::Paused)?;
    let paused = runtime.snapshot(&id)?;

    let sender = runtime
        .table
        .lock()
        .map_err(|_| "table poisoned")?
        .sender
        .take();
    let failed = runtime.resume(&id);
    runtime.table.lock().map_err(|_| "table poisoned")?.sender = sender;
    assert_eq!(failed, Err(RuntimeError::DeliveryUnavailable));
    let unchanged = runtime.snapshot(&id)?;
    assert_eq!(
        (unchanged.state, unchanged.sequence),
        (paused.state, paused.sequence)
    );
    assert!(matches!(result.try_recv(), Err(mpsc::TryRecvError::Empty)));

    assert_eq!(
        runtime.resume(&id)?.0,
        CancelResult::Accepted(JobState::Running)
    );
    assert_eq!(
        result.recv_timeout(Duration::from_secs(2))?,
        Checkpoint::Continue
    );
    thread.join().map_err(|_| "thread panic")?;
    release.send(())?;
    worker.join().map_err(|_| "worker panic")??;
    Ok(())
}
