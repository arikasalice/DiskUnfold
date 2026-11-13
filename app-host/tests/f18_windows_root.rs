#![cfg(all(windows, feature = "gui"))]
use du_core_contracts::{change_notify::WatchEvent, scan_observation::ScopeRootPort};
use du_engine_api::watch_source::{
    Action, Capability, Completion, Definition, GapReason, Registration, Registry, State, Support,
};
use du_engine_api::{
    watch_gap::{Ledger, Outcome},
    watch_source::deliver,
};
use du_platform_os::{
    windows_observation::WindowsScopeRoots,
    windows_watch_root::{RecursiveScopeEvidence, RootRegistration, RootWatch},
};
use std::{
    fs,
    os::windows::ffi::OsStrExt,
    thread,
    time::{Duration, Instant},
};

/// Native root signals reach F33 while unverified descendant coverage stays open.
#[test]
fn actual_root_event_reaches_consumer() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("root");
    fs::create_dir(&path)?;
    let native: Vec<u8> = path
        .as_os_str()
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect();
    let root = WindowsScopeRoots
        .open_root(&native)
        .map_err(|e| format!("{e:?}"))?;
    assert!(RootWatch::start(&path, &root.volume_id, &[0; 8]).is_err());
    let mut native_watch =
        RootWatch::start(&path, &root.volume_id, &root.file_id).map_err(|e| format!("{e:?}"))?;
    let evidence = native_watch.registration_evidence();
    assert_eq!(evidence.root_registration, RootRegistration::Submitted);
    let uncovered = match evidence.recursive_scope {
        RecursiveScopeEvidence::NotEstablished => vec![vec![]],
    };
    let definition = Definition {
        watch_ref: [3; 16],
        scope_revision: [1; 16],
        target: vec![],
        entry_path: native,
        volume_path: vec![],
        nested_mounts: vec![],
        volume: root.volume_id,
        baseline_end: Some(0),
    };
    let mut registry = Registry::new(1, 1, Default::default()).map_err(|e| format!("{e:?}"))?;
    let caps = Capability {
        notification: Support::Supported,
        loss: Support::Supported,
        interruption: Support::Supported,
        path: Support::Unsupported,
        resource_limit: Support::Supported,
    };
    let started = registry
        .start(definition.clone(), &definition.volume, caps, 1)
        .map_err(|e| format!("{e:?}"))?;
    let token = started
        .actions
        .iter()
        .find_map(|v| {
            if let Action::Register { token, .. } = v {
                Some(*token)
            } else {
                None
            }
        })
        .ok_or("no registration")?;
    registry
        .registration(
            [3; 16],
            token,
            1,
            &Registration::Acquired {
                handle: 7,
                uncovered,
            },
            2,
        )
        .map_err(|e| format!("{e:?}"))?;
    let mut consumer = Ledger::new([1; 16]);
    consumer
        .register::<()>(definition)
        .map_err(|e| format!("{e:?}"))?;
    let snapshot = registry
        .incremental_snapshot([3; 16], 3)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(snapshot.state, State::Degraded);
    assert!(
        snapshot
            .gaps
            .iter()
            .any(|gap| { gap.reason == GapReason::PartialCapability && gap.end.is_none() })
    );
    assert_eq!(
        consumer
            .apply_snapshot(&snapshot, &mut |_| Ok::<(), ()>(()))
            .map_err(|e| format!("{e:?}"))?,
        Outcome::Applied
    );
    fs::write(path.join("actual-change"), b"x")?;
    let deadline = Instant::now() + Duration::from_secs(10);
    let event = loop {
        if let Some(event) = native_watch.poll(4)? {
            break event;
        }
        if Instant::now() >= deadline {
            return Err("notification timeout".into());
        }
        thread::sleep(Duration::from_millis(10));
    };
    assert!(matches!(event, WatchEvent::Signal { .. }));
    deliver(&mut registry, [3; 16], token, 1, &event).map_err(|e| format!("{e:?}"))?;
    let actions = registry
        .tick([3; 16], token, 1, 1004)
        .map_err(|e| format!("{e:?}"))?;
    let mut delivered = 0;
    for action in actions {
        if let Action::Changes(batch) = action {
            assert!(!batch.changes.is_empty());
            assert_eq!(
                consumer
                    .apply(&batch, &mut |_| Ok::<(), ()>(()))
                    .map_err(|e| format!("{e:?}"))?,
                Outcome::Applied
            );
            delivered += 1;
        }
    }
    assert!(delivered > 0);
    let loss = WatchEvent::Loss { observed_at: 1005 };
    for action in deliver(&mut registry, [3; 16], token, 1, &loss).map_err(|e| format!("{e:?}"))? {
        if let Action::Changes(batch) = action {
            consumer
                .apply(&batch, &mut |_| Ok::<(), ()>(()))
                .map_err(|e| format!("{e:?}"))?;
        }
    }
    assert!(
        consumer
            .unit(&[3; 16])
            .ok_or("missing consumer")?
            .gaps
            .values()
            .any(|gap| gap.reason == GapReason::SignalLost)
    );
    let stopped = registry.stop([3; 16], 1006).map_err(|e| format!("{e:?}"))?;
    assert!(
        stopped
            .iter()
            .any(|action| matches!(action, Action::Remove { .. }))
    );
    native_watch.stop()?;
    registry
        .removed([3; 16], token, 7, Completion::Removed, 1007)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        registry.snapshot([3; 16]).map_err(|e| format!("{e:?}"))?.0,
        State::Stopped
    );
    fs::rename(path, temp.path().join("released"))?;
    println!(
        "verified root -> actual Windows event -> registry batch -> F33 consumer; synthetic loss; stop and identity mismatch passed"
    );
    Ok(())
}
