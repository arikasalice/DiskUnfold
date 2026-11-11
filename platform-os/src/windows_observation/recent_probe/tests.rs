use super::*;
use std::{cell::Cell, fs, os::windows::ffi::OsStrExt};

#[test]
fn ntfs_duplicated_handle_observation_does_not_disturb_scan_batches_in_fixture()
-> Result<(), Box<dyn std::error::Error>> {
    use std::{collections::BTreeSet, sync::mpsc, time::Duration};
    let temp = tempfile::tempdir()?;
    let target = temp.path().join("scan");
    fs::create_dir(&target)?;
    let mut expected = BTreeSet::new();
    for index in 0..600 {
        let name = format!("entry-{index:04}");
        fs::write(target.join(&name), b"x")?;
        expected.insert(
            name.encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        );
    }
    let raw = target
        .as_os_str()
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    let (mut root, probe) = WindowsScopeRoots
        .open_with_recent_probe(&raw)
        .map_err(|e| format!("{e:?}"))?;
    let probe = probe.map_err(|e| format!("{e:?}"))?;
    let expected_volume = root.volume_id.clone();
    let expected_file = root.file_id.clone();
    let (request_tx, request_rx) = mpsc::channel();
    let (progress_tx, progress_rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let stop = AtomicBool::new(false);
        while request_rx.recv().is_ok() {
            let result = match probe.observe(&stop) {
                ProbeOutcome::Observed(evidence)
                    if evidence.volume_id == expected_volume
                        && evidence.file_id == expected_file =>
                {
                    Ok(())
                }
                other => Err(format!("unexpected probe result: {other:?}")),
            };
            if progress_tx.send(result).is_err() {
                break;
            }
        }
    });
    let mut received = BTreeSet::new();
    let mut batches = 0usize;
    loop {
        let entries = root
            .directory
            .next_batch()
            .map_err(|error| format!("{error:?}"))?;
        if entries.is_empty() {
            break;
        }
        batches += 1;
        for item in entries {
            assert!(received.insert(item.name), "duplicate scan entry");
        }
        // Exercise handle observation between successive scan batches.
        request_tx.send(())?;
        progress_rx.recv_timeout(Duration::from_secs(10))??;
    }
    drop(request_tx);
    worker.join().map_err(|_| "worker panic")?;
    assert_eq!(received, expected);
    assert!(
        batches > 1,
        "fixture did not exercise multiple scan batches"
    );
    eprintln!(
        "NTFS fixture: {} entries, {batches} scan batches and probe observations",
        received.len()
    );
    Ok(())
}

#[test]
fn ntfs_same_handle_probe_preserves_scan_and_rejects_replaced_path()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let target = temp.path().join("scan");
    fs::create_dir(&target)?;
    fs::write(target.join("file"), b"x")?;
    let raw = target
        .as_os_str()
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    let (mut root, probe) = WindowsScopeRoots
        .open_with_recent_probe(&raw)
        .map_err(|e| format!("{e:?}"))?;
    let probe = probe.map_err(|e| format!("{e:?}"))?;
    let stop = AtomicBool::new(false);
    let ProbeOutcome::Observed(evidence) = probe.observe(&stop) else {
        return Err("missing observation".into());
    };
    assert_eq!(evidence.volume_id, root.volume_id);
    assert_eq!(evidence.file_id, root.file_id);
    let mut path = evidence.entry_path.raw.bytes()?;
    let relative = evidence.relative_path.raw.bytes()?;
    assert!(path.ends_with(&[b'\\', 0]) && relative.starts_with(&[b'\\', 0]));
    path.extend_from_slice(&relative[2..]);
    assert_eq!(path, root.canonical_path.raw.bytes()?);
    assert_eq!(
        root.directory
            .next_batch()
            .map_err(|e| format!("{e:?}"))?
            .len(),
        1
    );
    fs::rename(&target, temp.path().join("old"))?;
    fs::create_dir(&target)?;
    assert!(matches!(
        probe.observe(&stop),
        ProbeOutcome::Unavailable(PortError {
            kind: PortErrorKind::IdentityChanged,
            ..
        })
    ));
    Ok(())
}

