//! An in-memory tree whose reads and opens are logged, with one call that
//! requests cancellation, for the per-folder collection records.
use super::*;
use du_analysis_index::folder_collection::{
    ExcludedChildren, FolderCollection, REASON_CANCELLED, Unenumerated,
};
use du_core_contracts::scan_observation::ObservedValue;
use du_core_contracts::scan_observation::{FilterDecision, FilteredBatch, HandleIdentity};
use std::sync::atomic::Ordering::SeqCst;

pub(super) const ROOT: u64 = 1;

/// What a directory does when it is opened and read.
#[derive(Clone)]
pub(super) enum Node {
    /// These batches in order, then the end of the listing.
    Batches(Vec<Vec<ObservedEntry>>),
    /// These batches in order, then a failed read.
    FailsAfter(Vec<Vec<ObservedEntry>>),
    /// The open is refused.
    Refused,
}

/// The port call that requests cancellation as it starts.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Trigger {
    Never,
    /// Read number `.1` (from zero) of the directory with inode `.0`.
    Read(u64, usize),
    /// The open of the directory with inode `.0`; the open still succeeds.
    Open(u64),
}

#[derive(Default)]
pub(super) struct Log {
    pub(super) reads: HashMap<u64, usize>,
    pub(super) ended: HashSet<u64>,
    pub(super) failed: HashSet<u64>,
    pub(super) opened: HashSet<u64>,
}

pub(super) struct Tree {
    nodes: HashMap<u64, Node>,
    trigger: Trigger,
    pub(super) cancel: Arc<AtomicBool>,
    pub(super) log: Mutex<Log>,
    /// The root reports no identity, so its boundary cannot be verified.
    pub(super) unverified: bool,
}

pub(super) struct TreeDir {
    tree: Arc<Tree>,
    inode: u64,
    reads: usize,
}

impl Tree {
    pub(super) fn new(nodes: HashMap<u64, Node>, trigger: Trigger) -> Arc<Self> {
        Arc::new(Self {
            nodes,
            trigger,
            cancel: Arc::new(AtomicBool::new(false)),
            log: Mutex::default(),
            unverified: false,
        })
    }

    pub(super) fn unverified(nodes: HashMap<u64, Node>, trigger: Trigger) -> Arc<Self> {
        let mut tree = Self::new(nodes, trigger);
        if let Some(tree) = Arc::get_mut(&mut tree) {
            tree.unverified = true;
        }
        tree
    }

    pub(super) fn root(self: &Arc<Self>) -> Box<dyn AnchoredDirectory> {
        Box::new(TreeDir {
            tree: Arc::clone(self),
            inode: ROOT,
            reads: 0,
        })
    }

    pub(super) fn with_log<R>(&self, read: impl FnOnce(&Log) -> R) -> Result<R, String> {
        self.log
            .lock()
            .map(|log| read(&log))
            .map_err(|_| "log".into())
    }

    fn note(&self, change: impl FnOnce(&mut Log)) {
        if let Ok(mut log) = self.log.lock() {
            change(&mut log);
        }
    }

    fn fire(&self, call: Trigger) {
        if call == self.trigger {
            self.cancel.store(true, SeqCst);
        }
    }
}

impl TreeDir {
    fn read(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        let index = self.reads;
        self.reads += 1;
        let inode = self.inode;
        self.tree.fire(Trigger::Read(inode, index));
        self.tree
            .note(|log| *log.reads.entry(inode).or_default() += 1);
        let (batches, fails) = match self.tree.nodes.get(&inode) {
            Some(Node::Batches(batches)) => (batches.as_slice(), false),
            Some(Node::FailsAfter(batches)) => (batches.as_slice(), true),
            _ => (&[][..], false),
        };
        if let Some(batch) = batches.get(index) {
            return Ok(batch.clone());
        }
        if fails {
            self.tree.note(|log| {
                log.failed.insert(inode);
            });
            return Err(PortError {
                kind: PortErrorKind::Io,
                reason_key: "os.query_failed".into(),
                os_code: None,
            });
        }
        self.tree.note(|log| {
            log.ended.insert(inode);
        });
        Ok(Vec::new())
    }
}

