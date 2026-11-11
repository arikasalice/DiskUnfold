use super::*;
use du_core_contracts::{
    ports::{PortError, PortErrorKind},
    scan_observation::DirectoryObservations,
};
struct Empty;
impl DirectoryObservations for Empty {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        Ok(Vec::new())
    }
}
impl AnchoredDirectory for Empty {
    fn open_child(&self, _: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        Err(PortError {
            kind: PortErrorKind::Unsupported,
            reason_key: "os.unsupported".into(),
            os_code: None,
        })
    }
}
#[test]
fn permit_is_released_during_panic_unwind() {
    let frame = Frame {
        root_index: 0,
        path: Vec::new(),
        directory: Box::new(Empty),
        entries: VecDeque::new(),
        deferred: VecDeque::new(),
        root: true,
        first_batch: false,
        root_published: false,
        batch_pending: false,
        case_observed: false,
    };
    let shared = Shared {
        queue: Mutex::new(Queue {
            ready: VecDeque::from([frame]),
            ready_high_water: 1,
            last_root: None,
            remaining: 1,
            root_remaining: vec![1],
            root_groups: vec![0],
            active_groups: vec![0],
            device_limit: 1,
            started: true,
        }),
        wake: Condvar::new(),
        abort: AtomicBool::new(false),
        disconnects: disconnect::Disconnects::new(1, &[]),
    };
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut queue = shared
            .queue
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let claim = claim_ready(&shared, &mut queue);
        drop(queue);
        let _claim = claim;
        assert!(
            shared
                .queue
                .lock()
                .is_ok_and(|queue| queue.active_groups[0] == 1)
        );
        std::panic::resume_unwind(Box::new("injected panic"));
    }));
    assert!(result.is_err());
    assert!(
        shared
            .queue
            .lock()
            .is_ok_and(|queue| queue.active_groups[0] == 0)
    );
}

#[test]
fn ready_peak_tracks_simultaneous_frames_and_overflow() {
    struct Context;
    impl JobContext for Context {
        fn checkpoint(&self) -> Checkpoint {
            Checkpoint::Continue
        }
        fn try_start_flow(&self) -> Result<Box<dyn JobFlow>, JobFlowError> {
            Err(JobFlowError::Closed)
        }
    }
    let frame = || Frame {
        root_index: 0,
        path: Vec::new(),
        directory: Box::new(Empty),
        entries: VecDeque::new(),
        deferred: VecDeque::new(),
        root: true,
        first_batch: false,
        root_published: false,
        batch_pending: false,
        case_observed: false,
    };
    let shared = Shared {
        queue: Mutex::new(Queue {
            ready: VecDeque::new(),
            ready_high_water: 0,
            last_root: None,
            remaining: 0,
            root_remaining: vec![0],
            root_groups: vec![0],
            active_groups: vec![0],
            device_limit: 1,
            started: true,
        }),
        wake: Condvar::new(),
        abort: AtomicBool::new(false),
        disconnects: disconnect::Disconnects::new(1, &[]),
    };
    for _ in 0..QUEUED_DIRECTORIES {
        assert!(schedule(&shared, &Context, frame()).is_none());
    }
    assert!(schedule(&shared, &Context, frame()).is_some());
    let mut queue = shared
        .queue
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    assert_eq!(queue.ready_high_water, QUEUED_DIRECTORIES);
    assert!(queue.pop_fair().is_some());
    queue.active_groups[0] -= 1;
    assert_eq!(queue.ready.len(), QUEUED_DIRECTORIES - 1);
    queue.push_ready(frame());
    assert_eq!(queue.ready_high_water, QUEUED_DIRECTORIES);
    drop(queue);
    let (sender, _receiver) = std::sync::mpsc::sync_channel(1);
    assert!(super::cancel_drain::drain(
        &shared,
        &mut Vec::new(),
        &sender,
        &Context,
        Policy::Legacy(&|_| true)
    ));
    let queue = shared
        .queue
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    assert!(queue.ready.is_empty());
    assert_eq!(queue.ready_high_water, QUEUED_DIRECTORIES);
}
