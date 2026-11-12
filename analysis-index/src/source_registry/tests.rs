use super::*;

fn id(value: u64) -> Id {
    Id::parse(&format!("018f0000-0000-7000-8000-{value:012x}")).expect("valid test ID")
}

#[test]
fn direct_and_snapshot_sources_start_at_zero_and_advance_after_atomic_writer_success() {
    for kind in [SourceKind::Direct, SourceKind::Snapshot] {
        let registry = SourceRegistry::default();
        let handle = registry.register(id(1), kind).expect("register");
        let source_id = handle.source_id().clone();
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
    let handle = registry
        .register_watch(id(2), || Ok::<_, ()>(41))
        .expect("register");
    assert_eq!(registry.next_sequence(handle.source_id(), &handle), Ok(42));
}

#[test]
fn failed_watch_snapshot_does_not_register_a_source() {
    let registry = SourceRegistry::default();
    assert!(matches!(
        registry.register_watch(id(3), || Err::<u64, _>("snapshot failed")),
        Err(WatchRegistrationError::Snapshot("snapshot failed"))
    ));
    let handle = registry
        .register_watch(id(3), || Ok::<_, ()>(7))
        .expect("retry accepted snapshot");
    assert_eq!(registry.next_sequence(handle.source_id(), &handle), Ok(8));
}

#[test]
fn rejects_missing_or_unexpected_cutoffs_and_issues_distinct_ids() {
    let registry = SourceRegistry::default();
    assert_eq!(
        registry.register(id(4), SourceKind::Watch).err(),
        Some(RegistryError::WatchRequiresSnapshot)
    );
    let first = registry
        .register(id(5), SourceKind::Direct)
        .expect("register");
    let second = registry
        .register(id(6), SourceKind::Snapshot)
        .expect("register");
    assert_ne!(first.source_id(), second.source_id());
    assert_eq!(
        registry.register(id(5), SourceKind::Snapshot).err(),
        Some(RegistryError::DuplicateSource)
    );
}

#[test]
fn rejects_cross_source_sequences_and_non_next_candidates_without_mutation() {
    let registry = SourceRegistry::default();
    let first = registry
        .register(id(7), SourceKind::Direct)
        .expect("register");
    let second = registry
        .register(id(8), SourceKind::Direct)
        .expect("register");
    let first_id = first.source_id().clone();
    let second_id = second.source_id().clone();
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
fn rejects_a_handle_from_another_registry() {
    let first_registry = SourceRegistry::default();
    let second_registry = SourceRegistry::default();
    let first = first_registry
        .register(id(9), SourceKind::Direct)
        .expect("register");
    assert_eq!(
        second_registry.next_sequence(first.source_id(), &first),
        Err(RegistryError::CrossSource)
    );
}

#[test]
fn concurrent_reuse_of_a_candidate_invokes_only_one_writer() {
    use std::sync::Barrier;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let registry = Arc::new(SourceRegistry::default());
    let handle = Arc::new(
        registry
            .register(id(10), SourceKind::Direct)
            .expect("register"),
    );
    let source_id = handle.source_id().clone();
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
fn failed_atomic_writer_leaves_external_and_registry_state_unchanged() {
    use std::cell::Cell;

    let registry = SourceRegistry::default();
    let handle = registry
        .register(id(11), SourceKind::Direct)
        .expect("register");
    let source_id = handle.source_id().clone();
    let committed = Cell::new(0);
    assert_eq!(
        registry.apply_next(&source_id, &handle, 1, |_| {
            let staged = 1;
            if staged == 0 {
                committed.set(staged);
                Ok(())
            } else {
                Err(staged)
            }
        }),
        Err(SequenceApplyError::Writer(1))
    );
    assert_eq!(committed.get(), 0);
    assert_eq!(registry.next_sequence(&source_id, &handle), Ok(1));
}

#[test]
fn rejects_writer_reentrancy_without_deadlocking() {
    let registry = SourceRegistry::default();
    let handle = registry
        .register(id(12), SourceKind::Direct)
        .expect("register");
    let source_id = handle.source_id().clone();
    let result = registry.apply_next(&source_id, &handle, 1, |_| {
        assert_eq!(
            registry.next_sequence(&source_id, &handle),
            Err(RegistryError::ReentrantOperation)
        );
        assert_eq!(
            registry.register(id(13), SourceKind::Direct).err(),
            Some(RegistryError::ReentrantOperation)
        );
        Ok::<_, ()>(())
    });
    assert!(result.is_ok());
}

#[test]
fn ending_releases_state_and_a_new_lifetime_gets_a_new_id() {
    let registry = SourceRegistry::default();
    let first = registry
        .register(id(14), SourceKind::Direct)
        .expect("register");
    let first_id = first.source_id().clone();
    registry.end(&first_id, &first).expect("end");
    assert_eq!(
        registry.next_sequence(&first_id, &first),
        Err(RegistryError::Expired)
    );
    assert!(registry.state.lock().expect("state").sources.is_empty());

    let second = registry
        .register(first_id.clone(), SourceKind::Direct)
        .expect("new lifetime");
    assert_eq!(&first_id, second.source_id());
    assert_ne!(first.generation, second.generation);
    assert_eq!(registry.next_sequence(second.source_id(), &second), Ok(1));
}

#[test]
fn registry_memory_tracks_active_lifetimes_not_ended_ids() {
    let registry = SourceRegistry::default();
    for n in 0..4096 {
        let handle = registry
            .register(id(100 + n), SourceKind::Direct)
            .expect("register");
        let source_id = handle.source_id().clone();
        registry.end(&source_id, &handle).expect("end");
    }
    assert!(registry.state.lock().expect("state").sources.is_empty());
}

#[test]
fn sequence_overflow_ends_cleanly() {
    let registry = SourceRegistry::default();
    let handle = registry
        .register_watch(id(15), || Ok::<_, ()>(u64::MAX))
        .expect("register");
    assert_eq!(
        registry.next_sequence(handle.source_id(), &handle),
        Err(RegistryError::SequenceOverflow)
    );
    registry.end(handle.source_id(), &handle).expect("end");
    assert_eq!(
        registry.next_sequence(handle.source_id(), &handle),
        Err(RegistryError::Expired)
    );
}
