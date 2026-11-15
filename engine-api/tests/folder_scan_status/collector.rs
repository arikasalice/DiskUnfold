//! A real collection from scan-engine, with exclusion rules and a cancellation, end to end.
use super::support::{Check, absent, err, id, item, present, unknown};
use du_analysis_index::IngestSink;
use du_core_contracts::{
    Id,
    browse_page_ipc::FolderFlags,
    job::Checkpoint,
    ports::{PortError, PortErrorKind},
    result::{ResultCompleteness, ResultVersionRead},
    scan_observation::{
        AnchoredDirectory, DirectoryObservations, HandleIdentity, MetadataObservation,
        ObservedEntry, ObservedKind,
    },
};
use du_scan_engine::scan::{ResultIds, collect};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
};

const ROOT: u64 = 1;
const KEEP: u64 = 11;

/// An in-memory volume: each directory lists its entries in one batch.
struct Tree {
    listings: HashMap<u64, Vec<ObservedEntry>>,
    refused: u64,
    /// Opening this directory requests cancellation; the open itself succeeds.
    cancel_on_open: Option<u64>,
    cancel: AtomicBool,
}

struct Dir {
    tree: Arc<Tree>,
    inode: u64,
    listed: bool,
}

impl DirectoryObservations for Dir {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        if std::mem::replace(&mut self.listed, true) {
            return Ok(Vec::new());
        }
        Ok(self
            .tree
            .listings
            .get(&self.inode)
            .cloned()
            .unwrap_or_default())
    }
}

impl AnchoredDirectory for Dir {
    fn open_child(&self, entry: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        let inode = entry.metadata.as_ref().map_err(Clone::clone)?.inode;
        if self.tree.cancel_on_open == Some(inode) {
            self.tree.cancel.store(true, Ordering::SeqCst);
        }
        if inode == self.tree.refused {
            return Err(PortError {
                kind: PortErrorKind::PermissionDenied,
                reason_key: "os.denied".into(),
                os_code: None,
            });
        }
        Ok(Box::new(Self {
            tree: Arc::clone(&self.tree),
            inode,
            listed: false,
        }))
    }

    fn observed_identity(&self) -> Option<HandleIdentity> {
        Some(HandleIdentity {
            device: 1,
            inode: self.inode,
            ancestors: Vec::new(),
        })
    }
}

fn entry(name: &str, kind: ObservedKind, device: u64, inode: u64) -> ObservedEntry {
    ObservedEntry {
        name: name.as_bytes().to_vec(),
        enumerated_inode: inode,
        enumerated_kind: kind.clone(),
        metadata: Ok(MetadataObservation {
            kind,
            device,
            inode,
            size: Ok(1),
            modified_at: None,
            native_modified_at: None,
            allocated: None,
            link_count: None,
            space_identity: None,
        }),
    }
}

fn dir(name: &str, device: u64, inode: u64) -> ObservedEntry {
    entry(name, ObservedKind::Directory, device, inode)
}

fn file(name: &str, inode: u64) -> ObservedEntry {
    entry(name, ObservedKind::File, 1, inode)
}

/// `denied` refuses its open, `done` is read before `keep`, `loop` is the root again,
/// `mnt` is another volume and every name starting with `x` is excluded.
fn tree(cancel_on_open: Option<u64>) -> Arc<Tree> {
    let listings = HashMap::from([
        (
            ROOT,
            vec![
                dir("denied", 1, 10),
                dir("done", 1, 17),
                dir("keep", 1, KEEP),
                dir("loop", 1, ROOT),
                dir("mnt", 2, 12),
                dir("x-dir", 1, 13),
                file("f", 14),
            ],
        ),
        (17, vec![file("b", 18)]),
        (KEEP, vec![file("a", 15), file("x-skip", 16)]),
    ]);
    Arc::new(Tree {
        listings,
        refused: 10,
        cancel_on_open,
        cancel: AtomicBool::new(false),
    })
}

struct Ids(AtomicU8);

