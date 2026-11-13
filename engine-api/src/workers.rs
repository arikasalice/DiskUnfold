//! Bounded wake-ups for workers that take jobs from the runtime queue.
use std::{
    io,
    sync::{Arc, Mutex, mpsc},
    thread::JoinHandle,
};

use du_job_runtime::JobRuntime;

pub(crate) struct Workers {
    pub(crate) runtime: Arc<JobRuntime>,
    wake: Mutex<Option<mpsc::SyncSender<()>>>,
    threads: Mutex<Vec<JoinHandle<()>>>,
}

impl Workers {
    pub(crate) fn start(runtime: Arc<JobRuntime>) -> std::io::Result<Self> {
        let (wake, inbox) = mpsc::sync_channel(64);
        let inbox = Arc::new(Mutex::new(inbox));
        let mut threads = Vec::new();
        for index in 0..4 {
            let inbox = inbox.clone();
            let runtime = Arc::downgrade(&runtime);
            let thread = std::thread::Builder::new()
                .name(format!("analysis-job-{index}"))
                .spawn(move || {
                    while wait(&inbox) {
                        let Some(runtime) = runtime.upgrade() else {
                            break;
                        };
                        while matches!(runtime.run_next(), Ok(Some(_))) {}
                    }
                });
            match thread {
                Ok(thread) => threads.push(thread),
                Err(error) => {
                    drop(wake);
                    for thread in threads {
                        let _ = thread.join();
                    }
                    return Err(error);
                }
            }
        }
        Ok(Self {
            runtime,
            wake: Mutex::new(Some(wake)),
            threads: Mutex::new(threads),
        })
    }

    /// A full mailbox already has enough wake-ups to drain the job table.
    pub(crate) fn notify(&self) {
        if let Ok(wake) = self.wake.lock()
            && let Some(wake) = wake.as_ref()
        {
            let _ = wake.try_send(());
        }
    }

    /// Closes the worker mailbox and joins every idle worker thread.
    ///
    /// Call after the job runtime has no nonterminal work. Closing the sender
    /// also wakes threads blocked on the shared receiver so they can exit.
    pub(crate) fn close_and_join(&self) -> io::Result<()> {
        let active = self
            .runtime
            .list(None)
            .map_err(|error| io::Error::other(format!("could not inspect jobs: {error}")))?
            .into_iter()
            .any(|entry| {
                !matches!(
                    entry.snapshot.state,
                    du_core_contracts::job::JobState::Done
                        | du_core_contracts::job::JobState::Partial
                        | du_core_contracts::job::JobState::Failed
                        | du_core_contracts::job::JobState::Cancelled
                )
            });
        if active {
            return Err(io::Error::other("active jobs remain"));
        }
        let wake = self
            .wake
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        drop(wake);

        let mut threads = self
            .threads
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut panicked = false;
        for thread in threads.drain(..) {
            panicked |= thread.join().is_err();
        }
        if panicked {
            return Err(io::Error::other("job worker thread panicked"));
        }
        Ok(())
    }
}

fn wait(inbox: &Mutex<mpsc::Receiver<()>>) -> bool {
    inbox.lock().is_ok_and(|inbox| inbox.recv().is_ok())
}
