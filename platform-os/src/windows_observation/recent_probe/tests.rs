use super::*;
use std::{cell::Cell, fs, os::windows::ffi::OsStrExt};

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
