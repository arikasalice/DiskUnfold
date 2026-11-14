//! FR1.HD1.app-host.DD1 and FR6.HD1.app-host.DD1 through the collectors.
use super::*;
use du_core_contracts::{
    result::{ResultItem, ResultItemKind},
    scan_failures::{ScanFailureKind as Kind, ScanFailureStage as Stage},
    scan_observation::{FilterDecision, FilteredBatch, HandleIdentity},
};
use std::collections::{BTreeMap, HashMap};

const ROOT: u64 = 100;
const SKIP: u64 = 103;
const ODD: u64 = 104;
const MOUNT: u64 = 300;

#[derive(Clone)]
struct Node {
    name: &'static str,
    kind: ObservedKind,
    device: u64,
    inode: u64,
}

fn node(name: &'static str, kind: ObservedKind, device: u64, inode: u64) -> Node {
    Node {
        name,
        kind,
        device,
        inode,
    }
}

/// Listings keyed by directory inode; entries with one identity share a listing.
struct Tree {
    listings: BTreeMap<u64, Vec<Node>>,
    enumerations: Mutex<BTreeMap<u64, usize>>,
    probes: Mutex<Vec<&'static str>>,
    seed: u64,
    control: Mutex<Option<ScanControlHandle>>,
    priority: Mutex<Vec<Result<ScanPriorityOutcome, ScanControlError>>>,
}

fn tree(seed: u64) -> Arc<Tree> {
    use ObservedKind::{Directory as D, File as F, Link as L};
    let listings = BTreeMap::from([
        (
            ROOT,
            vec![
                node("a", D, 1, 101),
                node("b", D, 1, 102),
                node("twin", D, 1, 102),
                node("mnt", D, 2, MOUNT),
                node("foreign.bin", F, 2, 301),
                node("away", L, 1, 204),
                node("skip", D, 1, SKIP),
                node("odd", D, 1, ODD),
                node("top.txt", F, 1, 205),
            ],
        ),
        (
            101,
            vec![
                node("deep.txt", F, 1, 201),
                node("up", L, 1, 202),
                node("loop", D, 1, ROOT),
            ],
        ),
        (102, vec![node("b.txt", F, 1, 203)]),
        (SKIP, vec![node("hidden.txt", F, 1, 206)]),
        (ODD, vec![node("odd.txt", F, 1, 207)]),
        (MOUNT, vec![node("other.txt", F, 2, 302)]),
    ]);
    Arc::new(Tree {
        listings,
        enumerations: Mutex::default(),
        probes: Mutex::default(),
        seed,
        control: Mutex::default(),
        priority: Mutex::default(),
    })
}

impl Tree {
    fn enumerated(&self, inode: u64) -> usize {
        self.enumerations.lock().map_or(usize::MAX, |counts| {
            counts.get(&inode).copied().unwrap_or(0)
        })
    }

    fn probed(&self, name: &str) -> bool {
        self.probes
            .lock()
            .map_or(true, |probes| probes.contains(&name))
    }

    fn observe(&self, node: &Node) -> ObservedEntry {
        if let Ok(mut probes) = self.probes.lock() {
            probes.push(node.name);
        }
        ObservedEntry {
            name: node.name.as_bytes().to_vec(),
            enumerated_inode: node.inode,
            enumerated_kind: node.kind.clone(),
            metadata: Ok(MetadataObservation {
                kind: node.kind.clone(),
                device: node.device,
                inode: node.inode,
                size: Ok(1),
                allocated: None,
                link_count: None,
                space_identity: None,
                modified_at: None,
                native_modified_at: None,
            }),
        }
    }
}

struct Dir {
    tree: Arc<Tree>,
    device: u64,
    inode: u64,
    identity: bool,
    sent: bool,
}

impl Dir {
    fn root(tree: &Arc<Tree>, identity: bool) -> Box<dyn AnchoredDirectory> {
        Box::new(Self {
            tree: Arc::clone(tree),
            device: 1,
            inode: ROOT,
            identity,
            sent: false,
        })
    }

