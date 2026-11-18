//! Lane tests over a synthetic tree (NFR2.HD1.app-host.DD1, NFR3.HD1.app-host.DD1).
//!
//! Every synthetic directory has its own identity on device 1, so the
//! admission table enters each directory exactly once. A test hook runs before
//! every open and read and can delay, block or fail the call.
use super::*;
use du_core_contracts::{job::JobOutcome, result::ResultItem, scan_observation::HandleIdentity};
use std::{
    collections::HashMap,
    panic::Location,
    time::{Duration, Instant},
};

mod assignment_cost;
mod bounds;
mod control;
mod determinism;
mod gate;
mod isolation;
use gate::{Gate, Opened};
mod lending;
mod measure;
mod moves;
mod order;
mod priority;
mod remote;

type TestError = Box<dyn std::error::Error + Send + Sync>;

/// Every directory above `depth` holds `dirs` subdirectories and every
/// directory holds `files` files. One read returns at most `batch` entries.
#[derive(Clone, Copy)]
struct Shape {
    depth: usize,
    dirs: usize,
    files: usize,
    batch: usize,
}

impl Shape {
    /// Directories of one root, the root included.
    fn directories(self) -> usize {
        (0..=self.depth)
            .map(|level| self.dirs.pow(level as u32))
            .sum()
    }

    /// Result items of one fully collected root, the root item included.
    fn items(self) -> usize {
        self.directories() * (1 + self.files)
    }

    /// Reads of one fully collected root, each final empty read included.
    fn reads(self) -> usize {
        (0..=self.depth)
            .map(|level| {
                let dirs = if level < self.depth { self.dirs } else { 0 };
                let entries = dirs + self.files;
                self.dirs.pow(level as u32) * (entries.div_ceil(self.batch) + 1)
            })
            .sum()
    }
}

/// An OS call of a synthetic directory, passed to the hook before it runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Call {
    Open,
    Read,
}

type Hook = Box<dyn Fn(u32, &[Vec<u8>], Call) -> Result<(), PortError> + Send + Sync>;

/// Shape, hook and counters shared by every directory of one test tree.
struct Probe {
    shape: Shape,
    hook: Hook,
    live: AtomicUsize,
    peak: AtomicUsize,
    returned: AtomicUsize,
    /// Opens and reads that started and did not yet return or fail.
    in_flight: AtomicUsize,
    opens: Vec<AtomicUsize>,
    reads: Vec<AtomicUsize>,
    finished: Vec<AtomicUsize>,
}

impl Probe {
    fn new(shape: Shape, roots: usize, hook: Hook) -> Arc<Self> {
        let counters = || (0..roots).map(|_| AtomicUsize::new(0)).collect();
        Arc::new(Self {
            shape,
            hook,
            live: AtomicUsize::new(0),
            peak: AtomicUsize::new(0),
            returned: AtomicUsize::new(0),
            in_flight: AtomicUsize::new(0),
            opens: counters(),
            reads: counters(),
            finished: counters(),
        })
    }

    fn quiet(shape: Shape, roots: usize) -> Arc<Self> {
        Self::new(shape, roots, Box::new(|_, _, _| Ok(())))
    }

    fn roots(self: &Arc<Self>) -> Vec<Box<dyn AnchoredDirectory>> {
        (0..self.opens.len())
            .map(|root| {
                Box::new(Synthetic::new(self.clone(), root as u32, Vec::new()))
                    as Box<dyn AnchoredDirectory>
            })
            .collect()
    }

    /// Opens and reads that returned, over all roots.
    fn calls(&self) -> usize {
        sum(&self.opens) + sum(&self.reads)
    }

    fn finished(&self, root: usize) -> usize {
        self.finished[root].load(Ordering::SeqCst)
    }
}

fn sum(counters: &[AtomicUsize]) -> usize {
    counters
        .iter()
        .map(|count| count.load(Ordering::SeqCst))
        .sum()
}

/// Counts one OS call of a synthetic directory as in flight until dropped.
struct InFlight<'a>(&'a AtomicUsize);

impl<'a> InFlight<'a> {
    fn enter(calls: &'a AtomicUsize) -> Self {
        calls.fetch_add(1, Ordering::SeqCst);
        Self(calls)
    }
}

impl Drop for InFlight<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// FNV-1a over the root and path; never zero and clear of `test_identity`.
fn identity(root: u32, path: &[Vec<u8>]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let bytes = root.to_le_bytes().into_iter().chain(
        path.iter()
            .flat_map(|part| std::iter::once(0xff).chain(part.iter().copied())),
    );
    for byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    (hash >> 2) | 1
}

