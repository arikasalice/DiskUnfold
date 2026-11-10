use du_core_contracts::{
    Id, ItemRef,
    duplicate_content::{
        BoundContent, ContentIdentity, ContentObservation, DuplicateContentPort,
        PlaceholderEvidence,
    },
    ports::{PortError, PortErrorKind},
};
use du_duplicate_engine::{
    RegisteredHash,
    candidates::{EntryKind, FileIdentity, Placeholder, Size, Target},
    content_port::{Adapter, SelectedContent},
    verification::{ReadPort, Snapshot, State, VerificationTarget},
};
use std::collections::BTreeMap;

fn identity(n: u8) -> ContentIdentity {
    ContentIdentity {
        volume: vec![1],
        file: vec![n],
        enumerated_inode: Some(u64::from(n)),
    }
}
fn target(n: u8) -> VerificationTarget {
    let id = FileIdentity {
        volume: vec![1],
        file: vec![n],
    };
    VerificationTarget {
        target: Target {
            item_no: u32::from(n),
            kind: EntryKind::File,
            size: Size::Exact(3),
            identity: Some(id.clone()),
            placeholder: Placeholder::Local,
        },
        snapshot: Some(Snapshot {
            identity: id,
            size: 3,
            modified: 7,
            change_stamp: Some(1),
        }),
    }
}
fn version(n: u8) -> Id {
    Id::from_v7_bytes([n; 16])
}
fn selection(n: u8, placeholder: PlaceholderEvidence) -> SelectedContent {
    SelectedContent {
        item: ItemRef {
            result_version: version(1),
            item_no: u32::from(n),
        },
        identity: identity(n),
        placeholder,
    }
}
fn error(kind: PortErrorKind) -> PortError {
    PortError {
        kind,
        reason_key: "test.failure".into(),
        os_code: None,
    }
}

struct Fake {
    opens: usize,
    reads: usize,
    facts: BTreeMap<u32, ContentIdentity>,
    changed: bool,
    reject: bool,
}
struct Handle<'a> {
    facts: ContentIdentity,
    reads: &'a mut usize,
    changed: bool,
    observations: usize,
}
impl BoundContent for Handle<'_> {
    fn observe(&mut self) -> Result<ContentObservation, PortError> {
        self.observations += 1;
        let mut identity = self.facts.clone();
        if self.changed && self.observations > 1 {
            identity.enumerated_inode = Some(999);
        }
        Ok(ContentObservation {
            identity,
            size: 3,
            modified: 7,
            change_stamp: Some(1),
        })
    }
    fn read_at(&mut self, offset: u64, buffer: &mut [u8]) -> Result<usize, PortError> {
        *self.reads += 1;
        let start = usize::try_from(offset).map_err(|_| error(PortErrorKind::Io))?;
        let bytes = b"abc".get(start..).unwrap_or_default();
        let count = bytes.len().min(buffer.len());
        buffer[..count].copy_from_slice(&bytes[..count]);
        Ok(count)
    }
}
impl DuplicateContentPort for Fake {
    fn open_bound<'a>(
        &'a mut self,
        item: &ItemRef,
        expected: &ContentIdentity,
        _: PlaceholderEvidence,
    ) -> Result<Box<dyn BoundContent + 'a>, PortError> {
        self.opens += 1;
        if item.result_version != version(1) {
            return Err(error(PortErrorKind::IdentityChanged));
        }
        if self.reject {
            return Err(error(PortErrorKind::Unsupported));
        }
        let facts = self
            .facts
            .get(&item.item_no)
            .ok_or_else(|| error(PortErrorKind::NotFound))?;
        if facts != expected {
            return Err(error(PortErrorKind::IdentityChanged));
        }
        Ok(Box::new(Handle {
            facts: facts.clone(),
            reads: &mut self.reads,
            changed: self.changed,
            observations: 0,
        }))
    }
}
fn fake() -> Fake {
    Fake {
        opens: 0,
        reads: 0,
        facts: [(1, identity(1)), (2, identity(2))].into(),
        changed: false,
        reject: false,
    }
}

#[test]
fn registered_verifier_consumes_the_shared_port_and_only_verifies_stable_files()
-> Result<(), Box<dyn std::error::Error>> {
    let mut port = fake();
    let mut adapter = Adapter::new(
        &mut port,
        &version(1),
        vec![
            selection(1, PlaceholderEvidence::Local),
            selection(2, PlaceholderEvidence::Local),
        ],
    )
    .map_err(|e| format!("{e:?}"))?;
    let hash = RegisteredHash::resolve("blake3", Some(1)).map_err(|e| format!("{e:?}"))?;
    let result = hash
        .verify(&[target(1), target(2)], &mut adapter, || false)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(result.evidence().groups.len(), 1);
    assert_eq!(result.evidence().states[&1], State::Verified);
    drop(adapter);
    assert_eq!(port.opens, 2);
    assert!(port.reads > 0);
    Ok(())
}

