use du_duplicate_engine::candidates::{EntryKind, FileIdentity, Placeholder, Size, Target};
use du_duplicate_engine::verification::{
    self, Digest, HashPurpose, HashStrategy, OpenFile, ReadError, ReadPort, Snapshot, State,
    Strategies, VerificationTarget, VerifyError,
};
use std::cell::Cell;
use std::collections::BTreeMap;
use std::rc::Rc;

fn identity(n: u8) -> FileIdentity {
    FileIdentity {
        volume: vec![1],
        file: vec![n],
    }
}
fn item(n: u8, size: usize) -> VerificationTarget {
    let identity = identity(n);
    VerificationTarget {
        target: Target {
            item_no: u32::from(n),
            kind: EntryKind::File,
            size: Size::Exact(size as u64),
            identity: Some(identity.clone()),
            placeholder: Placeholder::Local,
        },
        snapshot: Some(Snapshot {
            identity,
            size: size as u64,
            modified: 7,
            change_stamp: Some(1),
        }),
    }
}
#[derive(Clone)]
struct Entry {
    bytes: Vec<u8>,
    before: Snapshot,
    after: Snapshot,
    fail_read: bool,
    fail_at: Option<u64>,
    invalid_at: Option<u64>,
}
struct FakePort {
    entries: BTreeMap<u32, Entry>,
    opens: Vec<u32>,
    reads: Vec<(u32, u64, usize)>,
    cancel_on_read: Option<Rc<Cell<bool>>>,
    cancel_on_open_failure: Option<Rc<Cell<bool>>>,
    max_read: usize,
}
impl FakePort {
    fn new(items: &[(VerificationTarget, Vec<u8>)]) -> Self {
        let entries = items
            .iter()
            .filter_map(|(target, bytes)| {
                target.snapshot.clone().map(|state| {
                    (
                        target.target.item_no,
                        Entry {
                            bytes: bytes.clone(),
                            before: state.clone(),
                            after: state,
                            fail_read: false,
                            fail_at: None,
                            invalid_at: None,
                        },
                    )
                })
            })
            .collect();
        Self {
            entries,
            opens: vec![],
            reads: vec![],
            cancel_on_read: None,
            cancel_on_open_failure: None,
            max_read: usize::MAX,
        }
    }
}
struct FakeFile<'a> {
    item_no: u32,
    entry: &'a Entry,
    snapshots: usize,
    reads: &'a mut Vec<(u32, u64, usize)>,
    cancel_on_read: Option<Rc<Cell<bool>>>,
    max_read: usize,
}
impl OpenFile for FakeFile<'_> {
    fn snapshot(&mut self) -> Result<Snapshot, ReadError> {
        self.snapshots += 1;
        Ok(if self.snapshots == 1 {
            self.entry.before.clone()
        } else {
            self.entry.after.clone()
        })
    }
    fn read_at(&mut self, offset: u64, buffer: &mut [u8]) -> Result<usize, ReadError> {
        self.reads.push((self.item_no, offset, buffer.len()));
        if let Some(cancel) = &self.cancel_on_read {
            cancel.set(true);
        }
        if self.entry.fail_read || self.entry.fail_at.is_some_and(|at| offset >= at) {
            return Err(ReadError::Failed);
        }
        if self.entry.invalid_at == Some(offset) {
            return Ok(buffer.len() + 1);
        }
        let start = usize::try_from(offset).map_err(|_| ReadError::Failed)?;
        let remaining = self.entry.bytes.get(start..).unwrap_or_default();
        let count = remaining.len().min(buffer.len()).min(self.max_read);
        buffer[..count].copy_from_slice(&remaining[..count]);
        Ok(count)
    }
}
impl ReadPort for FakePort {
    fn open_bound<'a>(
        &'a mut self,
        item_no: u32,
        expected: &FileIdentity,
    ) -> Result<Box<dyn OpenFile + 'a>, ReadError> {
        self.opens.push(item_no);
        if let Some(flag) = &self.cancel_on_open_failure {
            flag.set(true);
            return Err(ReadError::Failed);
        }
        let entry = self.entries.get(&item_no).ok_or(ReadError::Failed)?;
        if &entry.before.identity != expected {
            return Err(ReadError::IdentityChanged);
        }
        Ok(Box::new(FakeFile {
            item_no,
            entry,
            snapshots: 0,
            reads: &mut self.reads,
            cancel_on_read: self.cancel_on_read.clone(),
            max_read: self.max_read,
        }))
    }
}