fn entry(root: u32, parent: &[Vec<u8>], name: Vec<u8>, directory: bool) -> ObservedEntry {
    let mut path = parent.to_vec();
    path.push(name.clone());
    let inode = identity(root, &path);
    let kind = if directory {
        ObservedKind::Directory
    } else {
        ObservedKind::File
    };
    ObservedEntry {
        name,
        enumerated_inode: inode,
        enumerated_kind: kind.clone(),
        metadata: Ok(MetadataObservation {
            kind,
            device: 1,
            inode,
            size: Ok(u64::from(!directory)),
            allocated: None,
            link_count: None,
            space_identity: None,
            modified_at: None,
            native_modified_at: None,
        }),
    }
}

/// One open handle of the synthetic tree; dropping it closes the handle.
struct Synthetic {
    probe: Arc<Probe>,
    root: u32,
    path: Vec<Vec<u8>>,
    next: usize,
    done: bool,
}

impl Synthetic {
    fn new(probe: Arc<Probe>, root: u32, path: Vec<Vec<u8>>) -> Self {
        let live = probe.live.fetch_add(1, Ordering::SeqCst) + 1;
        probe.peak.fetch_max(live, Ordering::SeqCst);
        Self {
            probe,
            root,
            path,
            next: 0,
            done: false,
        }
    }

    fn names(&self) -> Vec<(Vec<u8>, bool)> {
        let shape = self.probe.shape;
        let dirs = if self.path.len() < shape.depth {
            shape.dirs
        } else {
            0
        };
        (0..dirs)
            .map(|index| (format!("d{index}").into_bytes(), true))
            .chain((0..shape.files).map(|index| (format!("f{index}").into_bytes(), false)))
            .collect()
    }
}

impl Drop for Synthetic {
    fn drop(&mut self) {
        self.probe.live.fetch_sub(1, Ordering::SeqCst);
    }
}

impl DirectoryObservations for Synthetic {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        let _call = InFlight::enter(&self.probe.in_flight);
        (self.probe.hook)(self.root, &self.path, Call::Read)?;
        let names = self.names();
        let end = (self.next + self.probe.shape.batch).min(names.len());
        let batch: Vec<_> = names[self.next..end]
            .iter()
            .map(|(name, directory)| entry(self.root, &self.path, name.clone(), *directory))
            .collect();
        self.next = end;
        let root = self.root as usize;
        if batch.is_empty() && !std::mem::replace(&mut self.done, true) {
            self.probe.finished[root].fetch_add(1, Ordering::SeqCst);
        }
        self.probe.returned.fetch_add(batch.len(), Ordering::SeqCst);
        self.probe.reads[root].fetch_add(1, Ordering::SeqCst);
        Ok(batch)
    }
}

impl AnchoredDirectory for Synthetic {
    fn observed_identity(&self) -> Option<HandleIdentity> {
        Some(HandleIdentity {
            device: 1,
            inode: identity(self.root, &self.path),
            ancestors: Vec::new(),
        })
    }

    fn open_child(&self, entry: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        let _call = InFlight::enter(&self.probe.in_flight);
        let mut path = self.path.clone();
        path.push(entry.name.clone());
        (self.probe.hook)(self.root, &path, Call::Open)?;
        self.probe.opens[self.root as usize].fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(Synthetic::new(
            self.probe.clone(),
            self.root,
            path,
        )))
    }
}

fn io_error(reason: &str) -> PortError {
    PortError {
        kind: PortErrorKind::Io,
        reason_key: reason.into(),
        os_code: None,
    }
}

/// Polls `ready` until it holds; a timeout names the caller and the time waited.
#[track_caller]
fn wait_until(mut ready: impl FnMut() -> bool) -> Result<(), TestError> {
    let (awaited, limit, started) = (Location::caller(), Duration::from_secs(20), Instant::now());
    while !ready() {
        if started.elapsed() >= limit {
            let elapsed = started.elapsed();
            let message = format!(
                "condition awaited at {awaited} was not reached after {elapsed:?} (limit {limit:?})"
            );
            return Err(message.into());
        }
        thread::sleep(Duration::from_millis(2));
    }
    Ok(())
}

/// One collection through the forest entry point and its published result.
struct Run {
    outcome: JobOutcome,
    completeness: ResultCompleteness,
    /// Sorted (root, path, kind, size) rows of every result item.
    rows: Vec<String>,
    /// Sorted scope gaps and scan issues.
    issues: Vec<String>,
}

/// Collects every root of `probe`; root `i` is in device group `groups[i]`
/// and on its own volume, so a disconnection closes only that root.
fn run_forest(
    probe: &Arc<Probe>,
    groups: Vec<usize>,
    workers: usize,
    limit: usize,
    context: &dyn JobContext,
) -> Result<Run, TestError> {
    let ids = Ids(AtomicUsize::new(1));
    let sink = IngestSink::default();
    let roots = (0..groups.len())
        .map(|index| observed_root(&format!("L:\\{index}"), index as u8 + 1))
        .collect();
    let build = sink.open_forest(ids.next_id(), roots);
    let devices = DevicePlan { groups, limit };
    let include = |_: &[Vec<u8>]| true;
    let outcome = collect_open_forest_with_groups(
        build,
        probe.roots(),
        &ids,
        context,
        workers,
        &include,
        devices,
    );
    let version = sink
        .pin()
        .map_err(|error| format!("{error:?}"))?
        .ok_or("missing result")?;
    let mut issues: Vec<_> = version
        .scope_gaps()
        .iter()
        .map(|gap| format!("{gap:?}"))
        .chain(
            version
                .scan_issues()
                .iter()
                .map(|issue| format!("{issue:?}")),
        )
        .collect();
    issues.sort();
    Ok(Run {
        outcome,
        completeness: version.completeness(),
        rows: rows(&version)?,
        issues,
    })
}

