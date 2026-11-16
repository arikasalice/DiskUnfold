use super::{FILES_PER_DIRECTORY, Probe, State, WORKER_COUNT, native_name};
use std::{
    sync::{Arc, Barrier, PoisonError, mpsc},
    thread,
};

#[test]
fn timed_out_wait_accepts_snapshot_with_four_active_workers_and_closed_gate()
-> Result<(), Box<dyn std::error::Error>> {
    let probe = probe_snapshot(WORKER_COUNT, false)?;
    inject_timeout(&probe, true);

    probe
        .wait_for_four_workers()
        .map_err(std::io::Error::other)?;
    assert!(
        probe
            .wait_timeout_override
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_none()
    );
    Ok(())
}

#[test]
fn timed_out_wait_rejects_snapshot_with_three_active_workers()
-> Result<(), Box<dyn std::error::Error>> {
    let probe = probe_snapshot(WORKER_COUNT - 1, false)?;
    inject_timeout(&probe, true);

    let Err(error) = probe.wait_for_four_workers() else {
        return Err(std::io::Error::other("three-worker snapshot was accepted").into());
    };
    assert!(
        error.contains("3 distinct workers"),
        "unexpected error: {error}"
    );
    Ok(())
}

#[test]
fn early_release_rejects_four_active_workers() -> Result<(), Box<dyn std::error::Error>> {
    let probe = probe_snapshot(WORKER_COUNT, true)?;
    inject_timeout(&probe, false);

    let Err(error) = probe.wait_for_four_workers() else {
        return Err(std::io::Error::other("early release was accepted").into());
    };
    assert!(error.contains("gate release"), "unexpected error: {error}");
    Ok(())
}

fn inject_timeout(probe: &Probe, timed_out: bool) {
    *probe
        .wait_timeout_override
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = Some(timed_out);
}

fn probe_snapshot(
    active_workers: usize,
    gate_released: bool,
) -> Result<Probe, Box<dyn std::error::Error>> {
    let barrier = Arc::new(Barrier::new(WORKER_COUNT + 1));
    let (send, receive) = mpsc::channel();
    let handles: Vec<_> = (0..WORKER_COUNT)
        .map(|_| {
            let barrier = Arc::clone(&barrier);
            let send = send.clone();
            thread::spawn(move || {
                let _ = send.send(thread::current().id());
                barrier.wait();
            })
        })
        .collect();
    drop(send);
    let workers: Vec<_> = (0..WORKER_COUNT)
        .map(|_| receive.recv())
        .collect::<Result<_, _>>()?;
    barrier.wait();
    for handle in handles {
        assert!(handle.join().is_ok(), "worker ID fixture thread panicked");
    }

    let mut state = State {
        gate_released,
        ..State::default()
    };
    for (index, worker) in workers.into_iter().take(active_workers).enumerate() {
        let directory = vec![native_name(&format!("directory-{index:02}"))];
        state.parked.push((directory.clone(), worker));
        for file in 0..FILES_PER_DIRECTORY {
            let mut path = directory.clone();
            path.push(native_name(&format!("item-{file:02}.bin")));
            state.entries.insert(path.clone(), 1);
            state.entry_sizes.insert(path, vec![Some(2)]);
        }
    }
    Ok(Probe {
        state: std::sync::Mutex::new(state),
        changed: std::sync::Condvar::new(),
        wait_timeout_override: std::sync::Mutex::new(None),
    })
}
