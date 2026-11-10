use super::*;
use du_core_contracts::ports::{PortError, PortErrorKind};
use std::sync::{Barrier, mpsc};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    List,
    Tree,
    Search,
}
struct Ids;
impl JobIdentity for Ids {
    fn next_id(&self) -> Id {
        id(99)
    }
}
struct BlockingIds {
    entered: mpsc::Sender<()>,
    release: Mutex<mpsc::Receiver<()>>,
}
impl JobIdentity for BlockingIds {
    fn next_id(&self) -> Id {
        let _ = self.entered.send(());
        if let Ok(release) = self.release.lock() {
            let _ = release.recv();
        }
        id(99)
    }
}
struct Entropy(u8);
impl Random for Entropy {
    fn fill(&self, bytes: &mut [u8]) -> Result<(), PortError> {
        bytes.fill(self.0);
        Ok(())
    }
}
struct FailedEntropy;
impl Random for FailedEntropy {
    fn fill(&self, _: &mut [u8]) -> Result<(), PortError> {
        Err(PortError {
            kind: PortErrorKind::Io,
            reason_key: "os.query_failed".into(),
            os_code: None,
        })
    }
}
fn id(n: u8) -> Id {
    Id::from_v7_bytes([1, 2, 3, 4, 5, 6, 0x70, n, 0x80, 0, 0, 0, 0, 0, 0, n])
}
fn owner() -> Owner {
    Owner {
        session: id(1),
        window: id(2),
    }
}
fn make_registry(owner: Owner, limit: usize) -> Result<RegionRegistry<Kind>, String> {
    ok(RegionRegistry::new(
        owner,
        limit,
        Arc::new(Ids),
        &Entropy(23),
    ))
}
fn ok<T>(value: Result<T, RegionError>) -> Result<T, String> {
    value.map_err(|e| format!("{e:?}"))
}
fn promote(
    registry: &RegionRegistry<Kind>,
    owner: &Owner,
    region: &Id,
    kind: Kind,
    sequence: u32,
) -> Result<Promotion<Kind, ()>, String> {
    ok(registry.promote_validated(owner, region, kind, sequence, Ok::<_, ()>(())))?
        .map_err(|_| "invalid".into())
}

#[test]
fn thousands_of_retired_ids_leave_only_live_storage_and_never_reactivate() -> Result<(), String> {
    let owner = owner();
    let registry = make_registry(owner.clone(), 2)?;
    let mut prior = std::collections::HashSet::new();
    for _ in 0..5000 {
        let region = ok(registry.open(&owner, Kind::List))?;
        assert!(prior.insert(region.clone()));
        ok(registry.close(&owner, &region))?;
        assert!(matches!(
            registry.validate_region(&owner, &region, Kind::List),
            Err(RegionError::Unavailable)
        ));
        let table = registry.table.lock().map_err(|_| "poisoned")?;
        assert!(table.regions.is_empty());
        assert_eq!(table.regions.capacity(), 0);
    }
    let live = ok(registry.open(&owner, Kind::List))?;
    assert!(!prior.contains(&live));
    for _ in 0..5000 {
        let transient = ok(registry.open(&owner, Kind::Tree))?;
        assert!(prior.insert(transient.clone()));
        ok(registry.close(&owner, &transient))?;
        let table = registry.table.lock().map_err(|_| "poisoned")?;
        assert_eq!(table.regions.len(), 1);
        assert!(table.regions.capacity() <= 4);
    }
    assert_eq!(
        registry.table.lock().map_err(|_| "poisoned")?.regions.len(),
        1
    );
    Ok(())
}

#[test]
fn sequence_limit_rejects_wrap_without_stopping_current_request() -> Result<(), String> {
    let owner = owner();
    let registry = make_registry(owner.clone(), 1)?;
    let region = ok(registry.open(&owner, Kind::List))?;
    let current = promote(&registry, &owner, &region, Kind::List, u32::MAX)?;
    assert!(promote(&registry, &owner, &region, Kind::List, 0).is_err());
    assert!(promote(&registry, &owner, &region, Kind::List, u32::MAX).is_err());
    assert!(!current.permit.is_stopped());
    Ok(())
}

