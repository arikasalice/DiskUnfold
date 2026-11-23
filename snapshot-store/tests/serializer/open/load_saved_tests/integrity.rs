use super::*;

#[test]
fn resealed_snapshot_still_must_match_index_body_hash() -> Result<(), String> {
    let (_, snapshot_id, mut bytes) = sample_file()?;
    let expected_body_hash = index_body_hash(&bytes)?;
    let size = u64::try_from(bytes.len()).map_err(|error| error.to_string())?;

    let metadata_len = usize::try_from(u32::from_le_bytes(
        bytes[12..16]
            .try_into()
            .map_err(|_| "snapshot header is truncated")?,
    ))
    .map_err(|error| error.to_string())?;
    let metadata_end = 16_usize
        .checked_add(metadata_len)
        .ok_or("snapshot metadata length overflowed")?;
    let app_version = b"\"appVersion\":\"";
    let version_offset = bytes[16..metadata_end]
        .windows(app_version.len())
        .position(|window| window == app_version)
        .ok_or("snapshot app version metadata is missing")?
        + 16
        + app_version.len();
    bytes[version_offset] = if bytes[version_offset] == b'9' {
        b'8'
    } else {
        b'9'
    };

    let footer_start = bytes.len() - 32;
    let resealed_hash = blake3::hash(&bytes[..footer_start]);
    bytes[footer_start..].copy_from_slice(resealed_hash.as_bytes());
    assert_ne!(resealed_hash.as_bytes(), &expected_body_hash);

    let port = FakePort::new(Some(bytes), Some(1_000_000));
    let discarded = Arc::new(AtomicBool::new(false));
    let error = du_snapshot_store::load_saved(
        &port,
        &snapshot_id,
        size,
        expected_body_hash,
        TrackedStage::new(discarded.clone()),
        || false,
    )
    .err()
    .ok_or("snapshot with a regenerated footer bypassed the index digest")?;

    assert_eq!(error.reason_key(), Some("snapshot.integrity_failed"));
    assert!(discarded.load(Ordering::SeqCst));
    assert!(!port.state().capture_open);
    Ok(())
}
#[test]
fn cancellation_and_insufficient_space_leave_no_capture() -> Result<(), String> {
    let (_, snapshot_id, bytes) = sample_file()?;
    let size = u64::try_from(bytes.len()).map_err(|error| error.to_string())?;
    let body_hash = index_body_hash(&bytes)?;
    let port = FakePort::new(Some(bytes.clone()), Some(1_000_000));
    let discarded = Arc::new(AtomicBool::new(false));
    let mut polls = 0;
    let error = du_snapshot_store::load_saved(
        &port,
        &snapshot_id,
        size,
        body_hash,
        TrackedStage::new(discarded.clone()),
        || {
            polls += 1;
            polls >= 3
        },
    )
    .err()
    .ok_or("cancelled load unexpectedly succeeded")?;
    assert_eq!(error.reason_key(), None);
    assert_eq!(
        error.kind(),
        du_snapshot_store::LoadSavedErrorKind::Cancelled
    );
    assert!(discarded.load(Ordering::SeqCst));
    assert!(!port.state().capture_open);
    assert_eq!(port.state().registered, Some(bytes.clone()));

    let port = FakePort::new(Some(bytes), Some(0));
    let discarded = Arc::new(AtomicBool::new(false));
    let error = du_snapshot_store::load_saved(
        &port,
        &snapshot_id,
        size,
        body_hash,
        TrackedStage::new(discarded.clone()),
        || false,
    )
    .err()
    .ok_or("insufficient space unexpectedly succeeded")?;
    assert_eq!(error.reason_key(), Some("snapshot.insufficient_space"));
    assert!(discarded.load(Ordering::SeqCst));
    assert!(!port.state().capture_open);
    Ok(())
}
