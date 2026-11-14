//! Synthetic trees computed from directory numbers; no tree is stored.
//!
//! Directories of one root are numbered in preorder. A directory knows how many
//! descendant directories it has (its budget); the shape turns that budget into
//! child budgets, so a child's number is its parent's number plus one plus the
//! sizes of the subtrees before it. Only open handles hold their child list.
//! Subdirectories are listed before files and a read returns `batch` entries.
use super::*;
use std::sync::atomic::AtomicU64;

/// Tree families of the measurement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    /// The root holds every directory; each directory holds `files` files.
    Wide,
    /// One chain of directories; each directory holds `files` files.
    Deep,
    /// Uneven fan-out and file counts drawn from the directory number.
    Mixed,
    /// Eight subdirectories and `files` files per directory, so with `files`
    /// equal to the read batch every first read returns a full batch.
    Full,
}

impl Kind {
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Wide => "wide",
            Self::Deep => "deep",
            Self::Mixed => "mixed",
            Self::Full => "full",
        }
    }

    pub(super) fn parse(text: &str) -> Option<Self> {
        [Self::Wide, Self::Deep, Self::Mixed, Self::Full]
            .into_iter()
            .find(|kind| kind.name() == text)
    }
}

/// One root's tree: `dirs` descendant directories of the root.
#[derive(Clone, Copy, Debug)]
pub(super) struct TreeShape {
    pub(super) kind: Kind,
    pub(super) dirs: u64,
    pub(super) files: u32,
}

/// Children `(number, budget)` and file count of one directory.
struct Plan {
    children: Vec<(u64, u64)>,
    files: u32,
}