impl DirectoryObservations for TreeDir {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        self.read()
    }

    fn next_filtered(
        &mut self,
        include: &mut dyn FnMut(&[u8]) -> FilterDecision,
    ) -> Result<FilteredBatch, PortError> {
        let batch = self.read()?;
        if batch.is_empty() {
            return Ok(FilteredBatch::End);
        }
        let mut entries = Vec::new();
        for entry in batch {
            match include(&entry.name) {
                FilterDecision::Admit => entries.push(entry),
                FilterDecision::Exclude => {}
                FilterDecision::Undecidable => return Ok(FilteredBatch::Undecidable),
            }
        }
        Ok(if entries.is_empty() {
            FilteredBatch::Progress
        } else {
            FilteredBatch::Entries(entries)
        })
    }
}

impl AnchoredDirectory for TreeDir {
    fn open_child(&self, entry: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        let inode = entry.metadata.as_ref().map_err(Clone::clone)?.inode;
        self.tree.fire(Trigger::Open(inode));
        self.tree.note(|log| {
            log.opened.insert(inode);
        });
        if let Some(Node::Refused) = self.tree.nodes.get(&inode) {
            return Err(PortError {
                kind: PortErrorKind::PermissionDenied,
                reason_key: "os.denied".into(),
                os_code: None,
            });
        }
        Ok(Box::new(Self {
            tree: Arc::clone(&self.tree),
            inode,
            reads: 0,
        }))
    }

    fn observed_identity(&self) -> Option<HandleIdentity> {
        (self.inode != ROOT || !self.tree.unverified).then_some(HandleIdentity {
            device: 1,
            inode: self.inode,
            ancestors: Vec::new(),
        })
    }
}

pub(super) fn entry(name: &str, kind: ObservedKind, device: u64, inode: u64) -> ObservedEntry {
    ObservedEntry {
        name: name.as_bytes().to_vec(),
        enumerated_inode: inode,
        enumerated_kind: kind.clone(),
        metadata: Ok(MetadataObservation {
            kind,
            device,
            inode,
            size: Ok(1),
            allocated: ObservedValue::NotApplicable,
            link_count: ObservedValue::NotApplicable,
            space_identity: ObservedValue::NotApplicable,
            modified_at: ObservedValue::NotApplicable,
            native_modified_at: None,
        }),
    }
}

pub(super) fn dir(name: &str, inode: u64) -> ObservedEntry {
    entry(name, ObservedKind::Directory, 1, inode)
}

pub(super) fn file(name: &str, inode: u64) -> ObservedEntry {
    entry(name, ObservedKind::File, 1, inode)
}

pub(super) fn path(parts: &[&str]) -> Vec<Vec<u8>> {
    parts.iter().map(|part| part.as_bytes().to_vec()).collect()
}

/// Names starting with `x` are excluded; with the filter, names starting
/// with `u` cannot be decided.
pub(super) fn legacy(path: &[Vec<u8>]) -> bool {
    path.last().is_none_or(|name| !name.starts_with(b"x"))
}

pub(super) fn filter(_: u32, _: &[Vec<u8>], name: &[u8]) -> FilterDecision {
    if name.starts_with(b"x") {
        FilterDecision::Exclude
    } else if name.starts_with(b"u") {
        FilterDecision::Undecidable
    } else {
        FilterDecision::Admit
    }
}

/// Cancels once the tree's trigger fired.
pub(super) struct Cancelling {
    pub(super) flag: Arc<AtomicBool>,
    pub(super) inner: Context,
}

impl JobContext for Cancelling {
    fn checkpoint(&self) -> Checkpoint {
        if self.flag.load(SeqCst) {
            Checkpoint::Cancel
        } else {
            Checkpoint::Continue
        }
    }
    fn try_start_flow(&self) -> Result<Box<dyn JobFlow>, JobFlowError> {
        self.inner.try_start_flow()
    }
}