    /// Counts one enumeration and varies completion order by seed.
    fn enter(&mut self) -> Option<Vec<Node>> {
        if std::mem::replace(&mut self.sent, true) {
            return None;
        }
        if let Ok(mut counts) = self.tree.enumerations.lock() {
            *counts.entry(self.inode).or_default() += 1;
        }
        // Every root row was decided before this child is enumerated.
        if self.inode == 102
            && let Ok(control) = self.tree.control.lock()
            && let Some(control) = control.as_ref()
            && let Ok(mut outcomes) = self.tree.priority.lock()
        {
            for name in ["mnt", "skip", "odd", "twin", "b"] {
                outcomes.push(control.prioritize(0, vec![name.as_bytes().to_vec()]));
            }
        }
        if self.tree.seed != 0 {
            let millis = (self.inode * 7 + self.tree.seed) % 4;
            thread::sleep(Duration::from_millis(millis));
        }
        Some(
            self.tree
                .listings
                .get(&self.inode)
                .cloned()
                .unwrap_or_default(),
        )
    }
}

impl DirectoryObservations for Dir {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        let nodes = self.enter().unwrap_or_default();
        Ok(nodes.iter().map(|node| self.tree.observe(node)).collect())
    }

    fn next_filtered(
        &mut self,
        include: &mut dyn FnMut(&[u8]) -> FilterDecision,
    ) -> Result<FilteredBatch, PortError> {
        let Some(nodes) = self.enter() else {
            return Ok(FilteredBatch::End);
        };
        let mut entries = Vec::new();
        for node in &nodes {
            match include(node.name.as_bytes()) {
                FilterDecision::Admit => entries.push(self.tree.observe(node)),
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

impl AnchoredDirectory for Dir {
    fn open_child(&self, entry: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        let meta = entry.metadata.as_ref().map_err(Clone::clone)?;
        Ok(Box::new(Self {
            tree: Arc::clone(&self.tree),
            device: meta.device,
            inode: meta.inode,
            identity: true,
            sent: false,
        }))
    }

    fn observed_identity(&self) -> Option<HandleIdentity> {
        self.identity.then(|| HandleIdentity {
            device: self.device,
            inode: self.inode,
            ancestors: Vec::new(),
        })
    }
}

fn filter(_: u32, parent: &[Vec<u8>], name: &[u8]) -> FilterDecision {
    match (parent.is_empty(), name) {
        (true, b"skip") => FilterDecision::Exclude,
        (true, b"odd") => FilterDecision::Undecidable,
        _ => FilterDecision::Admit,
    }
}

fn context() -> Context {
    Context {
        completed: Arc::new(AtomicUsize::new(0)),
        abandoned: Arc::new(AtomicUsize::new(0)),
    }
}

type Item = (Vec<Vec<u8>>, ResultItemKind, bool);
type Issue = (Vec<Vec<u8>>, Kind, Stage, String);
type Summary = (Vec<Item>, Vec<Issue>, Vec<(Vec<Vec<u8>>, String)>);

fn summary(version: &du_analysis_index::ResultVersion) -> Summary {
    let by_no: HashMap<u32, &ResultItem> = version
        .items()
        .iter()
        .map(|item| (item.item_no, item))
        .collect();
    let mut items: Vec<Item> = version
        .items()
        .iter()
        .map(|item| {
            let mut parts = Vec::new();
            let mut current = item;
            while let Some(parent) = current.parent {
                parts.push(current.name.clone());
                let Some(next) = by_no.get(&parent) else {
                    break;
                };
                current = next;
            }
            parts.reverse();
            (parts, item.kind, item.size.is_some())
        })
        .collect();
    items.sort_by(|a, b| a.0.cmp(&b.0));
    let issues = version
        .scan_issues()
        .iter()
        .map(|issue| {
            let key = issue.reason_key.clone();
            (issue.relative.clone(), issue.kind, issue.stage, key)
        })
        .collect();
    let mut gaps: Vec<_> = version
        .scope_gaps()
        .iter()
        .map(|gap| (gap.path.clone(), gap.reason_key.clone()))
        .collect();
    gaps.sort();
    (items, issues, gaps)
}

fn path(parts: &[&str]) -> Vec<Vec<u8>> {
    parts.iter().map(|part| part.as_bytes().to_vec()).collect()
}

fn filtered_run(
    tree: &Arc<Tree>,
    workers: usize,
    identity: bool,
) -> Result<(JobOutcome, Summary), String> {
    let ids = Ids(AtomicUsize::new(1));
    let sink = IngestSink::default();
    let build = sink.open_forest(ids.next_id(), vec![observed_root("C:\\A", 1)]);
    let outcome = collect_open_forest_filtered(
        build,
        vec![Dir::root(tree, identity)],
        &ids,
        &context(),
        workers,
        &filter,
        DevicePlan {
            groups: vec![0],
            limit: 4,
        },
    );
    let version = sink
        .pin()
        .map_err(|e| format!("{e:?}"))?
        .ok_or("missing result")?;
    Ok((outcome, summary(&version)))
}

#[test]
fn table_rows_decide_entry_without_opening_out_of_scope_directories() -> Result<(), String> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "lock")?;
    let tree = tree(0);
    let (outcome, (items, issues, gaps)) = filtered_run(&tree, 4, true)?;
    assert_eq!(outcome.counts.failed, 0);
    for inode in [SKIP, ODD, MOUNT] {
        assert_eq!(tree.enumerated(inode), 0, "inode {inode} was enumerated");
    }
    for inode in [ROOT, 101, 102] {
        assert_eq!(
            tree.enumerated(inode),
            1,
            "inode {inode} was not entered once"
        );
    }
    assert!(!tree.probed("odd") && !tree.probed("skip") && !tree.probed("other.txt"));
    let expected_items: Vec<Item> = vec![
        (path(&[]), ResultItemKind::Directory, false),
        (path(&["a"]), ResultItemKind::Directory, false),
        (path(&["a", "deep.txt"]), ResultItemKind::File, true),
        (path(&["a", "loop"]), ResultItemKind::Directory, false),
        (path(&["a", "up"]), ResultItemKind::UntraversedLink, false),
        (path(&["away"]), ResultItemKind::UntraversedLink, false),
        (path(&["b"]), ResultItemKind::Directory, false),
        (path(&["b", "b.txt"]), ResultItemKind::File, true),
        (path(&["foreign.bin"]), ResultItemKind::Unknown, false),
        (path(&["mnt"]), ResultItemKind::Directory, false),
        (path(&["top.txt"]), ResultItemKind::File, true),
        (path(&["twin"]), ResultItemKind::Directory, false),
    ];
    assert_eq!(items, expected_items);
    let unread = |parts: &[&str]| {
        (
            path(parts),
            Kind::UnreadRange,
            Stage::Traverse,
            "scan.unclassified".to_string(),
        )
    };
    let link = |parts: &[&str]| {
        let reason = "scan.link_untraversed".to_string();
        (path(parts), Kind::UntraversedLink, Stage::Traverse, reason)
    };
    let mut expected_issues = vec![
        unread(&["a", "loop"]),
        unread(&["foreign.bin"]),
        unread(&["mnt"]),
        unread(&["odd"]),
        unread(&["twin"]),
        link(&["a", "up"]),
        link(&["away"]),
    ];
    // Published order: relative path bytes, then kind and stage.
    expected_issues.sort_by(|a, b| (&a.0, a.1).cmp(&(&b.0, b.1)));
    assert_eq!(issues, expected_issues);
    let gap_paths: Vec<_> = gaps.into_iter().map(|(gap, _)| gap).collect();
    let expected_gaps = [
        &["a", "loop"][..],
        &["foreign.bin"],
        &["mnt"],
        &["odd"],
        &["twin"],
    ];
    assert_eq!(gap_paths, expected_gaps.map(path));
    Ok(())
}

#[test]
fn worker_count_and_delay_order_keep_the_same_result() -> Result<(), String> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "lock")?;
    let (_, baseline) = filtered_run(&tree(0), 1, true)?;
    for (workers, seed) in [(2, 1), (4, 2), (8, 3), (3, 5)] {
        let tree = tree(seed);
        let (_, summary) = filtered_run(&tree, workers, true)?;
        assert_eq!(summary, baseline, "workers={workers} seed={seed}");
        assert_eq!(tree.enumerated(ROOT), 1);
    }
    Ok(())
}

