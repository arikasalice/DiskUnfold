//! Pausing leaves no mark; a cancellation while paused marks like one
//! that was not paused.
use super::fixture::*;
use super::*;
use std::sync::atomic::Ordering::SeqCst;

/// Pauses every flow once `pause` is set, until it is cancelled or resumed.
struct Pausing {
    pause: Arc<AtomicBool>,
    gate: Arc<Gate>,
}

#[derive(Default)]
struct Gate {
    cancelled: AtomicBool,
    resumed: AtomicBool,
    parked: AtomicUsize,
}

struct PausingFlow {
    pause: Arc<AtomicBool>,
    gate: Arc<Gate>,
}

impl JobContext for Pausing {
    fn checkpoint(&self) -> Checkpoint {
        if self.gate.cancelled.load(SeqCst) {
            Checkpoint::Cancel
        } else {
            Checkpoint::Continue
        }
    }
    fn try_start_flow(&self) -> Result<Box<dyn JobFlow>, JobFlowError> {
        Ok(Box::new(PausingFlow {
            pause: Arc::clone(&self.pause),
            gate: Arc::clone(&self.gate),
        }))
    }
}

impl JobFlow for PausingFlow {
    fn safe_point(&self) -> Checkpoint {
        let held = || {
            self.pause.load(SeqCst)
                && !self.gate.cancelled.load(SeqCst)
                && !self.gate.resumed.load(SeqCst)
        };
        if held() {
            self.gate.parked.fetch_add(1, SeqCst);
            while held() {
                std::thread::sleep(Duration::from_millis(1));
            }
            self.gate.parked.fetch_sub(1, SeqCst);
        }
        if self.gate.cancelled.load(SeqCst) {
            Checkpoint::Cancel
        } else {
            Checkpoint::Continue
        }
    }
    fn report_item(&self, _: &[u8]) -> Result<(), JobFlowError> {
        Ok(())
    }
    fn finish(self: Box<Self>, _: JobCounts) {}
}

fn tree() -> Arc<Tree> {
    let root = vec![vec![dir("a", 30), dir("b", 31), file("x1", 35)]];
    Tree::new(
        HashMap::from([
            (ROOT, Node::Batches(root)),
            (30, Node::Batches(vec![vec![file("a1", 301)]])),
            (
                31,
                Node::Batches(vec![vec![file("b1", 311)], vec![file("b2", 312)]]),
            ),
        ]),
        Trigger::Read(31, 0),
    )
}

/// Collects with one worker and pauses at `b`'s first read; once the worker
/// parks, the scan is cancelled or resumed.
fn paused(filtered: bool, cancel: bool) -> Result<(Arc<Tree>, Arc<ResultVersion>), String> {
    let tree = tree();
    let gate = Arc::new(Gate::default());
    let sink = IngestSink::default();
    let worker_tree = Arc::clone(&tree);
    let worker_gate = Arc::clone(&gate);
    let worker_sink = sink.clone();
    let worker = std::thread::spawn(move || {
        let ids = Ids(AtomicUsize::new(1));
        let context = Pausing {
            pause: Arc::clone(&worker_tree.cancel),
            gate: worker_gate,
        };
        let build = worker_sink.open_forest(ids.next_id(), vec![observed_root("C:\\R", 1)]);
        let devices = DevicePlan {
            groups: vec![0],
            limit: 4,
        };
        let root = vec![worker_tree.root()];
        if filtered {
            collect_open_forest_filtered(build, root, &ids, &context, 1, &filter, devices)
        } else {
            collect_open_forest_with_groups(build, root, &ids, &context, 1, &legacy, devices)
        }
    });
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while gate.parked.load(SeqCst) == 0 {
        if std::time::Instant::now() >= deadline {
            return Err("the worker never parked".into());
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    let flag = if cancel {
        &gate.cancelled
    } else {
        &gate.resumed
    };
    flag.store(true, SeqCst);
    let outcome = worker.join().map_err(|_| "join")?;
    assert_eq!(outcome.completed_scope, !cancel);
    let version = sink.pin().map_err(|e| format!("{e:?}"))?.ok_or("missing")?;
    Ok((tree, version))
}

#[test]
fn a_pause_that_is_resumed_leaves_no_mark() -> Result<(), String> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "lock")?;
    for filtered in [false, true] {
        let (_, version) = paused(filtered, false)?;
        assert_eq!(version.completeness(), ResultCompleteness::Complete);
        assert!(cancelled(&version)?.is_empty());
        assert_eq!(version.items().len(), 6);
    }
    Ok(())
}

/// The paused worker holds `b` with its first batch read: `b` and the root,
/// whose end was not read yet, are marked; `a` was read to its end before.
#[test]
fn a_cancellation_while_paused_marks_like_a_cancellation() -> Result<(), String> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "lock")?;
    for filtered in [false, true] {
        let (tree, version) = paused(filtered, true)?;
        assert_eq!(version.completeness(), ResultCompleteness::Cancelled);
        let marked = cancelled(&version)?;
        let ended = tree.with_log(|log| log.ended.clone())?;
        assert!(marked.contains(&path(&["b"])), "filtered={filtered}");
        assert!(!ended.contains(&31));
        for (folder, inode) in [(path(&[]), ROOT), (path(&["a"]), 30)] {
            assert_eq!(marked.contains(&folder), !ended.contains(&inode));
        }
        assert_eq!(
            version.unread_folder_counts().cancelled as usize,
            marked.len()
        );
    }
    Ok(())
}
