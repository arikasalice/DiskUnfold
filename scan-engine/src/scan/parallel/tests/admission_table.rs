//! FR1.HD1.app-host.DD1 and FR6.HD1.app-host.DD1 through the collectors.
//!
//! The fixture serves listings keyed by directory inode, so entries with one
//! identity share a listing. A nonzero seed varies enumeration delays, which
//! changes how workers interleave between runs.
use super::*;
use du_core_contracts::{
    result::{ResultItem, ResultItemKind},
    scan_failures::{ScanFailureKind as Kind, ScanFailureStage as Stage},
    scan_observation::{FilterDecision, FilteredBatch, HandleIdentity},
};
use std::collections::{BTreeMap, HashMap};
mod roots;
mod rows;

const ROOT: u64 = 100;
const SKIP: u64 = 103;
const ODD: u64 = 104;
const MOUNT: u64 = 300;
/// Worker counts and delay seeds that each determinism test runs.
const RUNS: [(usize, u64); 7] = [(1, 0), (1, 3), (2, 1), (4, 2), (8, 3), (4, 5), (8, 7)];

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

type Listings = BTreeMap<u64, Vec<Node>>;

struct Tree {
    listings: Listings,
    enumerations: Mutex<BTreeMap<u64, usize>>,
    probes: Mutex<Vec<&'static str>>,
    seed: u64,
    control: Mutex<Option<ScanControlHandle>>,
    priority: Mutex<Vec<Result<ScanPriorityOutcome, ScanControlError>>>,
}

/// One volume with an alias pair, a cycle, a mount, links and filtered names.
fn listings() -> Listings {
    use ObservedKind::{Directory as D, File as F, Link as L};
    BTreeMap::from([
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
    ])
}

fn tree(seed: u64) -> Arc<Tree> {
    tree_of(listings(), seed)
}

fn tree_of(listings: Listings, seed: u64) -> Arc<Tree> {
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

    /// Observes like an adapter: an entry on another volume than its parent
    /// carries only its kind and identity, never a size.
    fn observe(&self, node: &Node, parent_device: u64) -> ObservedEntry {
        if let Ok(mut probes) = self.probes.lock() {
            probes.push(node.name);
        }
        let size = if node.device == parent_device {
            Ok(1)
        } else {
            Err(PortError {
                kind: PortErrorKind::Unsupported,
                reason_key: "os.unsupported".into(),
                os_code: None,
            })
        };
        ObservedEntry {
            name: node.name.as_bytes().to_vec(),
            enumerated_inode: node.inode,
            enumerated_kind: node.kind.clone(),
            metadata: Ok(MetadataObservation {
                kind: node.kind.clone(),
                device: node.device,
                inode: node.inode,
                size,
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
    /// Verified ancestors of a supported identity, or `None` when unsupported.
    identity: Option<Vec<u64>>,
    sent: bool,
}

impl Dir {
    fn root(tree: &Arc<Tree>, identity: bool) -> Box<dyn AnchoredDirectory> {
        Self::at(tree, ROOT, identity.then(Vec::new))
    }

    /// A root at `inode` on device 1 with the given verified ancestors.
    fn at(tree: &Arc<Tree>, inode: u64, identity: Option<Vec<u64>>) -> Box<dyn AnchoredDirectory> {
        Box::new(Self {
            tree: Arc::clone(tree),
            device: 1,
            inode,
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
        Ok(nodes
            .iter()
            .map(|node| self.tree.observe(node, self.device))
            .collect())
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
                FilterDecision::Admit => entries.push(self.tree.observe(node, self.device)),
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
            identity: Some(Vec::new()),
            sent: false,
        }))
    }

    fn observed_identity(&self) -> Option<HandleIdentity> {
        self.identity.clone().map(|ancestors| HandleIdentity {
            device: self.device,
            inode: self.inode,
            ancestors,
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

/// Root index, root-relative path, kind and whether a size was kept.
type Item = (u32, Vec<Vec<u8>>, ResultItemKind, bool);
type Issue = (u32, Vec<Vec<u8>>, Kind, Stage, String);
type Gap = (u32, Vec<Vec<u8>>, String);
type Summary = (Vec<Item>, Vec<Issue>, Vec<Gap>);

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
            // Root items come first, so a root's item number is its index.
            (current.item_no, parts, item.kind, item.size.is_some())
        })
        .collect();
    items.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
    let issues = version
        .scan_issues()
        .iter()
        .map(|issue| {
            let key = issue.reason_key.clone();
            let path = issue.relative.clone();
            (issue.root_index, path, issue.kind, issue.stage, key)
        })
        .collect();
    let mut gaps: Vec<Gap> = version
        .scope_gaps()
        .iter()
        .map(|gap| (gap.root_index, gap.path.clone(), gap.reason_key.clone()))
        .collect();
    gaps.sort();
    (items, issues, gaps)
}

fn path(parts: &[&str]) -> Vec<Vec<u8>> {
    parts.iter().map(|part| part.as_bytes().to_vec()).collect()
}

/// The temporary expression of an entry the table did not enter.
fn unread(root: u32, parts: &[&str]) -> Issue {
    let reason = "scan.unclassified".to_string();
    (
        root,
        path(parts),
        Kind::UnreadRange,
        Stage::Traverse,
        reason,
    )
}

fn link(root: u32, parts: &[&str]) -> Issue {
    let reason = "scan.link_untraversed".to_string();
    (
        root,
        path(parts),
        Kind::UntraversedLink,
        Stage::Traverse,
        reason,
    )
}

struct Run {
    outcome: JobOutcome,
    summary: Summary,
    completeness: ResultCompleteness,
}

/// Collects `roots` into one forest with the frozen pre-metadata filter.
fn filtered_forest(roots: Vec<Box<dyn AnchoredDirectory>>, workers: usize) -> Result<Run, String> {
    let ids = Ids(AtomicUsize::new(1));
    let sink = IngestSink::default();
    let observed = (0..roots.len())
        .map(|index| observed_root(&format!("C:\\R{index}"), 1))
        .collect();
    let build = sink.open_forest(ids.next_id(), observed);
    let groups = vec![0; roots.len()];
    let devices = DevicePlan { groups, limit: 4 };
    let outcome =
        collect_open_forest_filtered(build, roots, &ids, &context(), workers, &filter, devices);
    let version = sink
        .pin()
        .map_err(|e| format!("{e:?}"))?
        .ok_or("missing result")?;
    Ok(Run {
        outcome,
        summary: summary(&version),
        completeness: version.completeness(),
    })
}

fn filtered_run(tree: &Arc<Tree>, workers: usize, identity: bool) -> Result<Run, String> {
    filtered_forest(vec![Dir::root(tree, identity)], workers)
}
