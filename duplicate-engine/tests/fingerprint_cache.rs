use du_duplicate_engine::{
    RegisteredHash,
    candidates::{EntryKind, FileIdentity, Placeholder, Size, Target},
    fingerprint_cache::{Decision, Error, FingerprintCache, ReuseCapabilities},
    verification::{OpenFile, ReadError, ReadPort, Snapshot, State, VerificationTarget},
};
use std::{num::NonZeroUsize, time::Duration};

fn hash() -> Result<RegisteredHash, String> {
    RegisteredHash::resolve("blake3", Some(1)).map_err(|e| format!("{e:?}"))
}
fn caps() -> ReuseCapabilities {
    ReuseCapabilities {
        change_stamp_supported: true,
        modified_resolution: Some(Duration::from_secs(2)),
    }
}
fn make_target(n: u8) -> VerificationTarget {
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
            modified: 10,
            change_stamp: Some(4),
        }),
    }
}
struct Reader {
    snapshot: Snapshot,
    bytes: [u8; 3],
    change_after: bool,
    fail: bool,
    opens: usize,
}
struct File<'a> {
    reader: &'a mut Reader,
    snapshots: usize,
}
impl ReadPort for Reader {
    fn open_bound<'a>(
        &'a mut self,
        _: u32,
        expected: &FileIdentity,
    ) -> Result<Box<dyn OpenFile + 'a>, ReadError> {
        self.opens += 1;
        if self.snapshot.identity != *expected || self.fail {
            return Err(ReadError::Failed);
        }
        Ok(Box::new(File {
            reader: self,
            snapshots: 0,
        }))
    }
}
impl OpenFile for File<'_> {
    fn snapshot(&mut self) -> Result<Snapshot, ReadError> {
        self.snapshots += 1;
        let mut snapshot = self.reader.snapshot.clone();
        if self.reader.change_after && self.snapshots > 1 {
            snapshot.change_stamp = Some(5);
        }
        Ok(snapshot)
    }
    fn read_at(&mut self, offset: u64, buffer: &mut [u8]) -> Result<usize, ReadError> {
        let start = usize::try_from(offset).map_err(|_| ReadError::Failed)?;
        let bytes = self.reader.bytes.get(start..).ok_or(ReadError::Failed)?;
        let count = bytes.len().min(buffer.len());
        buffer[..count].copy_from_slice(&bytes[..count]);
        Ok(count)
    }
}
fn reader(t: &VerificationTarget) -> Result<Reader, String> {
    Ok(Reader {
        snapshot: t.snapshot.clone().ok_or("missing snapshot")?,
        bytes: *b"abc",
        change_after: false,
        fail: false,
        opens: 0,
    })
}

