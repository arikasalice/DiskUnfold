use du_duplicate_engine::{
    RegisteredHash,
    candidates::{EntryKind, FileIdentity, Placeholder, Size, Target},
    fingerprint_cache::{FingerprintCache, ReuseCapabilities},
    verification::{OpenFile, ReadError, ReadPort, Snapshot, State, VerificationTarget},
};
use std::{
    collections::BTreeMap,
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

fn target(n: u8) -> VerificationTarget {
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
}
fn caps(_: &FileIdentity) -> Option<ReuseCapabilities> {
    Some(ReuseCapabilities {
        change_stamp_supported: true,
        modified_resolution: Some(Duration::from_millis(1)),
    })
}
#[derive(Default)]
struct Reader {
    data: BTreeMap<u32, [u8; 3]>,
    bytes: u64,
    opens: u64,
    fail: Option<u32>,
    cancel: Option<Arc<AtomicBool>>,
    stamps: BTreeMap<u32, u64>,
}
struct File<'a> {
    owner: &'a mut Reader,
    id: u32,
    expected: FileIdentity,
}
impl ReadPort for Reader {
    fn open_bound<'a>(
        &'a mut self,
        id: u32,
        expected: &FileIdentity,
    ) -> Result<Box<dyn OpenFile + 'a>, ReadError> {
        self.opens += 1;
        if self.fail == Some(id) || !self.data.contains_key(&id) {
            return Err(ReadError::Failed);
        }
        Ok(Box::new(File {
            owner: self,
            id,
            expected: expected.clone(),
        }))
    }
}
impl OpenFile for File<'_> {
    fn snapshot(&mut self) -> Result<Snapshot, ReadError> {
        Ok(Snapshot {
            identity: self.expected.clone(),
            size: 3,
            modified: 7,
            change_stamp: Some(self.owner.stamps.get(&self.id).copied().unwrap_or(1)),
        })
    }
    fn read_at(&mut self, offset: u64, buffer: &mut [u8]) -> Result<usize, ReadError> {
        let data = self.owner.data.get(&self.id).ok_or(ReadError::Failed)?;
        let start = usize::try_from(offset).map_err(|_| ReadError::Failed)?;
        let bytes = data.get(start..).ok_or(ReadError::Failed)?;
        let count = bytes.len().min(buffer.len());
        buffer[..count].copy_from_slice(&bytes[..count]);
        self.owner.bytes += count as u64;
        if count > 0
            && let Some(cancel) = &self.owner.cancel
        {
            cancel.store(true, Ordering::SeqCst);
        }
        Ok(count)
    }
}
fn make_reader() -> Reader {
    Reader {
        data: [(1, *b"abc"), (2, *b"abc"), (3, *b"xyz")].into(),
        ..Reader::default()
    }
}
fn hash() -> Result<RegisteredHash, String> {
    RegisteredHash::resolve("blake3", Some(1)).map_err(|e| format!("{e:?}"))
}

#[test]
fn pipeline_reuses_verified_hash_but_recompares_current_members() -> Result<(), String> {
    let cache = FingerprintCache::new(NonZeroUsize::MIN.saturating_add(1));
    let hash = hash()?;
    let mut reader = make_reader();
    let first = cache
        .verify_with_reuse(
            &[target(1), target(2)],
            &mut reader,
            &hash,
            &du_duplicate_engine::fingerprint_cache::ReuseInputs {
                capabilities: &caps,
                clock: &|| Some(100),
                scope_proven: true,
            },
            || false,
        )
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(reader.bytes, 6);
    assert_eq!(reader.opens, 2);
    assert_eq!(first.evidence().full_reads.observed, 6);
    assert_eq!(first.evidence().groups.len(), 1);
    assert!(!first.evidence().full_fingerprints[&1].reused);
    assert_eq!(
        first.evidence().full_fingerprints[&1].verified_at_ms,
        Some(100)
    );
    reader.bytes = 0;
    reader.opens = 0;
    let second = cache
        .verify_with_reuse(
            &[target(1), target(2), target(3)],
            &mut reader,
            &hash,
            &du_duplicate_engine::fingerprint_cache::ReuseInputs {
                capabilities: &caps,
                clock: &|| Some(200),
                scope_proven: true,
            },
            || false,
        )
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(reader.bytes, 3);
    assert_eq!(second.evidence().full_reads.observed, 3);
    assert!(second.evidence().full_fingerprints[&1].reused);
    assert!(second.evidence().full_fingerprints[&2].reused);
    assert_eq!(
        second.evidence().full_fingerprints[&1].verified_at_ms,
        Some(100)
    );
    assert!(!second.evidence().full_fingerprints[&3].reused);
    assert_eq!(second.evidence().states[&3], State::Distinct);
    assert_eq!(second.evidence().groups[0].files.len(), 2);
    reader.bytes = 0;
    let third = cache
        .verify_with_reuse(
            &[target(1), target(3)],
            &mut reader,
            &hash,
            &du_duplicate_engine::fingerprint_cache::ReuseInputs {
                capabilities: &caps,
                clock: &|| Some(300),
                scope_proven: true,
            },
            || false,
        )
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(reader.bytes, 3); // full cache did not admit item 3
    assert!(third.evidence().groups.is_empty());
    assert_eq!(third.evidence().states[&1], State::Distinct);
    assert_eq!(third.evidence().states[&3], State::Distinct);
    Ok(())
}

