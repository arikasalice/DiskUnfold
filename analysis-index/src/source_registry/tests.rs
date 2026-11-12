use super::*;

fn id(value: u8) -> Id {
    Id::from_v7_bytes([value; 16])
}

#[test]
fn direct_and_snapshot_sources_start_at_zero_and_advance_after_writer_success() {
    for kind in [SourceKind::Direct, SourceKind::Snapshot] {
        let registry = SourceRegistry::default();
        let source_id = id(1);
        let handle = registry
            .register(source_id.clone(), kind)
            .expect("register");
        assert_eq!(registry.next_sequence(&source_id, &handle), Ok(1));
        assert_eq!(registry.next_sequence(&source_id, &handle), Ok(1));
        registry
            .apply_next(&source_id, &handle, 1, |_| Ok::<_, ()>(()))
            .expect("commit");
        assert_eq!(registry.next_sequence(&source_id, &handle), Ok(2));
    }
}

#[test]
fn watch_source_starts_after_accepted_snapshot_cutoff() {
    let registry = SourceRegistry::default();
    let source_id = id(2);
    let handle = registry
        .register_watch(source_id.clone(), || Ok::<_, ()>(41))
        .expect("register");
    assert_eq!(registry.next_sequence(&source_id, &handle), Ok(42));
}

#[test]
fn failed_watch_snapshot_does_not_register_or_advance_a_source() {
    let registry = SourceRegistry::default();
    let source_id = id(12);
    assert!(matches!(
        registry.register_watch(source_id.clone(), || Err::<u64, _>("snapshot failed")),
        Err(WatchRegistrationError::Snapshot("snapshot failed"))
    ));
    let handle = registry
        .register_watch(source_id.clone(), || Ok::<_, ()>(7))
        .expect("retry accepted snapshot");
    assert_eq!(registry.next_sequence(&source_id, &handle), Ok(8));
}

#[test]
fn rejects_missing_or_unexpected_cutoffs_and_duplicate_ids() {
    let registry = SourceRegistry::default();
    assert_eq!(
        registry.register(id(3), SourceKind::Watch).err(),
        Some(RegistryError::WatchRequiresSnapshot)
    );
    let source_id = id(5);
    let _handle = registry
        .register(source_id.clone(), SourceKind::Direct)
        .expect("register");
    assert_eq!(
        registry.register(source_id, SourceKind::Snapshot).err(),
        Some(RegistryError::DuplicateSource)
    );
}

#[test]
fn rejects_cross_source_sequences_and_non_next_candidates_without_mutation() {
    let registry = SourceRegistry::default();
    let first_id = id(6);
    let second_id = id(7);
    let first = registry
        .register(first_id.clone(), SourceKind::Direct)
        .expect("register");
    let second = registry
        .register(second_id.clone(), SourceKind::Direct)
        .expect("register");
    assert_eq!(
        registry.next_sequence(&second_id, &first),
        Err(RegistryError::CrossSource)
    );
    assert_eq!(
        registry.apply_next(&first_id, &first, 2, |_| Ok::<_, ()>(())),
        Err(SequenceApplyError::Registry(
            RegistryError::SequenceMismatch
        ))
    );
    assert_eq!(registry.next_sequence(&first_id, &first), Ok(1));
    assert_eq!(registry.next_sequence(&second_id, &second), Ok(1));
}

#[test]
fn rejects_a_handle_from_another_registry_even_when_source_ids_match() {
    let source_id = id(10);
    let first_registry = SourceRegistry::default();
    let second_registry = SourceRegistry::default();
    let first = first_registry
        .register(source_id.clone(), SourceKind::Direct)
        .expect("register");
    let _second = second_registry
        .register(source_id.clone(), SourceKind::Direct)
        .expect("register");
    assert_eq!(
        second_registry.next_sequence(&source_id, &first),
        Err(RegistryError::CrossSource)
    );
}

#[test]
fn concurrent_reuse_of_a_candidate_invokes_only_one_writer() {
    use std::sync::Barrier;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let registry = Arc::new(SourceRegistry::default());
    let source_id = id(11);
    let handle = Arc::new(
        registry
            .register(source_id.clone(), SourceKind::Direct)
            .expect("register"),
    );
    let barrier = Arc::new(Barrier::new(2));
    let writes = Arc::new(AtomicUsize::new(0));
    let results = std::thread::scope(|scope| {
        let mut tasks = Vec::new();
        for _ in 0..2 {
            let registry = Arc::clone(&registry);
            let handle = Arc::clone(&handle);
            let source_id = source_id.clone();
            let barrier = Arc::clone(&barrier);
            let writes = Arc::clone(&writes);
            tasks.push(scope.spawn(move || {
                barrier.wait();
                registry.apply_next(&source_id, &handle, 1, |_| {
                    writes.fetch_add(1, Ordering::SeqCst);
                    Ok::<_, ()>(())
                })
            }));
        }
        tasks
            .into_iter()
            .map(|task| task.join().expect("worker"))
            .collect::<Vec<_>>()
    });
    assert_eq!(writes.load(Ordering::SeqCst), 1);
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(
                result,
                Err(SequenceApplyError::Registry(
                    RegistryError::SequenceMismatch
                ))
            ))
            .count(),
        1
    );
    assert_eq!(registry.next_sequence(&source_id, &handle), Ok(2));
}

#[test]
fn failed_writer_does_not_advance_the_registry() {
    let registry = SourceRegistry::default();
    let source_id = id(8);
    let handle = registry
        .register(source_id.clone(), SourceKind::Direct)
        .expect("register");
    let candidate = registry
        .next_sequence(&source_id, &handle)
        .expect("candidate");
    assert_eq!(
        registry.apply_next(&source_id, &handle, candidate, |_| Err::<(), _>(
            "write failed"
        )),
        Err(SequenceApplyError::Writer("write failed"))
    );
    assert_eq!(registry.next_sequence(&source_id, &handle), Ok(1));
}

#[test]
fn exhaustion_end_late_commit_and_reregistration_are_rejected() {
    let registry = SourceRegistry::default();
    let source_id = id(9);
    let handle = registry
        .register_watch(source_id.clone(), || Ok::<_, ()>(u64::MAX))
        .expect("register");
    assert_eq!(
        registry.next_sequence(&source_id, &handle),
        Err(RegistryError::SequenceOverflow)
    );
    registry.end(&source_id, &handle).expect("end");
    assert_eq!(
        registry.next_sequence(&source_id, &handle),
        Err(RegistryError::Expired)
    );
    assert_eq!(
        registry.apply_next(&source_id, &handle, u64::MAX, |_| Ok::<_, ()>(())),
        Err(SequenceApplyError::Registry(RegistryError::Expired))
    );
    assert_eq!(
        registry.register(source_id, SourceKind::Direct).err(),
        Some(RegistryError::DuplicateSource)
    );
}