#[test]
fn a_closed_window_does_not_close_another_window_in_same_session() -> Result<(), String> {
    let first_owner = owner();
    let second_owner = Owner {
        session: first_owner.session.clone(),
        window: id(5),
    };
    let first: RegionRegistry<Kind> = ok(RegionRegistry::new(
        first_owner.clone(),
        1,
        Arc::new(Ids),
        &Entropy(23),
    ))?;
    let second: RegionRegistry<Kind> = ok(RegionRegistry::new(
        second_owner.clone(),
        1,
        Arc::new(Ids),
        &Entropy(24),
    ))?;
    let first_ref = ok(first.open(&first_owner, Kind::List))?;
    let second_ref = ok(second.open(&second_owner, Kind::List))?;
    assert_ne!(first_ref, second_ref);
    let other = promote(&second, &second_owner, &second_ref, Kind::List, 1)?;
    ok(first.close_session(&first_owner))?;
    assert!(matches!(
        first.open(&first_owner, Kind::List),
        Err(RegionError::Unavailable)
    ));
    assert!(!other.permit.is_stopped());
    assert!(matches!(
        second.validate_region(&first_owner, &second_ref, Kind::List),
        Err(RegionError::Unavailable)
    ));
    Ok(())
}

#[test]
fn ownership_kind_sequence_and_exact_targets_are_isolated() -> Result<(), String> {
    let owner = owner();
    let registry = make_registry(owner.clone(), 3)?;
    let list = ok(registry.open(&owner, Kind::List))?;
    let tree = ok(registry.open(&owner, Kind::Tree))?;
    let other_window = Owner {
        session: owner.session.clone(),
        window: id(3),
    };
    let other_session = Owner {
        session: id(4),
        window: owner.window.clone(),
    };
    assert!(matches!(
        registry.open(&other_window, Kind::List),
        Err(RegionError::Unavailable)
    ));
    assert!(matches!(
        registry.validate_region(&other_session, &list, Kind::List),
        Err(RegionError::Unavailable)
    ));
    assert!(matches!(
        registry.validate_region(&owner, &list, Kind::Search),
        Err(RegionError::WrongKind)
    ));
    assert!(promote(&registry, &owner, &list, Kind::List, 0).is_err());
    let first = promote(&registry, &owner, &list, Kind::List, 1)?;
    let second = promote(&registry, &owner, &tree, Kind::Tree, 1)?;
    assert!(promote(&registry, &owner, &list, Kind::List, 1).is_err());
    assert_eq!(
        ok(registry.cancel_request(&owner, &list, 1))?,
        CancelAction::Accepted
    );
    assert_eq!(
        ok(registry.cancel_request(&owner, &list, 1))?,
        CancelAction::NoEffect
    );
    assert!(first.permit.is_stopped());
    assert!(!second.permit.is_stopped());
    assert!(matches!(
        registry.cancel_request(&owner, &tree, 2),
        Err(RegionError::Unavailable)
    ));
    let future = promote(&registry, &owner, &tree, Kind::Tree, 2)?;
    assert!(!future.permit.is_stopped());
    Ok(())
}

#[test]
fn invalid_preparation_does_not_advance_or_cancel() -> Result<(), String> {
    let owner = owner();
    let registry = make_registry(owner.clone(), 1)?;
    let region = ok(registry.open(&owner, Kind::List))?;
    let old = promote(&registry, &owner, &region, Kind::List, 1)?;
    assert!(matches!(
        ok(registry.promote_validated(&owner, &region, Kind::List, 9, Err::<(), _>("bad")))?,
        Err("bad")
    ));
    assert!(!old.permit.is_stopped());
    let new = promote(&registry, &owner, &region, Kind::List, 2)?;
    assert!(old.permit.is_stopped());
    assert!(!new.permit.is_stopped());
    Ok(())
}