#[test]
fn missing_capability_failure_and_cancel_do_not_create_verified_groups() -> Result<(), String> {
    let cache = FingerprintCache::new(NonZeroUsize::MIN.saturating_add(3));
    let hash = hash()?;
    let mut reader = make_reader();
    let no_cap = |_: &FileIdentity| None;
    let first = cache
        .verify_with_reuse(
            &[target(1), target(2)],
            &mut reader,
            &hash,
            &du_duplicate_engine::fingerprint_cache::ReuseInputs {
                capabilities: &no_cap,
                clock: &|| Some(10),
                scope_proven: true,
            },
            || false,
        )
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(first.evidence().full_reads.observed, 6);
    reader.bytes = 0;
    reader.fail = Some(1);
    let failed = cache
        .verify_with_reuse(
            &[target(1), target(2)],
            &mut reader,
            &hash,
            &du_duplicate_engine::fingerprint_cache::ReuseInputs {
                capabilities: &caps,
                clock: &|| Some(20),
                scope_proven: true,
            },
            || false,
        )
        .map_err(|e| format!("{e:?}"))?;
    assert!(failed.evidence().groups.is_empty());
    assert_eq!(failed.evidence().states[&1], State::ReadFailed);
    assert!(!failed.evidence().full_fingerprints.contains_key(&1));
    reader.fail = None;
    let cancel = Arc::new(AtomicBool::new(false));
    let mut cancelled_reader = make_reader();
    cancelled_reader.cancel = Some(Arc::clone(&cancel));
    let cancelled = cache
        .verify_with_reuse(
            &[target(1), target(2)],
            &mut cancelled_reader,
            &hash,
            &du_duplicate_engine::fingerprint_cache::ReuseInputs {
                capabilities: &caps,
                clock: &|| Some(30),
                scope_proven: true,
            },
            || cancel.load(Ordering::SeqCst),
        )
        .map_err(|e| format!("{e:?}"))?;
    assert!(cancelled.evidence().cancelled);
    assert!(cancelled.evidence().groups.is_empty());
    assert!(!cancelled.evidence().full_fingerprints.contains_key(&1));
    let mut retry = make_reader();
    let outcome = cache
        .verify_with_reuse(
            &[target(1), target(2)],
            &mut retry,
            &hash,
            &du_duplicate_engine::fingerprint_cache::ReuseInputs {
                capabilities: &caps,
                clock: &|| Some(40),
                scope_proven: true,
            },
            || false,
        )
        .map_err(|e| format!("{e:?}"))?;
    assert!(!outcome.evidence().full_fingerprints[&1].reused);
    Ok(())
}

#[test]
fn incomplete_scope_does_not_promote_nonmatching_peers_to_distinct() -> Result<(), String> {
    let cache = FingerprintCache::new(NonZeroUsize::MIN.saturating_add(2));
    let hash = hash()?;
    let mut reader = make_reader();
    let inputs = du_duplicate_engine::fingerprint_cache::ReuseInputs {
        capabilities: &caps,
        clock: &|| Some(50),
        scope_proven: false,
    };
    let outcome = cache
        .verify_with_reuse(&[target(1), target(3)], &mut reader, &hash, &inputs, || {
            false
        })
        .map_err(|e| format!("{e:?}"))?;
    assert!(outcome.evidence().groups.is_empty());
    assert_ne!(outcome.evidence().states[&1], State::Distinct);
    assert_ne!(outcome.evidence().states[&3], State::Distinct);
    Ok(())
}