struct FakeHasher {
    strong: bool,
    id: &'static str,
}
struct FakeDigest {
    bytes: Vec<u8>,
}
impl Digest for FakeDigest {
    fn update(&mut self, bytes: &[u8]) {
        self.bytes.extend_from_slice(bytes);
    }
    fn finish(self: Box<Self>) -> Vec<u8> {
        self.bytes
    }
}
impl HashStrategy for FakeHasher {
    fn id(&self) -> &str {
        self.id
    }
    fn revision(&self) -> u32 {
        1
    }
    fn collision_resistant(&self) -> bool {
        self.strong
    }
    fn start(&self, purpose: HashPurpose, _approved_size: u64) -> Box<dyn Digest> {
        Box::new(FakeDigest {
            bytes: vec![match purpose {
                HashPurpose::Sample => 1,
                HashPurpose::Full => 2,
            }],
        })
    }
}
fn strategies<'a>(sample: &'a FakeHasher, full: &'a FakeHasher) -> Strategies<'a> {
    Strategies { sample, full }
}
fn hashers() -> (FakeHasher, FakeHasher) {
    (
        FakeHasher {
            strong: true,
            id: "sample",
        },
        FakeHasher {
            strong: true,
            id: "full",
        },
    )
}

#[test]
fn blake3_v1_compares_full_bytes_after_matching_samples_and_short_reads() -> Result<(), VerifyError>
{
    const SIZE: usize = 128 * 1024 + 1;
    let [a, b, c] = [1, 2, 3].map(|number| item(number, SIZE));
    let same = vec![0x11; SIZE];
    let mut different = same.clone();
    different[SIZE / 2] ^= 0xff;
    let mut port = FakePort::new(&[
        (a.clone(), same.clone()),
        (b.clone(), same.clone()),
        (c.clone(), different),
    ]);
    port.max_read = 4093;
    let result = verification::verify(&[a, b, c], &mut port, Strategies::blake3_v1(), || false)?;
    assert_eq!(result.groups.len(), 1);
    assert_eq!(result.sample_reads.observed, 3 * 128 * 1024);
    assert_eq!(result.full_reads.observed, 3 * SIZE as u64);
    assert!(result.sample_reads.exact && result.full_reads.exact);
    assert_eq!(result.groups[0].files.len(), 2);
    assert_eq!(
        result.groups[0].digest,
        blake3::derive_key(
            "com.github.arikasalice.diskunfold.f09.blake3.v1.full",
            &same
        )
    );
    assert_eq!(result.states.get(&3), Some(&State::Distinct));
    Ok(())
}

#[test]
fn only_approved_and_fully_known_candidates_are_opened() -> Result<(), VerifyError> {
    let a = item(1, 3);
    let b = item(2, 3);
    let outside = item(3, 3);
    let mut unknown = item(4, 3);
    unknown.snapshot = None;
    let mut no_identity = item(5, 3);
    no_identity.target.identity = None;
    let mut cloud = item(6, 3);
    cloud.target.placeholder = Placeholder::DownloadRequired;
    let mut unknown_cloud = item(7, 3);
    unknown_cloud.target.placeholder = Placeholder::Unknown;
    let mut port = FakePort::new(&[
        (a.clone(), b"abc".to_vec()),
        (b.clone(), b"abc".to_vec()),
        (outside, b"abc".to_vec()),
        (unknown.clone(), b"abc".to_vec()),
        (no_identity.clone(), b"abc".to_vec()),
        (cloud.clone(), b"abc".to_vec()),
        (unknown_cloud.clone(), b"abc".to_vec()),
    ]);
    let (sample, full) = hashers();
    let result = verification::verify(
        &[a, b, unknown, no_identity, cloud, unknown_cloud],
        &mut port,
        strategies(&sample, &full),
        || false,
    )?;
    assert_eq!(port.opens, vec![1, 2]);
    assert_eq!(result.groups.len(), 1);
    for n in [4, 5, 7] {
        assert_eq!(result.states.get(&n), Some(&State::Unchecked));
    }
    assert_eq!(result.states.get(&6), Some(&State::PlaceholderDeferred));
    Ok(())
}

#[test]
fn matching_samples_are_not_full_verification() -> Result<(), VerifyError> {
    let size = 128 * 1024 + 1;
    let a = item(1, size);
    let b = item(2, size);
    let one = vec![0_u8; size];
    let mut two = one.clone();
    two[64 * 1024] = 9;
    let mut port = FakePort::new(&[(a.clone(), one), (b.clone(), two)]);
    let (sample, full) = hashers();
    let result = verification::verify(&[a, b], &mut port, strategies(&sample, &full), || false)?;
    assert!(result.groups.is_empty());
    assert_eq!(port.opens, vec![1, 2, 1, 2]);
    assert_eq!(result.states.get(&1), Some(&State::Distinct));
    assert_eq!(result.states.get(&2), Some(&State::Distinct));
    Ok(())
}

