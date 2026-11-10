use super::*;

fn disconnected() -> PortError {
    PortError {
        kind: PortErrorKind::Io,
        reason_key: "scan.disconnected".into(),
        os_code: None,
    }
}

fn entry(name: &str, kind: ObservedKind) -> ObservedEntry {
    ObservedEntry {
        name: native(name),
        enumerated_inode: 7,
        enumerated_kind: kind.clone(),
        metadata: Ok(MetadataObservation {
            kind,
            device: 1,
            inode: 7,
            size: Ok(5),
            allocated: None,
            link_count: None,
            space_identity: None,
            modified_at: None,
        }),
    }
}

struct ScriptedRoot {
    batches: VecDeque<Result<Vec<ObservedEntry>, PortError>>,
    calls: Arc<AtomicUsize>,
    opens: Arc<AtomicUsize>,
}

impl DirectoryObservations for ScriptedRoot {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        self.calls.fetch_add(1, Ordering::AcqRel);
        self.batches.pop_front().unwrap_or_else(|| Ok(Vec::new()))
    }
}

impl AnchoredDirectory for ScriptedRoot {
    fn open_child(&self, _: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        self.opens.fetch_add(1, Ordering::AcqRel);
        Err(disconnected())
    }
}

#[test]
fn metadata_disconnect_drains_returned_items_and_closes_only_matching_volume()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "worker test lock")?;
    let ids = Ids(AtomicUsize::new(1));
    let sink = IngestSink::default();
    let roots = vec![
        observed_root("C:\\A", 1),
        observed_root("C:\\B", 1),
        observed_root("D:\\C", 2),
    ];
    let mut missing = entry("lost", ObservedKind::File);
    missing.metadata = Err(disconnected());
    let batches = [
        vec![
            entry("folder", ObservedKind::Directory),
            entry("held", ObservedKind::File),
            missing,
        ],
        vec![entry("unread", ObservedKind::File)],
        vec![entry("healthy", ObservedKind::File)],
    ];
    let calls: Vec<_> = (0..3).map(|_| Arc::new(AtomicUsize::new(0))).collect();
    let opens = Arc::new(AtomicUsize::new(0));
    let dirs = batches
        .into_iter()
        .zip(&calls)
        .map(|(batch, calls)| {
            Box::new(ScriptedRoot {
                batches: VecDeque::from([Ok(batch)]),
                calls: Arc::clone(calls),
                opens: Arc::clone(&opens),
            }) as Box<dyn AnchoredDirectory>
        })
        .collect();
    let context = Context {
        completed: Arc::new(AtomicUsize::new(0)),
        abandoned: Arc::new(AtomicUsize::new(0)),
    };
    let build = sink.open_forest(ids.next_id(), roots);
    let outcome = collect_open_forest_with_groups(
        build,
        dirs,
        &ids,
        &context,
        1,
        &|_| true,
        DevicePlan {
            groups: vec![0, 0, 0],
            limit: 1,
        },
    );
    let result = sink
        .pin()
        .map_err(|error| format!("{error:?}"))?
        .ok_or("missing result")?;
    assert_eq!(
        outcome.result_version_id.as_ref(),
        Some(result.version_id())
    );
    assert_eq!(result.completeness(), ResultCompleteness::Missing);
    assert_eq!(
        calls
            .iter()
            .map(|c| c.load(Ordering::Acquire))
            .collect::<Vec<_>>(),
        [1, 0, 2]
    );
    assert_eq!(opens.load(Ordering::Acquire), 0);
    for name in ["folder", "held", "lost", "healthy"] {
        assert!(
            result.items().iter().any(|item| item.name == native(name)),
            "missing {name}"
        );
    }
    assert!(
        !result
            .items()
            .iter()
            .any(|item| item.name == native("unread"))
    );
    assert!(
        result
            .scope_gaps()
            .iter()
            .any(|gap| gap.root_index == 1 && gap.reason_key == "scan.disconnected")
    );
    assert!(!result.scope_gaps().iter().any(|gap| gap.root_index == 2));
    Ok(())
}

#[test]
fn missing_volume_ids_do_not_share_a_shutdown_key() {
    let keys = super::super::disconnect::Disconnects::new(&[vec![], vec![7], vec![7], vec![]]);
    keys.close(0);
    keys.close(2);
    assert!(keys.closed(0));
    assert!(keys.closed(1));
    assert!(keys.closed(2));
    assert!(!keys.closed(3));
}