#[test]
fn only_verified_complete_key_reuses_original_fingerprint() -> Result<(), String> {
    let cache = FingerprintCache::new(NonZeroUsize::MIN);
    let target = make_target(1);
    let hash = hash()?;
    let mut reader = reader(&target)?;
    assert!(RegisteredHash::resolve("other", Some(1)).is_err());
    assert!(RegisteredHash::resolve("blake3", Some(2)).is_err());
    assert_eq!(
        cache.decide(&target, caps(), &hash, &mut reader),
        Ok(Decision::ReadRequired)
    );
    cache
        .verify_and_record(&target, caps(), &hash, &mut reader, || Some(100), || false)
        .map_err(|e| format!("{e:?}"))?;
    let Decision::Reuse(saved) = cache
        .decide(&target, caps(), &hash, &mut reader)
        .map_err(|e| format!("{e:?}"))?
    else {
        return Err("missing reuse".into());
    };
    assert_eq!(saved.digest.len(), 32);
    assert_eq!(saved.verified_at_ms, 100);
    assert_eq!(reader.opens, 2);
    reader.snapshot.change_stamp = Some(9);
    assert_eq!(
        cache.decide(&target, caps(), &hash, &mut reader),
        Ok(Decision::ReadRequired)
    );
    reader.snapshot.change_stamp = Some(4);
    reader.change_after = true;
    assert_eq!(
        cache.decide(&target, caps(), &hash, &mut reader),
        Ok(Decision::ReadRequired)
    );
    reader.change_after = false;
    reader.fail = true;
    assert_eq!(
        cache.decide(&target, caps(), &hash, &mut reader),
        Ok(Decision::ReadRequired)
    );
    reader.fail = false;
    let mut changed = target.clone();
    changed.snapshot.as_mut().ok_or("snapshot")?.modified += 1;
    assert_eq!(
        cache.decide(&changed, caps(), &hash, &mut reader),
        Ok(Decision::ReadRequired)
    );
    changed = target.clone();
    changed.snapshot.as_mut().ok_or("snapshot")?.change_stamp = Some(5);
    assert_eq!(
        cache.decide(&changed, caps(), &hash, &mut reader),
        Ok(Decision::ReadRequired)
    );
    changed = target.clone();
    changed.snapshot.as_mut().ok_or("snapshot")?.change_stamp = None;
    assert_eq!(
        cache.decide(&changed, caps(), &hash, &mut reader),
        Ok(Decision::ReadRequired)
    );
    changed = target.clone();
    changed.target.identity = None;
    assert_eq!(
        cache.decide(&changed, caps(), &hash, &mut reader),
        Ok(Decision::ReadRequired)
    );
    changed = target.clone();
    changed.target.size = Size::Exact(4);
    assert_eq!(
        cache.decide(&changed, caps(), &hash, &mut reader),
        Ok(Decision::ReadRequired)
    );
    assert_eq!(
        cache.decide(&make_target(2), caps(), &hash, &mut reader),
        Ok(Decision::ReadRequired)
    );
    assert_eq!(
        cache.decide(
            &target,
            ReuseCapabilities {
                change_stamp_supported: false,
                ..caps()
            },
            &hash,
            &mut reader
        ),
        Ok(Decision::ReadRequired)
    );
    for resolution in [
        None,
        Some(Duration::ZERO),
        Some(Duration::from_millis(2001)),
    ] {
        assert_eq!(
            cache.decide(
                &target,
                ReuseCapabilities {
                    modified_resolution: resolution,
                    ..caps()
                },
                &hash,
                &mut reader
            ),
            Ok(Decision::ReadRequired)
        );
    }
    Ok(())
}

#[test]
fn failures_cancellation_capacity_and_clock_never_publish_unverified_data() -> Result<(), String> {
    let cache = FingerprintCache::new(NonZeroUsize::MIN);
    let a = make_target(1);
    let b = make_target(2);
    let hash = hash()?;
    let mut r = reader(&a)?;
    let mut failing = reader(&a)?;
    failing.change_after = true;
    assert_eq!(
        cache.verify_and_record(&a, caps(), &hash, &mut failing, || Some(1), || false),
        Err(Error::ReadFailed(State::Changed))
    );
    assert_eq!(
        cache.decide(&a, caps(), &hash, &mut r),
        Ok(Decision::ReadRequired)
    );
    assert_eq!(
        cache.verify_and_record(&a, caps(), &hash, &mut r, || None, || false),
        Err(Error::ClockUnavailable)
    );
    let mut calls = 0;
    assert_eq!(
        cache.verify_and_record(
            &a,
            caps(),
            &hash,
            &mut r,
            || Some(1),
            || {
                calls += 1;
                calls > 1
            }
        ),
        Err(Error::Cancelled)
    );
    assert_eq!(
        cache.decide(&a, caps(), &hash, &mut r),
        Ok(Decision::ReadRequired)
    );
    cache
        .verify_and_record(&a, caps(), &hash, &mut r, || Some(100), || false)
        .map_err(|e| format!("{e:?}"))?;
    cache
        .verify_and_record(&a, caps(), &hash, &mut r, || Some(99), || false)
        .map_err(|e| format!("{e:?}"))?;
    let Decision::Reuse(saved) = cache
        .decide(&a, caps(), &hash, &mut r)
        .map_err(|e| format!("{e:?}"))?
    else {
        return Err("missing".into());
    };
    assert_eq!(saved.verified_at_ms, 100);
    let mut rb = reader(&b)?;
    assert_eq!(
        cache.verify_and_record(&b, caps(), &hash, &mut rb, || Some(101), || false),
        Err(Error::Capacity)
    );
    assert_eq!(
        cache.decide(&b, caps(), &hash, &mut r),
        Ok(Decision::ReadRequired)
    );
    Ok(())
}