#[test]
fn unequal_samples_do_not_trigger_full_reads() -> Result<(), VerifyError> {
    let size = 128 * 1024 + 1;
    let a = item(1, size);
    let b = item(2, size);
    let one = vec![0_u8; size];
    let mut two = one.clone();
    two[0] = 1;
    let mut port = FakePort::new(&[(a.clone(), one), (b.clone(), two)]);
    let (sample, full) = hashers();
    let result = verification::verify(&[a, b], &mut port, strategies(&sample, &full), || false)?;
    assert!(result.groups.is_empty());
    assert_eq!(port.opens, vec![1, 2]);
    assert_eq!(result.states.get(&1), Some(&State::Distinct));
    assert_eq!(result.states.get(&2), Some(&State::Distinct));
    Ok(())
}

#[test]
fn different_hashes_do_not_prove_distinction_against_an_unchecked_peer() -> Result<(), VerifyError>
{
    let a = item(1, 3);
    let b = item(2, 3);
    let mut unknown = item(3, 3);
    unknown.snapshot = None;
    let mut port = FakePort::new(&[(a.clone(), b"abc".to_vec()), (b.clone(), b"def".to_vec())]);
    let (sample, full) = hashers();
    let result = verification::verify(
        &[a, b, unknown],
        &mut port,
        strategies(&sample, &full),
        || false,
    )?;
    assert_eq!(port.opens, vec![1, 2]);
    assert!(result.groups.is_empty());
    assert_eq!(result.states.get(&1), Some(&State::Candidate));
    assert_eq!(result.states.get(&2), Some(&State::Candidate));
    assert_eq!(result.states.get(&3), Some(&State::Unchecked));
    Ok(())
}

#[test]
fn zero_size_requires_successful_open_and_full_eof_check() -> Result<(), VerifyError> {
    let a = item(1, 0);
    let b = item(2, 0);
    let mut port = FakePort::new(&[(a.clone(), vec![]), (b.clone(), vec![])]);
    let (sample, full) = hashers();
    let result = verification::verify(
        &[a.clone(), b.clone()],
        &mut port,
        strategies(&sample, &full),
        || false,
    )?;
    assert_eq!(result.groups.len(), 1);
    assert_eq!(port.opens, vec![1, 2]);
    assert_eq!(port.reads, vec![(1, 0, 1), (2, 0, 1)]);
    assert_eq!(result.full_reads.observed, 0);
    assert!(result.full_reads.exact);
    let mut port = FakePort::new(&[(a.clone(), vec![]), (b.clone(), vec![])]);
    if let Some(entry) = port.entries.get_mut(&2) {
        entry.fail_read = true;
    }
    let result = verification::verify(&[a, b], &mut port, strategies(&sample, &full), || false)?;
    assert!(result.groups.is_empty());
    assert_eq!(result.states.get(&2), Some(&State::ReadFailed));
    assert_eq!(result.states.get(&1), Some(&State::Candidate));
    assert!(!result.full_reads.exact);
    Ok(())
}

#[test]
fn changed_during_read_and_untrusted_strategy_never_verify() -> Result<(), VerifyError> {
    let a = item(1, 2);
    let b = item(2, 2);
    let mut port = FakePort::new(&[(a.clone(), b"xx".to_vec()), (b.clone(), b"xx".to_vec())]);
    if let Some(entry) = port.entries.get_mut(&2) {
        entry.after.modified += 1;
    }
    let (sample, full) = hashers();
    let result = verification::verify(
        &[a.clone(), b.clone()],
        &mut port,
        strategies(&sample, &full),
        || false,
    )?;
    assert!(result.groups.is_empty());
    assert_eq!(result.states.get(&2), Some(&State::Changed));
    let mut port = FakePort::new(&[(a.clone(), b"xx".to_vec()), (b.clone(), b"xx".to_vec())]);
    let weak = FakeHasher {
        strong: false,
        id: "weak",
    };
    assert_eq!(
        verification::verify(&[a, b], &mut port, strategies(&sample, &weak), || false),
        Err(VerifyError::UnsupportedHash)
    );
    assert!(port.opens.is_empty());
    Ok(())
}