impl ResultIds for Ids {
    fn next_id(&self) -> Id {
        id(self.0.fetch_add(1, Ordering::SeqCst))
    }
}

/// Collects the tree with the real sequential collector and pins the sealed result.
fn scan(tree: &Arc<Tree>) -> Result<(IngestSink, Arc<du_analysis_index::ResultVersion>), String> {
    let sink = IngestSink::default();
    let root = Box::new(Dir {
        tree: Arc::clone(tree),
        inode: ROOT,
        listed: false,
    });
    let cancel = || match tree.cancel.load(Ordering::SeqCst) {
        true => Checkpoint::Cancel,
        false => Checkpoint::Continue,
    };
    let include = |path: &[Vec<u8>]| path.last().is_none_or(|name| !name.starts_with(b"x"));
    let outcome = collect(&sink, id(1), root, &Ids(AtomicU8::new(10)), cancel, include);
    assert_eq!(outcome.failure_reason_key, None);
    let version = sink.pin().map_err(err)?.ok_or("no result")?;
    Ok((sink, version))
}

fn folder(unenumerated: Value, boundary: Value) -> Value {
    json!({"unenumerated": unenumerated, "excluded": present(false), "boundary": boundary})
}

#[test]
fn a_finished_collection_reports_each_folder_and_the_excluded_entries() -> Check {
    let (sink, version) = scan(&tree(None))?;
    assert_eq!(version.completeness(), ResultCompleteness::Missing);
    let browser = super::support::session(sink);
    let top = super::support::browse(&browser, None, None, 2)?;
    top.check()?;
    let flags = |name| -> Result<Value, String> {
        Ok(top.item_row(&item(&version, &[name])?)?["flags"].clone())
    };
    assert_eq!(flags("denied")?, folder(present(true), absent()));
    assert_eq!(flags("done")?, folder(present(false), absent()));
    assert_eq!(flags("keep")?, folder(present(false), absent()));
    assert_eq!(
        flags("loop")?,
        folder(present(false), present("already_counted"))
    );
    assert_eq!(
        flags("mnt")?,
        folder(present(false), present("other_volume"))
    );
    assert_eq!(
        flags("f")?,
        json!({"unenumerated": absent(), "excluded": present(false)})
    );
    assert_eq!(top.excluded(), Some(&json!({"state": "exact", "count": 1})));
    for (name, expected) in [
        ("keep", json!({"state": "exact", "count": 1})),
        ("done", json!({"state": "exact", "count": 0})),
        ("denied", json!({"state": "lower_bound", "count": 0})),
    ] {
        let below = super::support::browse(&browser, None, Some(item(&version, &[name])?), 2)?;
        below.check()?;
        assert_eq!(below.excluded(), Some(&expected), "{name}");
    }
    Ok(())
}

#[test]
fn a_cancelled_collection_marks_what_it_left_and_undecides_the_rest() -> Check {
    let (sink, version) = scan(&tree(Some(KEEP)))?;
    assert_eq!(version.completeness(), ResultCompleteness::Cancelled);
    let browser = super::support::session(sink);
    let top = super::support::browse(&browser, None, None, 2)?;
    top.check()?;
    let flags = |name| -> Result<Value, String> {
        Ok(top.item_row(&item(&version, &[name])?)?["flags"].clone())
    };
    let insufficient = unknown(FolderFlags::REASON_DATA_INSUFFICIENT);
    assert_eq!(flags("done")?, folder(insufficient.clone(), insufficient));
    assert_eq!(flags("keep")?, folder(present(true), absent()));
    assert_eq!(flags("denied")?, folder(present(true), absent()));
    assert_eq!(
        flags("mnt")?,
        folder(present(false), present("other_volume"))
    );
    // The root's listing stopped early, so its count is a lower bound.
    assert_eq!(
        top.excluded(),
        Some(&json!({"state": "lower_bound", "count": 1}))
    );
    let keep = super::support::browse(&browser, None, Some(item(&version, &["keep"])?), 2)?;
    assert_eq!(keep.excluded(), None);
    Ok(())
}
