//! Registry admission and caller-supplied strategy comparison for F09.
use du_duplicate_engine::{
    RegisteredHash,
    candidates::{EntryKind, FileIdentity, Placeholder, Size, Target},
    verification::{
        self, Digest, HashPurpose, HashStrategy, OpenFile, ReadError, ReadPort, Snapshot,
        Strategies, VerificationTarget,
    },
};

struct ClaimedBlake3;
struct ConstantDigest;

impl Digest for ConstantDigest {
    fn update(&mut self, _bytes: &[u8]) {}
    fn finish(self: Box<Self>) -> Vec<u8> {
        vec![0; 32]
    }
}

impl HashStrategy for ClaimedBlake3 {
    fn id(&self) -> &str {
        "blake3"
    }
    fn revision(&self) -> u32 {
        1
    }
    fn collision_resistant(&self) -> bool {
        true
    }
    fn start(&self, _purpose: HashPurpose, _size: u64) -> Box<dyn Digest> {
        Box::new(ConstantDigest)
    }
}

struct FakeFile {
    snapshot: Snapshot,
    data: u8,
}

impl OpenFile for FakeFile {
    fn snapshot(&mut self) -> Result<Snapshot, ReadError> {
        Ok(self.snapshot.clone())
    }
    fn read_at(&mut self, offset: u64, buffer: &mut [u8]) -> Result<usize, ReadError> {
        if offset == 0 && !buffer.is_empty() {
            buffer[0] = self.data;
            Ok(1)
        } else {
            Ok(0)
        }
    }
}

struct FakeReader;

impl ReadPort for FakeReader {
    fn open_bound<'a>(
        &'a mut self,
        item_no: u32,
        expected: &FileIdentity,
    ) -> Result<Box<dyn OpenFile + 'a>, ReadError> {
        Ok(Box::new(FakeFile {
            snapshot: Snapshot {
                identity: expected.clone(),
                size: 1,
                modified: 1,
                change_stamp: None,
            },
            data: u8::try_from(item_no).map_err(|_| ReadError::Failed)?,
        }))
    }
}

fn target(item_no: u8) -> VerificationTarget {
    let identity = FileIdentity {
        volume: vec![1],
        file: vec![item_no],
    };
    VerificationTarget {
        target: Target {
            item_no: u32::from(item_no),
            kind: EntryKind::File,
            size: Size::Exact(1),
            identity: Some(identity.clone()),
            placeholder: Placeholder::Local,
        },
        snapshot: Some(Snapshot {
            identity,
            size: 1,
            modified: 1,
            change_stamp: None,
        }),
    }
}

#[test]
fn caller_supplied_blake3_label_does_not_prove_strategy_origin()
-> Result<(), verification::VerifyError> {
    let strategy = ClaimedBlake3;
    let outcome = verification::verify(
        &[target(1), target(2)],
        &mut FakeReader,
        Strategies {
            sample: &strategy,
            full: &strategy,
        },
        || false,
    )?;
    // The fake files contain different bytes; the self-reported strategy still
    // produces a group. This proves the label alone cannot seal an F09 result.
    assert_eq!(outcome.algorithm_id, "blake3");
    assert_eq!(outcome.algorithm_revision, 1);
    assert_eq!(outcome.groups.len(), 1);
    let registered = RegisteredHash::resolve(strategy.id(), Some(strategy.revision()))?;
    let real = registered.verify(&[target(1), target(2)], &mut FakeReader, || false)?;
    assert!(real.evidence().groups.is_empty());
    assert_eq!(real.evidence().algorithm_id, outcome.algorithm_id);
    Ok(())
}

#[test]
fn registry_pins_the_default_and_rejects_unsupported_selections()
-> Result<(), verification::VerifyError> {
    fn worker_safe<T: Send + Sync>() {}
    worker_safe::<RegisteredHash>();
    let strategy = RegisteredHash::resolve("blake3", None)?;
    let result = strategy.verify(&[target(1), target(2)], &mut FakeReader, || false)?;
    assert!(result.evidence().groups.is_empty());
    assert_eq!(result.evidence().algorithm_revision, 1);
    assert_eq!(result.evidence().sample_algorithm_revision, 1);
    for (id, revision) in [
        ("blake3", Some(0)),
        ("blake3", Some(2)),
        ("hash.sha256", None),
        ("hash.sample-xxh3", None),
    ] {
        assert!(matches!(
            RegisteredHash::resolve(id, revision),
            Err(verification::VerifyError::UnsupportedHash)
        ));
    }
    Ok(())
}
