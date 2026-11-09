//! Bounded wake-ups for workers that take jobs from the runtime queue.
use std::sync::{Arc, Mutex, mpsc};

use du_job_runtime::JobRuntime;

pub(crate) struct Workers {
    pub(crate) runtime: Arc<JobRuntime>,
    wake: mpsc::SyncSender<()>,
}

impl Workers {
    pub(crate) fn start(runtime: Arc<JobRuntime>) -> std::io::Result<Self> {
        let (wake, inbox) = mpsc::sync_channel(64);
        let inbox = Arc::new(Mutex::new(inbox));
        for index in 0..4 {
            let inbox = inbox.clone();
            let runtime = Arc::downgrade(&runtime);
            std::thread::Builder::new()
                .name(format!("analysis-job-{index}"))
                .spawn(move || {
                    while wait(&inbox) {
                        let Some(runtime) = runtime.upgrade() else {
                            break;
                        };
                        while matches!(runtime.run_next(), Ok(Some(_))) {}
                    }
                })?;
        }
        Ok(Self { runtime, wake })
    }

    /// A full mailbox already has enough wake-ups to drain the job table.
    pub(crate) fn notify(&self) {
        let _ = self.wake.try_send(());
    }
}

fn wait(inbox: &Mutex<mpsc::Receiver<()>>) -> bool {
    inbox.lock().is_ok_and(|inbox| inbox.recv().is_ok())
}