#[test]
fn delayed_preparation_and_job_receipt_are_rejected() -> Result<(), String> {
    let owner = owner();
    let registry = Arc::new(make_registry(owner.clone(), 1)?);
    let region = ok(registry.open(&owner, Kind::List))?;
    let old = promote(&registry, &owner, &region, Kind::List, 1)?;
    let gate = Arc::new(Barrier::new(2));
    let worker = {
        let gate = gate.clone();
        let permit = old.permit.clone();
        std::thread::spawn(move || {
            gate.wait();
            permit.bind_job(&id(11))
        })
    };
    let new = promote(&registry, &owner, &region, Kind::List, 2)?;
    gate.wait();
    assert!(matches!(
        worker.join().map_err(|_| "panic")?,
        Err(RegionError::Unavailable)
    ));
    assert!(promote(&registry, &owner, &region, Kind::List, 1).is_err());
    assert!(matches!(
        registry.cancel_job(&owner, &region, &id(11)),
        Err(RegionError::Unavailable)
    ));
    assert_eq!(ok(new.permit.publish(true, || Ok::<_, ()>(5)))?, Ok(5));
    Ok(())
}

#[test]
fn bound_jobs_fast_publish_and_close_are_exact() -> Result<(), String> {
    let owner = owner();
    let registry = make_registry(owner.clone(), 1)?;
    let region = ok(registry.open(&owner, Kind::List))?;
    let current = promote(&registry, &owner, &region, Kind::List, 1)?;
    let job = id(8);
    assert_eq!(ok(current.permit.publish(false, || Ok::<_, ()>(3)))?, Ok(3));
    assert!(current.permit.publish(false, || Ok::<_, ()>(4)).is_err());
    ok(current.permit.bind_job(&job))?;
    assert!(registry.cancel_job(&owner, &region, &id(9)).is_err());
    assert_eq!(
        ok(registry.cancel_request(&owner, &region, 1))?,
        CancelAction::Job(job.clone())
    );
    assert_eq!(ok(registry.close(&owner, &region))?, Some(job));
    assert!(current.permit.publish(false, || Ok::<_, ()>(())).is_err());
    Ok(())
}

#[test]
fn replacement_returns_only_the_previously_bound_job() -> Result<(), String> {
    let owner = owner();
    let registry = make_registry(owner.clone(), 2)?;
    let a = ok(registry.open(&owner, Kind::List))?;
    let b = ok(registry.open(&owner, Kind::Tree))?;
    let old = promote(&registry, &owner, &a, Kind::List, 1)?;
    let unrelated = promote(&registry, &owner, &b, Kind::Tree, 1)?;
    ok(old.permit.bind_job(&id(8)))?;
    ok(unrelated.permit.bind_job(&id(9)))?;
    let new = promote(&registry, &owner, &a, Kind::List, 2)?;
    assert_eq!(new.previous_job, Some(id(8)));
    assert!(old.permit.is_stopped());
    assert!(!unrelated.permit.is_stopped());
    assert!(matches!(
        registry.cancel_job(&owner, &a, &id(8)),
        Err(RegionError::Unavailable)
    ));
    assert_eq!(
        ok(registry.cancel_job(&owner, &b, &id(9)))?,
        CancelAction::Job(id(9))
    );
    Ok(())
}

#[test]
fn close_session_rejects_late_issuance_and_only_stops_its_owner() -> Result<(), String> {
    let owner = owner();
    let registry = Arc::new(make_registry(owner.clone(), 2)?);
    let region = ok(registry.open(&owner, Kind::List))?;
    let permit = promote(&registry, &owner, &region, Kind::List, 1)?.permit;
    let gate = Arc::new(Barrier::new(2));
    let worker = {
        let gate = gate.clone();
        let registry = registry.clone();
        let owner = owner.clone();
        std::thread::spawn(move || {
            gate.wait();
            registry.open(&owner, Kind::List)
        })
    };
    assert!(ok(registry.close_session(&owner))?.is_empty());
    gate.wait();
    assert!(matches!(
        worker.join().map_err(|_| "panic")?,
        Err(RegionError::Unavailable)
    ));
    assert!(permit.is_stopped());
    let other_owner = Owner {
        session: id(10),
        window: id(11),
    };
    let other = make_registry(other_owner.clone(), 1)?;
    let other_region = ok(other.open(&other_owner, Kind::Search))?;
    assert!(promote(&other, &other_owner, &other_region, Kind::Search, 1).is_ok());
    Ok(())
}

