//! Lane moves of workers lent to the marked area (FR8.HD1.app-host.DD1).
//!
//! Real workers collect two roots in lanes 0 and 1 with all of root 1 marked
//! from the start; every read takes about 0.3 ms so that turns overlap. The
//! moves of one scan are counted per kind and printed; run with `--nocapture`
//! to see them. Their number depends on thread timing, so only the result and
//! the absence of moves without a marked area are asserted.
//!
//! A move is made while a worker claims: it leaves one lane, joins another and
//! returns its stack to the lane it left. In a tree whose marked part holds one
//! or two directories at a time, a worker of another lane can take the marked
//! item that a worker of the area lane returned at the end of its turn, before
//! that worker claims again; that worker then finds nothing and leaves.
use super::*;
use crate::scan::parallel::{lanes::Moves, queue::PriorityRegion};
use std::sync::mpsc::RecvTimeoutError;

/// Collects `shape` twice, root `r` in lane `r`, and returns the moves.
fn measure(shape: Shape, workers: usize, marked: bool) -> Result<Moves, TestError> {
    let probe = Probe::new(
        shape,
        2,
        Box::new(|_, _, call| {
            if call == Call::Read {
                thread::sleep(Duration::from_micros(300));
            }
            Ok(())
        }),
    );
    let shared = shared_for(&probe, vec![0, 1], workers, QUEUED_DIRECTORIES)?;
    if marked {
        let mut queue = shared.queue.lock().map_err(|_| "queue lock")?;
        queue.priority = Some(PriorityRegion {
            root_index: 1,
            path: Vec::new(),
        });
        queue.reclassify(None);
    }
    let deadline = Instant::now() + Duration::from_secs(20);
    let items = run_workers(&shared, workers, 64, |receiver| {
        let mut items = 0;
        loop {
            match receiver.recv_timeout(Duration::from_millis(50)) {
                Ok(Event::Item(..)) => items += 1,
                Ok(_) => {}
                Err(RecvTimeoutError::Disconnected) => return Ok(items),
                Err(RecvTimeoutError::Timeout) if Instant::now() < deadline => {}
                Err(RecvTimeoutError::Timeout) => {
                    shared.abort.store(true, Ordering::SeqCst);
                    shared.wake.notify_all();
                    return Err(format!("stalled after {items} items"));
                }
            }
        }
    })??;
    // Each root item is reported by its root batch, not as an item event.
    assert_eq!(items, 2 * (shape.items() - 1));
    let moves = shared.queue.lock().map_err(|_| "queue lock")?.lanes.moves;
    eprintln!(
        "directories={} workers={workers} marked={marked} os_calls={} {moves:?}",
        2 * shape.directories(),
        probe.calls(),
    );
    Ok(moves)
}

#[test]
fn lane_moves_of_one_scan_are_counted() -> Result<(), TestError> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "worker test lock")?;
    let wide = Shape {
        depth: 3,
        dirs: 4,
        files: 2,
        batch: 2,
    };
    let chain = Shape {
        depth: 40,
        dirs: 1,
        files: 2,
        batch: 2,
    };
    for shape in [wide, chain] {
        for workers in [4, 8] {
            assert_eq!(measure(shape, workers, false)?, Moves::default());
            measure(shape, workers, true)?;
        }
    }
    Ok(())
}
