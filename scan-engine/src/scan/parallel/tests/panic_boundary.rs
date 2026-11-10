use super::*;
use std::sync::mpsc;

struct PanickingRoot {
    at_open: bool,
    emitted: bool,
}
impl DirectoryObservations for PanickingRoot {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        if !self.at_open {
            std::panic::resume_unwind(Box::new("next_batch panic"));
        }
        if std::mem::replace(&mut self.emitted, true) {
            return Ok(Vec::new());
        }
        Ok(vec![ObservedEntry {
            name: native("child"),
            enumerated_inode: 9,
            enumerated_kind: ObservedKind::Directory,
            metadata: Ok(MetadataObservation {
                kind: ObservedKind::Directory,
                device: 1,
                inode: 9,
                size: Ok(0),
                allocated: None,
                link_count: None,
                space_identity: None,
                modified_at: None,
            }),
        }])
    }
}
impl AnchoredDirectory for PanickingRoot {
    fn open_child(&self, _: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        std::panic::resume_unwind(Box::new("open_child panic"));
    }
}

#[test]
fn worker_panics_abort_other_flows_and_restore_previous_result()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "worker test lock")?;
    for at_open in [false, true] {
        let ids = Arc::new(Ids(AtomicUsize::new(1)));
        let sink = IngestSink::default();
        let mut prior = sink.open(ids.next_id()).map_err(|e| format!("{e:?}"))?;
        prior
            .add_directory(Vec::new())
            .map_err(|e| format!("{e:?}"))?;
        let baseline = prior
            .seal(ids.next_id(), ResultCompleteness::Complete)
            .map_err(|e| format!("{e:?}"))?
            .version_id()
            .clone();
        let finished = Arc::new(AtomicUsize::new(0));
        let abandoned = Arc::new(AtomicUsize::new(0));
        let context = Arc::new(Context {
            completed: finished.clone(),
            abandoned: abandoned.clone(),
        });
        let (send, receive) = mpsc::channel();
        let task_sink = sink.clone();
        let task_ids = ids.clone();
        std::thread::spawn(move || {
            let outcome = collect_parallel(
                &task_sink,
                task_ids.next_id(),
                Box::new(PanickingRoot {
                    at_open,
                    emitted: false,
                }),
                &*task_ids,
                &*context,
                2,
                &|_| true,
            );
            let _ = send.send(outcome);
        });
        let outcome = receive
            .recv_timeout(Duration::from_secs(3))
            .map_err(|_| "worker panic did not finish")?;
        assert!(outcome.result_version_id.is_none());
        assert_eq!(finished.load(Ordering::Acquire), 3);
        assert_eq!(abandoned.load(Ordering::Acquire), 0);
        assert_eq!(
            sink.pin()
                .map_err(|e| format!("{e:?}"))?
                .ok_or("missing baseline")?
                .version_id(),
            &baseline
        );
        assert!(sink.open(ids.next_id()).is_ok(), "build was not released");
    }
    Ok(())
}