#[test]
fn injected_reuse_reaches_job_body_and_published_result() -> Result<(), String> {
    use du_core_contracts::{
        Id, ItemRef,
        job::{Checkpoint, JobContext, JobCounts, JobFlow, JobFlowError},
        result::ResultCompleteness,
    };
    use du_duplicate_engine::{
        job::{self, Input, JobReuse},
        result_store::{ResultStore, ScopeEvidence},
    };
    use du_job_runtime::DuplicateTerminal;
    struct Flow;
    impl JobFlow for Flow {
        fn report_item(&self, _: &[u8]) -> Result<(), JobFlowError> {
            Ok(())
        }
        fn finish(self: Box<Self>, _: JobCounts) {}
    }
    struct Context;
    impl JobContext for Context {
        fn checkpoint(&self) -> Checkpoint {
            Checkpoint::Continue
        }
        fn try_start_flow(&self) -> Result<Box<dyn JobFlow>, JobFlowError> {
            Ok(Box::new(Flow))
        }
    }
    let source = Id::from_v7_bytes([1; 16]);
    let store = Arc::new(ResultStore::new(
        NonZeroUsize::MIN.saturating_add(4),
        NonZeroUsize::MIN.saturating_add(10),
    ));
    store
        .register_source(source.clone())
        .map_err(|e| format!("{e:?}"))?;
    let cache = Arc::new(FingerprintCache::new(NonZeroUsize::MIN.saturating_add(4)));
    for (id_byte, expected_bytes, expected_reused) in [(2, 6, false), (3, 0, true)] {
        let scope = ScopeEvidence {
            roots: vec![ItemRef {
                result_version: source.clone(),
                item_no: 0,
            }],
            exclusions: vec![],
            minimum_bytes: 0,
            outside: 0,
            excluded: 0,
            below_minimum: 0,
            unavailable: 0,
            source_completeness: ResultCompleteness::Complete,
            observed_at: None,
        };
        let result_id = Id::from_v7_bytes([id_byte; 16]);
        let prepared = job::prepare_with_reuse(
            Input {
                source: source.clone(),
                result: result_id.clone(),
                scope,
                targets: vec![target(1), target(2)],
                reader: make_reader(),
            },
            hash()?,
            Arc::clone(&store),
            JobReuse {
                cache: Arc::clone(&cache),
                capabilities: Arc::new(caps),
                clock: Arc::new(|| Some(100)),
            },
        )
        .map_err(|e| format!("{e:?}"))?;
        let result = prepared.body.run(Arc::new(Context));
        assert!(result.completed_scope);
        (prepared.commit)(DuplicateTerminal::Completed)?;
        let pinned = store.pin(Some(&result_id)).map_err(|e| format!("{e:?}"))?;
        let evidence = pinned.outcome().evidence();
        assert_eq!(evidence.full_reads.observed, expected_bytes);
        assert_eq!(evidence.full_fingerprints[&1].reused, expected_reused);
    }
    Ok(())
}

#[test]
fn previously_cached_files_are_read_when_capability_or_stamp_changes() -> Result<(), String> {
    use du_duplicate_engine::fingerprint_cache::ReuseInputs;
    let cache = FingerprintCache::new(NonZeroUsize::MIN.saturating_add(4));
    let hash = hash()?;
    let mut reader = make_reader();
    let inputs = ReuseInputs {
        capabilities: &caps,
        clock: &|| Some(100),
        scope_proven: true,
    };
    cache
        .verify_with_reuse(&[target(1), target(2)], &mut reader, &hash, &inputs, || {
            false
        })
        .map_err(|e| format!("{e:?}"))?;
    reader.bytes = 0;
    let unknown = ReuseInputs {
        capabilities: &|_| None,
        clock: &|| Some(110),
        scope_proven: true,
    };
    let result = cache
        .verify_with_reuse(
            &[target(1), target(2)],
            &mut reader,
            &hash,
            &unknown,
            || false,
        )
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(reader.bytes, 6);
    assert!(!result.evidence().full_fingerprints[&1].reused);
    for time in [None, Some(99)] {
        reader.bytes = 0;
        let clock = || time;
        let uncertain = ReuseInputs {
            capabilities: &caps,
            clock: &clock,
            scope_proven: true,
        };
        let result = cache
            .verify_with_reuse(
                &[target(1), target(2)],
                &mut reader,
                &hash,
                &uncertain,
                || false,
            )
            .map_err(|e| format!("{e:?}"))?;
        assert_eq!(reader.bytes, 6);
        assert!(!result.evidence().full_fingerprints[&1].reused);
    }
    reader.bytes = 0;
    reader.stamps.insert(1, 2);
    let mut changed = target(1);
    changed
        .snapshot
        .as_mut()
        .ok_or("missing snapshot")?
        .change_stamp = Some(2);
    let result = cache
        .verify_with_reuse(&[changed, target(2)], &mut reader, &hash, &inputs, || false)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(reader.bytes, 3);
    assert!(!result.evidence().full_fingerprints[&1].reused);
    assert!(result.evidence().full_fingerprints[&2].reused);
    assert_eq!(result.evidence().groups.len(), 1);
    Ok(())
}
