use du_core_contracts::Id;
use du_duplicate_engine::{
    RegisteredHash, RegisteredOutcome,
    candidates::FileIdentity,
    result_store::{ResultStore, StoreError},
    verification::{OpenFile, ReadError, ReadPort},
};

fn store() -> ResultStore {
    ResultStore::new(
        std::num::NonZeroUsize::MIN.saturating_add(15),
        std::num::NonZeroUsize::MIN.saturating_add(31),
    )
}
fn id(n: u8) -> Id {
    Id::from_v7_bytes([n; 16])
}
struct NoReads;
impl ReadPort for NoReads {
    fn open_bound<'a>(
        &'a mut self,
        _: u32,
        _: &FileIdentity,
    ) -> Result<Box<dyn OpenFile + 'a>, ReadError> {
        Err(ReadError::Failed)
    }
}
fn outcome() -> Result<RegisteredOutcome, String> {
    RegisteredHash::resolve("blake3", Some(1))
        .map_err(|e| format!("{e:?}"))?
        .verify(&[], &mut NoReads, || false)
        .map_err(|e| format!("{e:?}"))
}

#[test]
fn pins_are_immutable_and_latest_changes_atomically() -> Result<(), String> {
    let store = store();
    let source = id(1);
    store
        .register_source(source.clone())
        .map_err(|e| format!("{e:?}"))?;
    store
        .publish(&source, id(2), outcome()?)
        .map_err(|e| format!("{e:?}"))?;
    let first = store.pin(None).map_err(|e| format!("{e:?}"))?;
    store
        .publish(&source, id(3), outcome()?)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(first.version(), &id(2));
    assert!(first.outcome().evidence().groups.is_empty());
    assert_eq!(
        store.pin(None).map_err(|e| format!("{e:?}"))?.version(),
        &id(3)
    );
    assert_eq!(
        store
            .pin(Some(&id(2)))
            .map_err(|e| format!("{e:?}"))?
            .version(),
        &id(2)
    );
    Ok(())
}

#[test]
fn source_retirement_fences_admission_preserves_pins_and_restores_latest() -> Result<(), String> {
    let store = store();
    let a = id(1);
    let b = id(2);
    store
        .register_source(a.clone())
        .map_err(|e| format!("{e:?}"))?;
    store
        .register_source(b.clone())
        .map_err(|e| format!("{e:?}"))?;
    store
        .publish(&a, id(3), outcome()?)
        .map_err(|e| format!("{e:?}"))?;
    store
        .publish(&b, id(4), outcome()?)
        .map_err(|e| format!("{e:?}"))?;
    let pinned = store.pin(None).map_err(|e| format!("{e:?}"))?;
    store.retire_source(&b).map_err(|e| format!("{e:?}"))?;
    assert!(matches!(store.pin(Some(&id(4))), Err(StoreError::Stale)));
    assert_eq!(pinned.source(), &b);
    assert_eq!(
        store.pin(None).map_err(|e| format!("{e:?}"))?.version(),
        &id(3)
    );
    assert!(matches!(
        store.register_source(b.clone()),
        Err(StoreError::SourceUnavailable)
    ));
    assert!(matches!(
        store.publish(&b, id(5), outcome()?),
        Err(StoreError::SourceUnavailable)
    ));
    assert!(matches!(
        store.publish(&a, id(4), outcome()?),
        Err(StoreError::DuplicateVersion)
    ));
    Ok(())
}

#[test]
fn missing_duplicate_and_invalid_publications_leave_latest_unchanged() -> Result<(), String> {
    let store = store();
    assert!(matches!(store.pin(None), Err(StoreError::NotFound)));
    assert!(matches!(store.pin(Some(&id(9))), Err(StoreError::NotFound)));
    assert!(matches!(
        store.publish(&id(1), id(2), outcome()?),
        Err(StoreError::SourceUnavailable)
    ));
    store.register_source(id(1)).map_err(|e| format!("{e:?}"))?;
    store
        .publish(&id(1), id(2), outcome()?)
        .map_err(|e| format!("{e:?}"))?;
    assert!(matches!(
        store.publish(&id(1), id(2), outcome()?),
        Err(StoreError::DuplicateVersion)
    ));
    assert_eq!(
        store.pin(None).map_err(|e| format!("{e:?}"))?.version(),
        &id(2)
    );
    Ok(())
}

#[test]
fn retirement_racing_publication_cannot_leave_a_live_result() -> Result<(), String> {
    use std::sync::{Arc, Barrier};
    let store = Arc::new(store());
    let gate = Arc::new(Barrier::new(2));
    store.register_source(id(1)).map_err(|e| format!("{e:?}"))?;
    let worker_store = Arc::clone(&store);
    let worker_gate = Arc::clone(&gate);
    let candidate = outcome()?;
    let worker = std::thread::spawn(move || {
        worker_gate.wait();
        worker_store.publish(&id(1), id(2), candidate)
    });
    gate.wait();
    store.retire_source(&id(1)).map_err(|e| format!("{e:?}"))?;
    let published = worker.join().map_err(|_| "worker panicked")?;
    match published {
        Ok(()) => assert!(matches!(store.pin(Some(&id(2))), Err(StoreError::Stale))),
        Err(StoreError::SourceUnavailable) => {
            assert!(matches!(store.pin(Some(&id(2))), Err(StoreError::NotFound)))
        }
        Err(other) => return Err(format!("unexpected error: {other:?}")),
    }
    assert!(matches!(store.pin(None), Err(StoreError::NotFound)));
    Ok(())
}