pub(super) fn context(tree: &Tree) -> Cancelling {
    Cancelling {
        flag: Arc::clone(&tree.cancel),
        inner: Context {
            completed: Arc::new(AtomicUsize::new(0)),
            abandoned: Arc::new(AtomicUsize::new(0)),
        },
    }
}

/// How a test collects the tree.
#[derive(Clone, Copy, Debug)]
pub(super) enum Collector {
    Legacy(usize),
    Filtered(usize),
    Sequential,
}

pub(super) const COLLECTORS: [Collector; 5] = [
    Collector::Legacy(1),
    Collector::Legacy(4),
    Collector::Filtered(1),
    Collector::Filtered(4),
    Collector::Sequential,
];

/// Collects `tree` once and pins the sealed version.
pub(super) fn collect(
    tree: &Arc<Tree>,
    collector: Collector,
) -> Result<(JobOutcome, Arc<ResultVersion>), String> {
    let ids = Ids(AtomicUsize::new(1));
    let sink = IngestSink::default();
    let context = context(tree);
    let devices = || DevicePlan {
        groups: vec![0],
        limit: 4,
    };
    let build = || sink.open_forest(ids.next_id(), vec![observed_root("C:\\R", 1)]);
    let outcome = match collector {
        Collector::Legacy(workers) => collect_open_forest_with_groups(
            build(),
            vec![tree.root()],
            &ids,
            &context,
            workers,
            &legacy,
            devices(),
        ),
        Collector::Filtered(workers) => collect_open_forest_filtered(
            build(),
            vec![tree.root()],
            &ids,
            &context,
            workers,
            &filter,
            devices(),
        ),
        Collector::Sequential => crate::scan::collect(
            &sink,
            ids.next_id(),
            tree.root(),
            &ids,
            || context.checkpoint(),
            |path: &[Vec<u8>]| legacy(path),
        ),
    };
    let version = sink.pin().map_err(|e| format!("{e:?}"))?.ok_or("missing")?;
    Ok((outcome, version))
}

/// Every directory item of a version by its path below the root.
pub(super) fn folders(version: &ResultVersion) -> HashMap<Vec<Vec<u8>>, ItemRef> {
    let by_no: HashMap<u32, &ResultItem> = version
        .items()
        .iter()
        .map(|item| (item.item_no, item))
        .collect();
    let path_of = |mut item_no: u32| {
        let mut parts = Vec::new();
        while let Some(item) = by_no.get(&item_no)
            && let Some(parent) = item.parent
        {
            parts.push(item.name.clone());
            item_no = parent;
        }
        parts.reverse();
        parts
    };
    version
        .items()
        .iter()
        .filter(|item| item.kind == ResultItemKind::Directory)
        .map(|item| {
            let reference = ItemRef {
                result_version: version.version_id().clone(),
                item_no: item.item_no,
            };
            (path_of(item.item_no), reference)
        })
        .collect()
}

pub(super) fn excluded(
    version: &ResultVersion,
    parts: &[&str],
) -> Result<Option<ExcludedChildren>, String> {
    let item = folders(version)
        .remove(&path(parts))
        .ok_or_else(|| format!("no folder {parts:?}"))?;
    version
        .folder_excluded_children(&item)
        .map_err(|e| format!("{e:?}"))
}

/// The folders whose entries the version reports as not read, with the reason.
pub(super) fn unread(version: &ResultVersion) -> Result<HashMap<Vec<Vec<u8>>, String>, String> {
    let mut found = HashMap::new();
    for (path, item) in folders(version) {
        let state = version
            .folder_collection(&item)
            .map_err(|e| format!("{e:?}"))?;
        if let FolderCollection::Folder {
            unenumerated: Unenumerated::Yes { reason_key },
            ..
        } = state
        {
            found.insert(path, reason_key.to_owned());
        }
    }
    Ok(found)
}

/// The folders marked as left unread by cancellation.
pub(super) fn cancelled(version: &ResultVersion) -> Result<HashSet<Vec<Vec<u8>>>, String> {
    Ok(unread(version)?
        .into_iter()
        .filter(|(_, reason)| reason == REASON_CANCELLED)
        .map(|(path, _)| path)
        .collect())
}