#[test]
fn injected_failure_and_cancellation_never_return_evidence()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let raw = temp
        .path()
        .as_os_str()
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    let (mut retained, failed) = open_with_probe_using(&raw, |_| {
        Err(std::io::Error::other("injected duplicate failure"))
    })
    .map_err(|e| format!("{e:?}"))?;
    assert!(failed.is_err());
    assert!(retained.directory.next_batch().is_ok());
    let (mut retained, observer) = open_port_with_recent_using(&raw, |_| {
        Err(std::io::Error::other("injected duplicate failure"))
    })
    .map_err(|e| format!("{e:?}"))?;
    assert!(observer.ok_or("missing creation outcome")?.is_err());
    assert!(retained.directory.next_batch().is_ok());
    let (_root, probe) = WindowsScopeRoots
        .open_with_recent_probe(&raw)
        .map_err(|e| format!("{e:?}"))?;
    let probe = probe.map_err(|e| format!("{e:?}"))?;
    let stop = AtomicBool::new(true);
    let called = Cell::new(false);
    assert_eq!(
        probe.observe_with(
            &stop,
            |_| {
                called.set(true);
                Err(invalid())
            },
            || {}
        ),
        ProbeOutcome::Cancelled
    );
    assert!(!called.get());
    stop.store(false, Ordering::Release);
    let failure = probe.observe_with(
        &stop,
        |_| {
            Err(PortError {
                kind: PortErrorKind::Unsupported,
                reason_key: "os.unsupported".into(),
                os_code: None,
            })
        },
        || {},
    );
    assert!(matches!(
        failure,
        ProbeOutcome::Unavailable(PortError {
            kind: PortErrorKind::Unsupported,
            ..
        })
    ));
    let cancelled_during = probe.observe_with(
        &stop,
        |handle| {
            let result = final_path(handle, VOLUME_NAME_GUID);
            stop.store(true, Ordering::Release);
            result
        },
        || {},
    );
    assert_eq!(cancelled_during, ProbeOutcome::Cancelled);
    stop.store(false, Ordering::Release);
    let cancelled_after = probe.observe_with(
        &stop,
        |handle| final_path(handle, VOLUME_NAME_GUID),
        || stop.store(true, Ordering::Release),
    );
    assert_eq!(cancelled_after, ProbeOutcome::Cancelled);
    Ok(())
}

#[test]
fn port_default_and_opt_in_use_one_open_and_only_opt_in_duplicates()
-> Result<(), Box<dyn std::error::Error>> {
    use du_core_contracts::scan_observation::{OpenedScopeRoot, ScopeRootPort};
    struct DefaultPort;
    impl ScopeRootPort for DefaultPort {
        fn open_root(&self, path: &[u8]) -> Result<OpenedScopeRoot, PortError> {
            WindowsScopeRoots.open_root(path)
        }
    }
    let temp = tempfile::tempdir()?;
    let raw = temp
        .path()
        .as_os_str()
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    scope::OPEN_COUNT.with(|n| n.set(0));
    DUPLICATE_COUNT.with(|n| n.set(0));
    let _root = WindowsScopeRoots
        .open_root(&raw)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(scope::OPEN_COUNT.with(Cell::get), 1);
    assert_eq!(DUPLICATE_COUNT.with(Cell::get), 0);

    scope::OPEN_COUNT.with(|n| n.set(0));
    let (_root, observer) = DefaultPort
        .open_root_with_recent(&raw)
        .map_err(|e| format!("{e:?}"))?;
    assert!(observer.is_none());
    assert_eq!(scope::OPEN_COUNT.with(Cell::get), 1);
    assert_eq!(DUPLICATE_COUNT.with(Cell::get), 0);

    scope::OPEN_COUNT.with(|n| n.set(0));
    let (root, observer) = WindowsScopeRoots
        .open_root_with_recent(&raw)
        .map_err(|e| format!("{e:?}"))?;
    let observer = observer
        .ok_or("missing observer")?
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(scope::OPEN_COUNT.with(Cell::get), 1);
    assert_eq!(DUPLICATE_COUNT.with(Cell::get), 1);
    let cancelled = AtomicBool::new(true);
    assert_eq!(observer.observe(&cancelled), ProbeOutcome::Cancelled);
    cancelled.store(false, Ordering::Release);
    let ProbeOutcome::Observed(evidence) = observer.observe(&cancelled) else {
        return Err("missing observed evidence".into());
    };
    assert_eq!(root.volume_id, evidence.volume_id);
    assert_eq!(root.file_id, evidence.file_id);
    Ok(())
}

#[test]
fn port_observer_rejects_actual_replacement_without_invalidating_open_root()
-> Result<(), Box<dyn std::error::Error>> {
    use du_core_contracts::scan_observation::ScopeRootPort;
    let temp = tempfile::tempdir()?;
    let target = temp.path().join("root");
    fs::create_dir(&target)?;
    let raw = target
        .as_os_str()
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    let (mut root, observer) = WindowsScopeRoots
        .open_root_with_recent(&raw)
        .map_err(|e| format!("{e:?}"))?;
    let observer = observer
        .ok_or("missing observer")?
        .map_err(|e| format!("{e:?}"))?;
    fs::rename(&target, temp.path().join("old"))?;
    fs::create_dir(&target)?;
    assert!(matches!(
        observer.observe(&AtomicBool::new(false)),
        ProbeOutcome::Unavailable(PortError {
            kind: PortErrorKind::IdentityChanged,
            ..
        })
    ));
    assert!(root.directory.next_batch().is_ok());
    Ok(())
}
