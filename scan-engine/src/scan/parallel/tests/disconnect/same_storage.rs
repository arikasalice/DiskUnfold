//! FR12.HD1.app-host.DD1: which other roots a disconnection closes.
//!
//! Lanes follow device groups, which join roots by volume identity or by
//! physical device, and put every root without either into one group. The
//! disconnection state follows volume identity alone; a root without one is
//! closed only by its own evidence. So a root in the same lane is closed only
//! when it shares the reporting root's volume (see
//! `metadata_disconnect_drains_returned_items_and_closes_only_matching_volume`).
//! A root on another volume of the same device, or without a volume identity,
//! keeps being read to its end.
use super::*;

/// What became of the healthy root beside the one that disconnected.
struct Beside {
    /// Read calls of the healthy root.
    calls: usize,
    /// Whether the healthy root's entry is in the result.
    healthy: bool,
    /// The root index of every scope gap.
    gap_roots: Vec<u32>,
}

/// Collects a root that reports a disconnection and a healthy root in one
/// device group with one worker.
fn beside_a_disconnect(roots: Vec<ResultScopeRoot>) -> Result<Beside, Box<dyn std::error::Error>> {
    let ids = Ids(AtomicUsize::new(1));
    let sink = IngestSink::default();
    let mut missing = entry("lost", ObservedKind::File);
    missing.metadata = Err(disconnected());
    let healthy_calls = Arc::new(AtomicUsize::new(0));
    let dirs: Vec<Box<dyn AnchoredDirectory>> = vec![
        Box::new(ScriptedRoot {
            batches: VecDeque::from([Ok(vec![missing])]),
            calls: Arc::new(AtomicUsize::new(0)),
            opens: Arc::new(AtomicUsize::new(0)),
        }),
        Box::new(ScriptedRoot {
            batches: VecDeque::from([Ok(vec![entry("healthy", ObservedKind::File)])]),
            calls: Arc::clone(&healthy_calls),
            opens: Arc::new(AtomicUsize::new(0)),
        }),
    ];
    let context = Context {
        completed: Arc::new(AtomicUsize::new(0)),
        abandoned: Arc::new(AtomicUsize::new(0)),
    };
    let outcome = collect_open_forest_with_groups(
        sink.open_forest(ids.next_id(), roots),
        dirs,
        &ids,
        &context,
        1,
        &|_| true,
        DevicePlan {
            groups: vec![0, 0],
            limit: 1,
        },
    );
    assert!(outcome.completed_scope);
    let result = sink
        .pin()
        .map_err(|error| format!("{error:?}"))?
        .ok_or("missing result")?;
    Ok(Beside {
        calls: healthy_calls.load(Ordering::Acquire),
        healthy: result
            .items()
            .iter()
            .any(|item| item.name == native("healthy")),
        gap_roots: result
            .scope_gaps()
            .iter()
            .map(|gap| gap.root_index)
            .collect(),
    })
}

fn assert_read_to_its_end(beside: &Beside) {
    // One batch and the end of the listing, whichever root ran first.
    assert_eq!(beside.calls, 2);
    assert!(beside.healthy);
    assert!(!beside.gap_roots.is_empty());
    assert!(beside.gap_roots.iter().all(|&root| root == 0));
}

#[test]
fn another_volume_of_the_same_device_keeps_being_read() -> Result<(), Box<dyn std::error::Error>> {
    let _guard = worker_test_lock();
    let roots = vec![observed_root("C:\\A", 1), observed_root("D:\\B", 2)];
    assert_read_to_its_end(&beside_a_disconnect(roots)?);
    Ok(())
}

#[test]
fn a_root_without_a_volume_identity_keeps_being_read() -> Result<(), Box<dyn std::error::Error>> {
    let _guard = worker_test_lock();
    let mut roots = vec![observed_root("C:\\A", 1), observed_root("C:\\B", 1)];
    for root in &mut roots {
        root.volume_id = Vec::new();
    }
    assert_read_to_its_end(&beside_a_disconnect(roots)?);
    Ok(())
}