#[test]
fn published_verified_group_is_read_from_the_pinned_outcome() -> Result<(), String> {
    use du_duplicate_engine::candidates::{EntryKind, Placeholder, Size, Target};
    use du_duplicate_engine::verification::{Snapshot, State, VerificationTarget};
    struct File {
        snapshot: Snapshot,
    }
    impl OpenFile for File {
        fn snapshot(&mut self) -> Result<Snapshot, ReadError> {
            Ok(self.snapshot.clone())
        }
        fn read_at(&mut self, offset: u64, buffer: &mut [u8]) -> Result<usize, ReadError> {
            let bytes = b"abc".get(offset as usize..).unwrap_or_default();
            let count = bytes.len().min(buffer.len());
            buffer[..count].copy_from_slice(&bytes[..count]);
            Ok(count)
        }
    }
    struct Port;
    impl ReadPort for Port {
        fn open_bound<'a>(
            &'a mut self,
            _: u32,
            identity: &FileIdentity,
        ) -> Result<Box<dyn OpenFile + 'a>, ReadError> {
            Ok(Box::new(File {
                snapshot: Snapshot {
                    identity: identity.clone(),
                    size: 3,
                    modified: 7,
                    change_stamp: Some(1),
                },
            }))
        }
    }
    let inputs: Vec<_> = (1..=2)
        .map(|n| {
            let identity = FileIdentity {
                volume: vec![1],
                file: vec![n],
            };
            VerificationTarget {
                target: Target {
                    item_no: u32::from(n),
                    kind: EntryKind::File,
                    size: Size::Exact(3),
                    identity: Some(identity.clone()),
                    placeholder: Placeholder::Local,
                },
                snapshot: Some(Snapshot {
                    identity,
                    size: 3,
                    modified: 7,
                    change_stamp: Some(1),
                }),
            }
        })
        .collect();
    let registered = RegisteredHash::resolve("blake3", Some(1))
        .map_err(|e| format!("{e:?}"))?
        .verify(&inputs, &mut Port, || false)
        .map_err(|e| format!("{e:?}"))?;
    let store = store();
    store.register_source(id(1)).map_err(|e| format!("{e:?}"))?;
    store
        .publish(&id(1), id(2), registered)
        .map_err(|e| format!("{e:?}"))?;
    let pin = store.pin(Some(&id(2))).map_err(|e| format!("{e:?}"))?;
    let evidence = pin.outcome().evidence();
    assert_eq!(evidence.groups.len(), 1);
    assert_eq!(evidence.states[&1], State::Verified);
    assert_eq!(evidence.groups[0].files.len(), 2);
    Ok(())
}

#[test]
fn bounded_admission_releases_live_slots_but_preserves_identifier_tombstones() -> Result<(), String>
{
    use std::num::NonZeroUsize;
    let store = ResultStore::new(NonZeroUsize::MIN, NonZeroUsize::MIN.saturating_add(3));
    store.register_source(id(1)).map_err(|e| format!("{e:?}"))?;
    store.register_source(id(2)).map_err(|e| format!("{e:?}"))?;
    store
        .publish(&id(1), id(3), outcome()?)
        .map_err(|e| format!("{e:?}"))?;
    let pin = store.pin(None).map_err(|e| format!("{e:?}"))?;
    assert!(matches!(
        store.publish(&id(2), id(4), outcome()?),
        Err(StoreError::Capacity)
    ));
    assert_eq!(
        store.pin(None).map_err(|e| format!("{e:?}"))?.version(),
        &id(3)
    );
    assert!(matches!(store.pin(Some(&id(4))), Err(StoreError::NotFound)));
    store.retire_source(&id(1)).map_err(|e| format!("{e:?}"))?;
    store
        .publish(&id(2), id(4), outcome()?)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(pin.version(), &id(3));
    assert!(matches!(store.pin(Some(&id(3))), Err(StoreError::Stale)));
    assert!(matches!(
        store.register_source(id(5)),
        Err(StoreError::Capacity)
    ));
    store.register_source(id(2)).map_err(|e| format!("{e:?}"))?;
    store.retire_source(&id(2)).map_err(|e| format!("{e:?}"))?;
    assert!(matches!(
        store.register_source(id(5)),
        Err(StoreError::Capacity)
    ));
    Ok(())
}

#[test]
fn identifier_limit_blocks_publish_even_when_live_capacity_remains() -> Result<(), String> {
    use std::num::NonZeroUsize;
    let store = ResultStore::new(
        NonZeroUsize::MIN.saturating_add(2),
        NonZeroUsize::MIN.saturating_add(2),
    );
    store.register_source(id(1)).map_err(|e| format!("{e:?}"))?;
    store
        .publish(&id(1), id(2), outcome()?)
        .map_err(|e| format!("{e:?}"))?;
    store
        .publish(&id(1), id(3), outcome()?)
        .map_err(|e| format!("{e:?}"))?;
    assert!(matches!(
        store.publish(&id(1), id(4), outcome()?),
        Err(StoreError::Capacity)
    ));
    assert!(matches!(store.pin(Some(&id(4))), Err(StoreError::NotFound)));
    assert_eq!(
        store.pin(None).map_err(|e| format!("{e:?}"))?.version(),
        &id(3)
    );
    Ok(())
}
