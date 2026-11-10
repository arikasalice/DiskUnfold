//! Bounded native folder validation for WebView2 drop requests.
use std::{
    io,
    sync::{Arc, Mutex, mpsc},
};

const WORKER_COUNT: usize = 4;
const WAITING_LIMIT: usize = 4;

type FolderVerifier = dyn Fn(u64, &[u16]) -> String + Send + Sync;

/// Keeps directory checks bounded while allowing a later drop to bypass one slow check.
pub(super) fn start(
    replies: mpsc::SyncSender<String>,
    verify: Arc<FolderVerifier>,
) -> io::Result<mpsc::SyncSender<(u64, Vec<u16>)>> {
    let (sender, receiver) = mpsc::sync_channel::<(u64, Vec<u16>)>(WAITING_LIMIT);
    let receiver = Arc::new(Mutex::new(receiver));
    for index in 0..WORKER_COUNT {
        let incoming = Arc::clone(&receiver);
        let outgoing = replies.clone();
        let verify = Arc::clone(&verify);
        std::thread::Builder::new()
            .name(format!("diskunfold-folder-drop-{index}"))
            .spawn(move || {
                loop {
                    let request = incoming.lock().ok().and_then(|queue| queue.recv().ok());
                    let Some((id, path)) = request else { break };
                    if outgoing.send(verify(id, &path)).is_err() {
                        break;
                    }
                }
            })?;
    }
    Ok(sender)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn slow_folder_validation_does_not_block_a_later_drop() -> Result<(), Box<dyn std::error::Error>>
    {
        let (entered, started) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        let gate = Mutex::new(gate);
        let (outgoing, replies) = mpsc::sync_channel(8);
        let queue = start(
            outgoing,
            Arc::new(move |id, _| {
                if id == 1 {
                    let _ = entered.send(());
                    if let Ok(gate) = gate.lock() {
                        let _ = gate.recv_timeout(Duration::from_secs(5));
                    }
                }
                format!("folder-drop:{id}")
            }),
        )?;
        queue.try_send((1, vec![1]))?;
        started.recv_timeout(Duration::from_secs(5))?;
        queue.try_send((2, vec![2]))?;
        assert_eq!(
            replies.recv_timeout(Duration::from_secs(5))?,
            "folder-drop:2"
        );
        release.send(())?;
        assert_eq!(
            replies.recv_timeout(Duration::from_secs(5))?,
            "folder-drop:1"
        );
        Ok(())
    }

    #[test]
    fn folder_verification_queue_remains_bounded_when_all_workers_block()
    -> Result<(), Box<dyn std::error::Error>> {
        let (entered, started) = mpsc::channel();
        let gate = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
        let worker_gate = gate.clone();
        let (outgoing, _replies) = mpsc::sync_channel(16);
        let queue = start(
            outgoing,
            Arc::new(move |id, _| {
                let _ = entered.send(());
                if let Ok(locked) = worker_gate.0.lock() {
                    let _guard = worker_gate.1.wait_timeout_while(
                        locked,
                        Duration::from_secs(5),
                        |released| !*released,
                    );
                }
                format!("folder-drop:{id}")
            }),
        )?;
        for id in 0..WORKER_COUNT {
            queue.try_send((id as u64, vec![]))?;
        }
        for _ in 0..WORKER_COUNT {
            started.recv_timeout(Duration::from_secs(5))?;
        }
        for id in WORKER_COUNT..WORKER_COUNT + WAITING_LIMIT {
            queue.try_send((id as u64, vec![]))?;
        }
        let full = matches!(
            queue.try_send(((WORKER_COUNT + WAITING_LIMIT) as u64, vec![])),
            Err(mpsc::TrySendError::Full(_))
        );
        if let Ok(mut released) = gate.0.lock() {
            *released = true;
            gate.1.notify_all();
        }
        assert!(full);
        Ok(())
    }
}