#[test]
fn unsupported_volume_identity_leaves_only_child_directories_unverifiable() -> Result<(), String> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "lock")?;
    let tree = tree(0);
    let (outcome, (items, issues, _)) = filtered_run(&tree, 2, false)?;
    assert!(outcome.result_version_id.is_some());
    assert_eq!(tree.enumerated(ROOT), 1);
    for inode in [101, 102, SKIP, ODD, MOUNT] {
        assert_eq!(tree.enumerated(inode), 0);
    }
    assert!(items.contains(&(path(&["top.txt"]), ResultItemKind::File, true)));
    for unverifiable in [&["a"][..], &["b"], &["twin"], &["mnt"], &["odd"]] {
        assert!(issues.contains(&(
            path(unverifiable),
            Kind::UnreadRange,
            Stage::Traverse,
            "scan.unclassified".into()
        )));
    }
    Ok(())
}

#[test]
fn sequential_and_legacy_parallel_paths_share_the_table() -> Result<(), String> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "lock")?;
    let include = |path: &[Vec<u8>]| path.first().is_none_or(|first| first != b"skip");
    let ids = Ids(AtomicUsize::new(1));
    let sequential_tree = tree(0);
    let sink = IngestSink::default();
    let outcome = crate::scan::collect(
        &sink,
        ids.next_id(),
        Dir::root(&sequential_tree, true),
        &ids,
        || Checkpoint::Continue,
        include,
    );
    assert!(outcome.result_version_id.is_some());
    let sequential = summary(&*sink.pin().map_err(|e| format!("{e:?}"))?.ok_or("missing")?);
    let parallel_tree = tree(0);
    let sink = IngestSink::default();
    let build = sink.open_forest(ids.next_id(), vec![observed_root("C:\\A", 1)]);
    let roots = vec![Dir::root(&parallel_tree, true)];
    collect_open_forest(build, roots, &ids, &context(), 3, &include);
    let parallel = summary(&*sink.pin().map_err(|e| format!("{e:?}"))?.ok_or("missing")?);
    assert_eq!(sequential, parallel);
    for tree in [&sequential_tree, &parallel_tree] {
        assert_eq!(tree.enumerated(SKIP), 0);
        assert_eq!(tree.enumerated(MOUNT), 0);
        assert_eq!(tree.enumerated(ROOT), 1);
        assert_eq!(tree.enumerated(ODD), 1);
    }
    Ok(())
}