#[test]
fn issuance_in_progress_cannot_reopen_a_closed_session() -> Result<(), String> {
    let owner = owner();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let ids = Arc::new(BlockingIds {
        entered: entered_tx,
        release: Mutex::new(release_rx),
    });
    let registry = Arc::new(ok(RegionRegistry::new(
        owner.clone(),
        1,
        ids,
        &Entropy(23),
    ))?);
    let worker = {
        let registry = registry.clone();
        let owner = owner.clone();
        std::thread::spawn(move || registry.open(&owner, Kind::List))
    };
    entered_rx.recv().map_err(|e| e.to_string())?;
    assert!(ok(registry.close_session(&owner))?.is_empty());
    release_tx.send(()).map_err(|e| e.to_string())?;
    assert!(matches!(
        worker.join().map_err(|_| "panic")?,
        Err(RegionError::Unavailable)
    ));
    Ok(())
}

#[test]
fn no_global_lock_is_held_while_a_region_is_publishing() -> Result<(), String> {
    let owner = owner();
    let registry = Arc::new(make_registry(owner.clone(), 2)?);
    let region = ok(registry.open(&owner, Kind::List))?;
    let permit = promote(&registry, &owner, &region, Kind::List, 1)?.permit;
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        permit.publish(true, || {
            let _ = entered_tx.send(());
            let _ = release_rx.recv();
            Ok::<_, ()>(())
        })
    });
    entered_rx.recv().map_err(|e| e.to_string())?;
    let (done_tx, done_rx) = mpsc::channel();
    let r = registry.clone();
    let o = owner.clone();
    let opener = std::thread::spawn(move || {
        let _ = done_tx.send(r.open(&o, Kind::Tree));
    });
    let result = done_rx.recv_timeout(std::time::Duration::from_secs(2));
    let _ = release_tx.send(());
    opener.join().map_err(|_| "panic")?;
    worker
        .join()
        .map_err(|_| "panic")?
        .map_err(|e| format!("{e:?}"))?
        .map_err(|_| "publication failed")?;
    ok(result.map_err(|e| e.to_string())?)?;
    Ok(())
}

#[test]
fn entropy_failure_and_counter_exhaustion_fail_closed() -> Result<(), String> {
    let owner = owner();
    assert!(matches!(
        RegionRegistry::<Kind>::new(owner.clone(), 1, Arc::new(Ids), &FailedEntropy),
        Err(RegionError::Entropy)
    ));
    let registry = make_registry(owner.clone(), 1)?;
    registry.table.lock().map_err(|_| "poisoned")?.issued = 0x3fff_ffff_ffff_ffff;
    assert!(matches!(
        registry.open(&owner, Kind::List),
        Err(RegionError::Capacity)
    ));
    Ok(())
}

#[test]
fn closing_releases_region_before_external_job_gate() -> Result<(), String> {
    let owner = owner();
    let registry = make_registry(owner.clone(), 1)?;
    let region = ok(registry.open(&owner, Kind::List))?;
    let permit = promote(&registry, &owner, &region, Kind::List, 1)?.permit;
    let job = id(9);
    ok(permit.bind_job(&job))?;
    let gate = Arc::new(Mutex::new(()));
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let worker = {
        let gate = gate.clone();
        std::thread::spawn(move || {
            let _held = gate.lock().map_err(|_| "poisoned")?;
            let _ = entered_tx.send(());
            release_rx.recv().map_err(|_| "closed")?;
            Ok::<_, &str>(permit.publish(false, || Ok::<_, ()>(())))
        })
    };
    entered_rx.recv().map_err(|e| e.to_string())?;
    assert_eq!(ok(registry.close(&owner, &region))?, Some(job));
    release_tx.send(()).map_err(|e| e.to_string())?;
    assert!(matches!(
        worker.join().map_err(|_| "panic")?.map_err(str::to_owned)?,
        Err(RegionError::Unavailable)
    ));
    let _held = gate.lock().map_err(|_| "poisoned")?;
    Ok(())
}