#[test]
fn unverified_placeholder_and_unsupported_port_never_read() -> Result<(), Box<dyn std::error::Error>>
{
    for (placeholder, reject) in [
        (PlaceholderEvidence::Unknown, false),
        (PlaceholderEvidence::DownloadRequired, false),
        (PlaceholderEvidence::Local, true),
    ] {
        let mut port = fake();
        port.reject = reject;
        let mut adapter = Adapter::new(
            &mut port,
            &version(1),
            vec![selection(1, placeholder), selection(2, placeholder)],
        )
        .map_err(|e| format!("{e:?}"))?;
        let hash = RegisteredHash::resolve("blake3", Some(1)).map_err(|e| format!("{e:?}"))?;
        let result = hash
            .verify(&[target(1), target(2)], &mut adapter, || false)
            .map_err(|e| format!("{e:?}"))?;
        assert!(result.evidence().groups.is_empty());
        drop(adapter);
        assert_eq!(port.reads, 0);
        if !reject {
            assert_eq!(port.opens, 0);
        }
    }
    Ok(())
}

#[test]
fn changed_open_handle_identity_cannot_create_a_group() -> Result<(), Box<dyn std::error::Error>> {
    let mut port = fake();
    port.changed = true;
    let mut adapter = Adapter::new(
        &mut port,
        &version(1),
        vec![
            selection(1, PlaceholderEvidence::Local),
            selection(2, PlaceholderEvidence::Local),
        ],
    )
    .map_err(|e| format!("{e:?}"))?;
    let hash = RegisteredHash::resolve("blake3", Some(1)).map_err(|e| format!("{e:?}"))?;
    let result = hash
        .verify(&[target(1), target(2)], &mut adapter, || false)
        .map_err(|e| format!("{e:?}"))?;
    assert!(result.evidence().groups.is_empty());
    assert_eq!(result.evidence().states[&1], State::Changed);
    Ok(())
}

#[test]
fn cancellation_before_verification_opens_no_content() -> Result<(), Box<dyn std::error::Error>> {
    let mut port = fake();
    let mut adapter = Adapter::new(
        &mut port,
        &version(1),
        vec![
            selection(1, PlaceholderEvidence::Local),
            selection(2, PlaceholderEvidence::Local),
        ],
    )
    .map_err(|e| format!("{e:?}"))?;
    let hash = RegisteredHash::resolve("blake3", Some(1)).map_err(|e| format!("{e:?}"))?;
    let result = hash
        .verify(&[target(1), target(2)], &mut adapter, || true)
        .map_err(|e| format!("{e:?}"))?;
    assert!(result.evidence().cancelled);
    assert!(result.evidence().groups.is_empty());
    drop(adapter);
    assert_eq!((port.opens, port.reads), (0, 0));
    Ok(())
}

#[test]
fn rejects_cross_version_duplicate_and_missing_identity_before_open() {
    let mut port = fake();
    let mut other = selection(1, PlaceholderEvidence::Local);
    other.item.result_version = version(2);
    assert!(Adapter::new(&mut port, &version(1), vec![other]).is_err());
    assert!(
        Adapter::new(
            &mut port,
            &version(1),
            vec![
                selection(1, PlaceholderEvidence::Local),
                selection(1, PlaceholderEvidence::Local)
            ]
        )
        .is_err()
    );
    for invalid in 0..4 {
        let mut value = selection(1, PlaceholderEvidence::Local);
        match invalid {
            0 => value.identity.volume.clear(),
            1 => value.identity.file.clear(),
            2 => value.identity.enumerated_inode = Some(0),
            _ => value.item.item_no = 0,
        }
        assert!(Adapter::new(&mut port, &version(1), vec![value]).is_err());
    }
    assert_eq!((port.opens, port.reads), (0, 0));
}

#[test]
fn unknown_enumeration_is_preserved_without_fabricating_a_value()
-> Result<(), Box<dyn std::error::Error>> {
    let mut port = fake();
    let mut selected = selection(1, PlaceholderEvidence::Local);
    selected.identity.enumerated_inode = None;
    port.facts.insert(1, selected.identity.clone());
    let mut adapter =
        Adapter::new(&mut port, &version(1), vec![selected]).map_err(|e| format!("{e:?}"))?;
    let expected = FileIdentity {
        volume: vec![1],
        file: vec![1],
    };
    let mut handle = adapter
        .open_bound(1, &expected)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        handle.snapshot().map_err(|e| format!("{e:?}"))?.identity,
        expected
    );
    Ok(())
}
