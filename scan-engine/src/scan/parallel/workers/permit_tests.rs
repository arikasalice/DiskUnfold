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

fn root_frame() -> Frame {
    Frame::root(0, Box::new(Empty))
}

#[test]
fn reserved_slot_and_seat_are_released_during_panic_unwind() {
    let mut queue = Queue::for_test(vec![0], 1);
    queue.remaining = 1;
    queue.root_remaining = vec![1];
    assert!(queue.push_root(root_frame()));
    let shared = Shared::for_test(queue, false);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let seat = SeatGuard::register(&shared);
        let index = seat.as_ref().map_or(usize::MAX, |seat| seat.index);
        let mut queue = shared
            .queue
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let claim = dispatch::claim(&shared, &mut queue, index);
        drop(queue);
        let _claim = claim;
        let _seat = seat;
        assert!(shared.queue.lock().is_ok_and(|queue| {
            queue.lanes.lanes[0].members == 1 && queue.lanes.lanes[0].reserved() == 1
        }));
        std::panic::resume_unwind(Box::new("injected panic"));
    }));
    assert!(result.is_err());
    assert!(shared.queue.lock().is_ok_and(|queue| {
        queue.lanes.lanes[0].members == 0 && queue.lanes.lanes[0].reserved() == 0
    }));
}

#[test]
fn ready_peak_tracks_simultaneous_frames_and_overflow() -> Result<(), Box<dyn std::error::Error>> {
    struct Context;
    impl JobContext for Context {
        fn checkpoint(&self) -> Checkpoint {
            Checkpoint::Continue
        }
        fn try_start_flow(&self) -> Result<Box<dyn JobFlow>, JobFlowError> {
            Err(JobFlowError::Closed)
        }
    }
    let shared = Shared::for_test(Queue::for_test(vec![0], 1), false);
    for _ in 0..QUEUED_DIRECTORIES {
        assert!(schedule(&shared, &Context, root_frame(), false).is_none());
    }
    let overflow = schedule(&shared, &Context, root_frame(), false).ok_or("overflow frame")?;
    let seat = {
        let mut queue = shared.queue.lock().map_err(|_| "queue lock")?;
        assert_eq!(queue.lanes.peak(), Some(QUEUED_DIRECTORIES));
        let seat = queue.lanes.register_seat();
        let claim = dispatch::claim(&shared, &mut queue, seat).ok_or("claim")?;
        assert_eq!(queue.lanes.queued(), QUEUED_DIRECTORIES - 1);
        // The claimed slot stays reserved: new work does not fit.
        let overflow = queue.offer(0, overflow).ok_or("reserved slot was reused")?;
        queue.stack(seat, 0, overflow);
        let Claim { frame, mut turn } = claim;
        let reserved = std::mem::take(&mut turn.reserved);
        queue.requeue(seat, 0, reserved, frame);
        drop(queue);
        drop(turn);
        seat
    };
    {
        let queue = shared.queue.lock().map_err(|_| "queue lock")?;
        assert_eq!(queue.lanes.queued(), QUEUED_DIRECTORIES);
        assert_eq!(queue.lanes.stacked(0), 1);
        assert_eq!(queue.lanes.peak(), Some(QUEUED_DIRECTORIES));
    }
    let (sender, _receiver) = std::sync::mpsc::sync_channel(1);
    assert!(super::cancel_drain::drain(
        &shared,
        seat,
        &sender,
        &Context,
        Policy::Legacy(&|_| true)
    ));
    let queue = shared.queue.lock().map_err(|_| "queue lock")?;
    assert_eq!(queue.lanes.queued(), 0);
    assert_eq!(queue.lanes.stacked(0), 0);
    assert_eq!(queue.lanes.peak(), Some(QUEUED_DIRECTORIES));
    Ok(())
}