#[test]
fn priority_requests_and_header_keep_the_frozen_scope() -> Result<(), String> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "lock")?;
    let tree = tree(0);
    let ids = Arc::new(Ids(AtomicUsize::new(1)));
    let sink = IngestSink::default();
    let scope = ids.next_id();
    let root = OpenedScopeRoot {
        canonical_path: OsPath {
            encoding: PathEncoding::WindowsUtf16le,
            raw: PathBytes::from_bytes(&native("C:\\A")),
            display: "C:\\A".into(),
            lossy: false,
        },
        volume_id: vec![1],
        file_id: vec![1],
        directory: Dir::root(&tree, true),
    };
    let job = crate::scan_job::MetadataScan::new(root, sink.clone(), scope.clone(), ids)
        .with_scope_fingerprint("scope-fingerprint".into())
        .with_pre_metadata_filter(Arc::new(filter))
        .with_priority_control();
    if let Ok(mut control) = tree.control.lock() {
        *control = job.control_handle();
    }
    let outcome = job.run(Arc::new(context()));
    let version = sink.pin().map_err(|e| format!("{e:?}"))?.ok_or("missing")?;
    assert_eq!(
        outcome.result_version_id.as_ref(),
        Some(version.version_id())
    );
    assert_eq!(version.scope_version_id(), &scope);
    assert_eq!(version.scope_fingerprint(), Some("scope-fingerprint"));
    let outcomes = tree.priority.lock().map_err(|_| "priority")?.clone();
    assert_eq!(outcomes.len(), 5);
    assert!(outcomes[..4].iter().all(|outcome| matches!(
        outcome,
        Ok(ScanPriorityOutcome::NoEffect {
            same_region: false,
            remaining_directories: 0,
        })
    )));
    assert_eq!(
        outcomes[4],
        Ok(ScanPriorityOutcome::Accepted {
            remaining_directories: 1
        })
    );
    for inode in [SKIP, ODD, MOUNT] {
        assert_eq!(tree.enumerated(inode), 0);
    }
    assert_eq!(tree.enumerated(102), 1);
    Ok(())
}