#[test]
fn cancellation_finishes_in_flight_file_without_starting_another() -> Result<(), VerifyError> {
    let a = item(1, 2);
    let b = item(2, 2);
    let flag = Rc::new(Cell::new(false));
    let mut port = FakePort::new(&[(a.clone(), b"xx".to_vec()), (b.clone(), b"xx".to_vec())]);
    port.cancel_on_read = Some(flag.clone());
    let (sample, full) = hashers();
    let result = verification::verify(&[a, b], &mut port, strategies(&sample, &full), || {
        flag.get()
    })?;
    assert!(result.cancelled);
    assert!(result.groups.is_empty());
    assert_eq!(port.opens, vec![1]);
    assert_eq!(result.full_reads.observed, 2);
    assert!(result.full_reads.exact);
    assert_eq!(result.states.get(&1), Some(&State::Candidate));
    Ok(())
}

#[test]
fn conflicting_alias_observations_are_deferred_before_open() -> Result<(), VerifyError> {
    let a = item(1, 2);
    let mut alias = item(2, 2);
    alias.target.identity = a.target.identity.clone();
    alias.snapshot = a.snapshot.clone();
    if let Some(snapshot) = &mut alias.snapshot {
        snapshot.modified += 1;
    }
    let b = item(3, 2);
    let mut port = FakePort::new(&[
        (a.clone(), b"xx".to_vec()),
        (alias.clone(), b"xx".to_vec()),
        (b.clone(), b"xx".to_vec()),
    ]);
    let (sample, full) = hashers();
    let result = verification::verify(
        &[a, alias, b],
        &mut port,
        strategies(&sample, &full),
        || false,
    )?;
    assert!(port.opens.is_empty());
    assert_eq!(result.states.get(&1), Some(&State::Unchecked));
    assert_eq!(result.states.get(&2), Some(&State::Unchecked));
    assert!(result.groups.is_empty());
    Ok(())
}

#[test]
fn identity_replacement_and_missing_change_stamp_are_reported() -> Result<(), VerifyError> {
    let mut a = item(1, 2);
    let mut b = item(2, 2);
    if let Some(snapshot) = &mut a.snapshot {
        snapshot.change_stamp = None;
    }
    if let Some(snapshot) = &mut b.snapshot {
        snapshot.change_stamp = None;
    }
    let mut port = FakePort::new(&[(a.clone(), b"xx".to_vec()), (b.clone(), b"xx".to_vec())]);
    let (sample, full) = hashers();
    let result = verification::verify(
        &[a.clone(), b.clone()],
        &mut port,
        strategies(&sample, &full),
        || false,
    )?;
    assert_eq!(result.groups.len(), 1);
    assert!(
        result.groups[0]
            .files
            .iter()
            .all(|file| !file.change_stamp_available)
    );
    let mut port = FakePort::new(&[(a.clone(), b"xx".to_vec()), (b.clone(), b"xx".to_vec())]);
    if let Some(entry) = port.entries.get_mut(&2) {
        entry.before.identity = identity(99);
    }
    let result = verification::verify(&[a, b], &mut port, strategies(&sample, &full), || false)?;
    assert_eq!(result.states.get(&2), Some(&State::Changed));
    assert!(result.groups.is_empty());
    assert!(!port.reads.iter().any(|(item, _, _)| *item == 2));
    Ok(())
}

#[test]
fn verified_groups_keep_aliases_without_reopening_and_order_by_first_reference()
-> Result<(), VerifyError> {
    let a = item(1, 2);
    let b = item(2, 2);
    let c = item(4, 2);
    let d = item(6, 2);
    let mut alias = a.clone();
    alias.target.item_no = 7;
    let mut port = FakePort::new(&[
        (a.clone(), b"zz".to_vec()),
        (b.clone(), b"aa".to_vec()),
        (c.clone(), b"aa".to_vec()),
        (d.clone(), b"zz".to_vec()),
        (alias.clone(), b"zz".to_vec()),
    ]);
    let (sample, full) = hashers();
    let result = verification::verify(
        &[d, alias, c, b, a],
        &mut port,
        strategies(&sample, &full),
        || false,
    )?;
    assert_eq!(port.opens, vec![1, 2, 4, 6]);
    assert_eq!(result.groups.len(), 2);
    assert_eq!(result.groups[0].files[0].item_nos, vec![1, 7]);
    assert_eq!(result.groups[0].files[1].item_nos, vec![6]);
    assert_eq!(result.groups[1].files[0].item_nos, vec![2]);
    assert_eq!(result.states.get(&7), Some(&State::Verified));
    assert_eq!(result.algorithm_id, "full");
    assert_eq!(result.sample_algorithm_id, "sample");
    Ok(())
}

mod measurement;