/// SplitMix64 finalizer; spreads directory numbers into pseudo-random bits.
fn mix(value: u64) -> u64 {
    let mut z = value.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// Mostly a few subdirectories, sometimes hundreds or thousands.
fn mixed_children(id: u64, budget: u64) -> Vec<u64> {
    if budget == 0 {
        return Vec::new();
    }
    let h = mix(id);
    let draw = h >> 10;
    let wanted = match h % 1000 {
        0..=699 => 1 + draw % 4,
        700..=949 => 5 + draw % 16,
        950..=994 => 21 + draw % 180,
        _ => 201 + draw % 1800,
    };
    let count = wanted.min(budget);
    // Heavy-tailed weights: a few children receive most of the subtree.
    let weights: Vec<u64> = (0..count)
        .map(|index| 1 << (mix(id ^ (index + 1).wrapping_mul(0x2545_f491_4f6c_dd1d)) % 12))
        .collect();
    split(budget - count, &weights)
}

/// Splits `rest` in proportion to `weights`; the remainder goes to the first.
fn split(rest: u64, weights: &[u64]) -> Vec<u64> {
    let total: u128 = weights.iter().map(|&weight| u128::from(weight)).sum();
    let mut budgets: Vec<u64> = weights
        .iter()
        .map(|&weight| (u128::from(rest) * u128::from(weight) / total.max(1)) as u64)
        .collect();
    let assigned: u64 = budgets.iter().sum();
    if let Some(first) = budgets.first_mut() {
        *first += rest - assigned;
    }
    budgets
}

/// About 11.5 files per directory on average, a few with hundreds.
fn mixed_files(id: u64) -> u32 {
    let h = mix(id ^ 0x00f1_1e5f_11e5);
    let draw = h >> 8;
    let files = match h % 100 {
        0..=44 => draw % 4,
        45..=84 => 4 + draw % 12,
        85..=97 => 16 + draw % 25,
        _ => 41 + draw % 260,
    };
    files as u32
}

impl TreeShape {
    fn plan(&self, id: u64, budget: u64) -> Plan {
        let budgets = match self.kind {
            Kind::Wide => vec![0; budget as usize],
            Kind::Deep if budget > 0 => vec![budget - 1],
            Kind::Deep => Vec::new(),
            Kind::Mixed => mixed_children(id, budget),
            Kind::Full => {
                let count = budget.min(8);
                split(budget - count, &vec![1; count as usize])
            }
        };
        let files = match self.kind {
            Kind::Mixed => mixed_files(id),
            _ => self.files,
        };
        let mut next = id + 1;
        let children = budgets
            .into_iter()
            .map(|budget| {
                let child = (next, budget);
                next += budget + 1;
                child
            })
            .collect();
        Plan { children, files }
    }

    /// (directories, files, deepest directory) of one root, root included.
    pub(super) fn expected(&self) -> (u64, u64, u64) {
        let (mut dirs, mut files, mut deepest) = (0, 0, 0);
        let mut pending = vec![(0, self.dirs, 0)];
        while let Some((id, budget, depth)) = pending.pop() {
            let plan = self.plan(id, budget);
            dirs += 1;
            files += u64::from(plan.files);
            deepest = deepest.max(depth);
            pending.extend(
                plan.children
                    .iter()
                    .map(|&(id, budget)| (id, budget, depth + 1)),
            );
        }
        (dirs, files, deepest)
    }
}

/// Shape, read behavior and counters shared by every handle of one run.
pub(super) struct Source {
    shape: TreeShape,
    batch: usize,
    delay: Duration,
    pub(super) reads: AtomicU64,
    pub(super) slept_nanos: AtomicU64,
    returned: AtomicU64,
    /// Entries recorded by the consumer; set by the measurement consumer.
    pub(super) consumed: AtomicU64,
    /// Most entries returned by reads and not yet recorded by the consumer.
    pub(super) unconsumed_peak: AtomicU64,
    live: AtomicUsize,
    pub(super) live_peak: AtomicUsize,
}

impl Source {
    pub(super) fn new(shape: TreeShape, batch: usize, delay: Duration) -> Arc<Self> {
        Arc::new(Self {
            shape,
            batch,
            delay,
            reads: AtomicU64::new(0),
            slept_nanos: AtomicU64::new(0),
            returned: AtomicU64::new(0),
            consumed: AtomicU64::new(0),
            unconsumed_peak: AtomicU64::new(0),
            live: AtomicUsize::new(0),
            live_peak: AtomicUsize::new(0),
        })
    }

    /// One copy of the shape per root; root `r` uses its own identities.
    pub(super) fn roots(self: &Arc<Self>, count: usize) -> Vec<Box<dyn AnchoredDirectory>> {
        (0..count)
            .map(|root| {
                Box::new(Computed::new(
                    self.clone(),
                    root as u64,
                    (0, self.shape.dirs),
                )) as Box<dyn AnchoredDirectory>
            })
            .collect()
    }
}

/// Inode of directory `id` of `root`; file `n` of it uses `n` in the low bits.
/// Never zero, which the collector treats as an unclassified entry.
fn inode(root: u64, id: u64, file: Option<u64>) -> u64 {
    (root << 54) | ((id + 1) << 14) | file.unwrap_or(0x3fff)
}

fn observed(name: Vec<u8>, inode: u64, directory: bool) -> ObservedEntry {
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

/// One open handle of a computed tree; dropping it closes the handle.
struct Computed {
    source: Arc<Source>,
    root: u64,
    id: u64,
    plan: Plan,
    next: usize,
}

impl Computed {
    fn new(source: Arc<Source>, root: u64, (id, budget): (u64, u64)) -> Self {
        let live = source.live.fetch_add(1, Ordering::Relaxed) + 1;
        source.live_peak.fetch_max(live, Ordering::Relaxed);
        let plan = source.shape.plan(id, budget);
        Self {
            source,
            root,
            id,
            plan,
            next: 0,
        }
    }

    fn entry(&self, index: usize) -> ObservedEntry {
        let dirs = self.plan.children.len();
        match self.plan.children.get(index) {
            Some(&(child, _)) => observed(
                format!("d{index}").into_bytes(),
                inode(self.root, child, None),
                true,
            ),
            None => {
                let file = (index - dirs) as u64;
                observed(
                    format!("f{file}").into_bytes(),
                    inode(self.root, self.id, Some(file)),
                    false,
                )
            }
        }
    }
}

impl Drop for Computed {
    fn drop(&mut self) {
        self.source.live.fetch_sub(1, Ordering::Relaxed);
    }
}

impl DirectoryObservations for Computed {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        let source = &*self.source;
        if !source.delay.is_zero() {
            let started = Instant::now();
            thread::sleep(source.delay);
            let slept = u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX);
            source.slept_nanos.fetch_add(slept, Ordering::Relaxed);
        }
        source.reads.fetch_add(1, Ordering::Relaxed);
        let total = self.plan.children.len() + self.plan.files as usize;
        let end = (self.next + source.batch).min(total);
        let batch: Vec<_> = (self.next..end).map(|index| self.entry(index)).collect();
        self.next = end;
        let count = batch.len() as u64;
        let returned = source.returned.fetch_add(count, Ordering::Relaxed) + count;
        let consumed = source.consumed.load(Ordering::Relaxed);
        source
            .unconsumed_peak
            .fetch_max(returned.saturating_sub(consumed), Ordering::Relaxed);
        Ok(batch)
    }
}

impl AnchoredDirectory for Computed {
    fn observed_identity(&self) -> Option<HandleIdentity> {
        Some(HandleIdentity {
            device: 1,
            inode: inode(self.root, self.id, None),
            ancestors: Vec::new(),
        })
    }

    fn open_child(&self, entry: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        let child = entry
            .name
            .strip_prefix(b"d")
            .and_then(|digits| std::str::from_utf8(digits).ok()?.parse::<usize>().ok())
            .and_then(|index| self.plan.children.get(index).copied())
            .ok_or_else(|| io_error("test.not_a_directory"))?;
        Ok(Box::new(Computed::new(
            self.source.clone(),
            self.root,
            child,
        )))
    }
}