fn rows(version: &du_analysis_index::ResultVersion) -> Result<Vec<String>, TestError> {
    let items = version.items();
    let by_number: HashMap<u32, &ResultItem> =
        items.iter().map(|item| (item.item_no, item)).collect();
    let mut rows = Vec::with_capacity(items.len());
    for item in items {
        let mut names = vec![item.name.clone()];
        let mut parent = item.parent;
        while let Some(number) = parent {
            let up = by_number.get(&number).ok_or("missing parent item")?;
            names.push(up.name.clone());
            parent = up.parent;
        }
        names.reverse();
        let reference = ItemRef {
            result_version: version.version_id().clone(),
            item_no: item.item_no,
        };
        let (root, _) = version
            .observed_root_for(&reference)
            .map_err(|error| format!("{error:?}"))?;
        rows.push(format!("{root} {names:?} {:?} {:?}", item.kind, item.size));
    }
    rows.sort();
    Ok(rows)
}

/// Builds an unstarted scheduler over `probe` with every root identity reserved.
fn shared_for(
    probe: &Arc<Probe>,
    groups: Vec<usize>,
    limit: usize,
    capacity: usize,
) -> Result<Shared, TestError> {
    shared_over(probe.roots(), groups, limit, capacity)
}

/// Builds an unstarted scheduler over `roots` with every root identity reserved.
fn shared_over(
    roots: Vec<Box<dyn AnchoredDirectory>>,
    groups: Vec<usize>,
    limit: usize,
    capacity: usize,
) -> Result<Shared, TestError> {
    let count = roots.len();
    let identities = roots.iter().map(|root| root.observed_identity()).collect();
    let (visited, starts) = admission::Visited::reserve_roots(identities);
    let boundaries: Vec<_> = starts
        .iter()
        .map(|start| match start {
            admission::RootStart::Collect(boundary) => Some(*boundary),
            admission::RootStart::Revisit => None,
        })
        .collect();
    let options = scheduler::LaneOptions {
        capacity,
        track_peak: true,
        track_paths: false,
    };
    let queue = scheduler::queue_for(roots, &boundaries, &DevicePlan { groups, limit }, options)
        .map_err(|error| format!("{error:?}"))?;
    Ok(Shared {
        queue: Arc::new(Mutex::new(queue)),
        wake: Arc::new(Condvar::new()),
        abort: AtomicBool::new(false),
        disconnects: super::super::disconnect::Disconnects::new(count, &[]),
        track_priority_paths: false,
        visited,
        correlated_budget: super::super::correlated_budget::CorrelatedBudget::new(),
    })
}

/// Runs `workers` workers over `shared` and hands their events to `consume`.
///
/// Workers are joined after `consume` returns; dropping the receiver early
/// makes blocked senders fail, so no worker outlives the call.
fn run_workers<R>(
    shared: &Shared,
    workers: usize,
    channel: usize,
    consume: impl FnOnce(mpsc::Receiver<Event>) -> R,
) -> Result<R, TestError> {
    let context = Context {
        completed: Arc::new(AtomicUsize::new(0)),
        abandoned: Arc::new(AtomicUsize::new(0)),
    };
    run_workers_in(shared, workers, channel, &context, |receiver, sender| {
        drop(sender);
        consume(receiver)
    })
}

/// Like [`run_workers`] under `context`; `consume` also gets a clone of the
/// workers' sender, which it must drop before waiting for the channel to end.
fn run_workers_in<R>(
    shared: &Shared,
    workers: usize,
    channel: usize,
    context: &dyn JobContext,
    consume: impl FnOnce(mpsc::Receiver<Event>, SyncSender<Event>) -> R,
) -> Result<R, TestError> {
    let include = |_: &[Vec<u8>]| true;
    let (sender, receiver) = mpsc::sync_channel(channel);
    thread::scope(|scope| {
        let flows = acquire_flows(context, workers).map_err(|error| format!("{error:?}"))?;
        spawn_workers(
            scope,
            flows,
            shared,
            &sender,
            context,
            Policy::Legacy(&include),
        )
        .and_then(|()| start_queue(shared))
        .map_err(|error| format!("{error:?}"))?;
        Ok(consume(receiver, sender))
    })
}
