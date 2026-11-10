use super::*;
use du_core_contracts::Timestamp;

struct TimedRoot {
    source: SmallRoot,
    value: Option<Timestamp>,
    calls: Arc<AtomicUsize>,
}

impl DirectoryObservations for TimedRoot {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        self.source.next_batch()
    }
}

impl AnchoredDirectory for TimedRoot {
    fn open_child(&self, entry: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        self.source.open_child(entry)
    }

    fn observed_modified_at(&self) -> Option<Timestamp> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.value.clone()
    }
}

#[test]
fn forest_retains_each_root_time_and_leaves_unavailable_values_unknown()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "worker lock")?;
    let ids = Ids(AtomicUsize::new(1));
    let sink = IngestSink::default();
    let build = sink.open_forest(
        ids.next_id(),
        vec![observed_root("C:\\A", 1), observed_root("D:\\B", 2)],
    );
    let expected = Timestamp::parse("2020-09-13T12:26:40.000Z")?;
    let calls = Arc::new(AtomicUsize::new(0));
    let roots = [Some(expected.clone()), None]
        .into_iter()
        .map(|value| {
            Box::new(TimedRoot {
                source: SmallRoot {
                    fail: false,
                    done: value.is_none(),
                    name: native("file"),
                },
                value,
                calls: calls.clone(),
            }) as Box<dyn AnchoredDirectory>
        })
        .collect();
    let context = Context {
        completed: Arc::new(AtomicUsize::new(0)),
        abandoned: Arc::new(AtomicUsize::new(0)),
    };
    let outcome = collect_open_forest(build, roots, &ids, &context, 2, &|_| true);
    let result = sink
        .pin()
        .map_err(|e| format!("{e:?}"))?
        .ok_or("missing result")?;
    assert_eq!(
        outcome.result_version_id.as_ref(),
        Some(result.version_id())
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(result.item_modified_at(0), Some(&expected));
    assert_eq!(result.item_modified_at(1), None);
    assert_eq!(result.item_modified_at(2), None);
    Ok(())
}
