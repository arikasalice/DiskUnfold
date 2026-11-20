//! Fixture directories that refuse opens, fail reads and break metadata.
use super::*;
use du_core_contracts::scan_observation::ObservedValue;

/// What a fixture directory does when it is opened or read.
#[derive(Clone)]
pub(super) enum Listing {
    /// One batch of entries, then the end.
    Entries(Vec<ObservedEntry>),
    /// The open is refused with this error.
    Refused(PortError),
    /// One batch of entries, if any, then this error.
    FailsAfter(Vec<ObservedEntry>, PortError),
    /// More entries than one batch may hold.
    Oversized,
    /// These batches in order, then this error or, without one, the end.
    Batches(Vec<Vec<ObservedEntry>>, Option<PortError>),
    /// Like `Batches` without an error, but every read after the first
    /// waits until cancellation is requested, so it is in flight then.
    Gated(Vec<Vec<ObservedEntry>>),
}

/// A listing key whose presence makes the root report no identity, so the
/// root's boundary cannot be verified.
pub(super) const UNVERIFIED: u64 = u64::MAX;

pub(super) struct Faults {
    listings: HashMap<u64, Listing>,
    /// Set once the root returns its first batch, or once a batch carries a
    /// disconnection when `cancel_on_disconnect` is set.
    pub(super) cancel: Arc<AtomicBool>,
    cancel_on_disconnect: bool,
    /// Reads and opens that started after `cancel` was set.
    pub(super) calls_after_cancel: AtomicUsize,
    /// Gated reads that gave up waiting for cancellation.
    pub(super) gate_timeouts: AtomicUsize,
    /// Reads per directory inode.
    reads: Mutex<HashMap<u64, usize>>,
    opened: Mutex<Vec<u64>>,
    /// A nonzero seed delays reads and opens, which varies worker interleaving.
    seed: u64,
}

pub(super) struct FaultDir {
    faults: Arc<Faults>,
    inode: u64,
    reads: usize,
}

impl FaultDir {
    pub(super) fn root(
        listings: HashMap<u64, Listing>,
    ) -> (Arc<Faults>, Box<dyn AnchoredDirectory>) {
        Self::seeded(listings, 0)
    }

    /// Like [`FaultDir::root`], with seed-dependent delays.
    pub(super) fn seeded(
        listings: HashMap<u64, Listing>,
        seed: u64,
    ) -> (Arc<Faults>, Box<dyn AnchoredDirectory>) {
        Self::with_cancel(listings, seed, false)
    }

    /// Like [`FaultDir::seeded`], requesting cancellation when a returned
    /// batch first carries disconnection evidence instead of at the root's
    /// first batch.
    pub(super) fn cancelling_on_disconnect(
        listings: HashMap<u64, Listing>,
        seed: u64,
    ) -> (Arc<Faults>, Box<dyn AnchoredDirectory>) {
        Self::with_cancel(listings, seed, true)
    }

    fn with_cancel(
        listings: HashMap<u64, Listing>,
        seed: u64,
        cancel_on_disconnect: bool,
    ) -> (Arc<Faults>, Box<dyn AnchoredDirectory>) {
        let faults = Arc::new(Faults {
            listings,
            cancel: Arc::new(AtomicBool::new(false)),
            cancel_on_disconnect,
            calls_after_cancel: AtomicUsize::new(0),
            gate_timeouts: AtomicUsize::new(0),
            reads: Mutex::default(),
            opened: Mutex::default(),
            seed,
        });
        let root = Box::new(Self {
            faults: Arc::clone(&faults),
            inode: ROOT,
            reads: 0,
        });
        (faults, root)
    }
}

impl Faults {
    pub(super) fn opened(&self, inode: u64) -> bool {
        self.opened
            .lock()
            .map_or(true, |opened| opened.contains(&inode))
    }

    /// How many times the directory with `inode` was read.
    pub(super) fn reads(&self, inode: u64) -> usize {
        self.reads
            .lock()
            .map_or(usize::MAX, |reads| reads.get(&inode).copied().unwrap_or(0))
    }

    /// Counts one port call, noting whether cancellation was already requested.
    fn call(&self) {
        if self.cancel.load(SeqCst) {
            self.calls_after_cancel.fetch_add(1, SeqCst);
        }
    }

    /// Waits, at most three seconds, until cancellation is requested.
    fn wait_for_cancel(&self) {
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while !self.cancel.load(SeqCst) {
            if std::time::Instant::now() >= deadline {
                self.gate_timeouts.fetch_add(1, SeqCst);
                return;
            }
            thread::sleep(Duration::from_millis(1));
        }
    }

    fn delay(&self, inode: u64, step: u64) {
        if self.seed != 0 {
            let micros = (inode * 7 + step * 13 + self.seed * 31) % 1500;
            thread::sleep(Duration::from_micros(micros));
        }
    }
}

impl FaultDir {
    fn read(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        self.faults.call();
        if let Ok(mut reads) = self.faults.reads.lock() {
            *reads.entry(self.inode).or_default() += 1;
        }
        let index = self.reads;
        self.reads = index.saturating_add(1);
        let first = index == 0;
        self.faults.delay(self.inode, u64::from(first));
        let listing = self.faults.listings.get(&self.inode).cloned();
        match listing {
            Some(Listing::Entries(entries)) if first => Ok(entries),
            Some(Listing::FailsAfter(entries, _)) if first && !entries.is_empty() => Ok(entries),
            Some(Listing::FailsAfter(_, error)) => Err(error),
            Some(Listing::Oversized) => Ok((0..8193).map(|_| file("many", 900)).collect()),
            Some(Listing::Batches(batches, error)) => match (batches.get(index), error) {
                (Some(batch), _) => Ok(batch.clone()),
                (None, Some(error)) => Err(error),
                (None, None) => Ok(Vec::new()),
            },
            Some(Listing::Gated(batches)) => {
                if !first {
                    self.faults.wait_for_cancel();
                }
                Ok(batches.get(index).cloned().unwrap_or_default())
            }
            _ => Ok(Vec::new()),
        }
    }

    /// Requests cancellation once the root hands over its first entries, or
    /// once a batch carries disconnection evidence.
    fn returned(&self, entries: &[ObservedEntry]) {
        let trigger = if self.faults.cancel_on_disconnect {
            entries.iter().any(crate::scan::issues::disconnected_entry)
        } else {
            self.inode == ROOT && !entries.is_empty()
        };
        if trigger {
            self.faults.cancel.store(true, SeqCst);
        }
    }
}

impl DirectoryObservations for FaultDir {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        let batch = self.read()?;
        self.returned(&batch);
        Ok(batch)
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
        self.returned(&entries);
        Ok(if entries.is_empty() {
            FilteredBatch::Progress
        } else {
            FilteredBatch::Entries(entries)
        })
    }
}

impl AnchoredDirectory for FaultDir {
    fn open_child(&self, entry: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        self.faults.call();
        let inode = entry.metadata.as_ref().map_err(Clone::clone)?.inode;
        self.faults.delay(inode, 2);
        if let Ok(mut opened) = self.faults.opened.lock() {
            opened.push(inode);
        }
        if let Some(Listing::Refused(error)) = self.faults.listings.get(&inode) {
            return Err(error.clone());
        }
        Ok(Box::new(Self {
            faults: Arc::clone(&self.faults),
            inode,
            reads: 0,
        }))
    }

    fn observed_identity(&self) -> Option<HandleIdentity> {
        if self.inode == ROOT && self.faults.listings.contains_key(&UNVERIFIED) {
            return None;
        }
        Some(HandleIdentity {
            device: 1,
            inode: self.inode,
            ancestors: Vec::new(),
        })
    }
}

pub(super) fn error(kind: PortErrorKind, reason_key: &str) -> PortError {
    PortError {
        kind,
        reason_key: reason_key.into(),
        os_code: None,
    }
}

pub(super) fn denied() -> PortError {
    error(PortErrorKind::PermissionDenied, "os.denied")
}

pub(super) fn disconnected() -> PortError {
    error(PortErrorKind::Io, "scan.disconnected")
}

pub(super) fn observed(name: &str, kind: ObservedKind, device: u64, inode: u64) -> ObservedEntry {
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
    observed(name, ObservedKind::Directory, 1, inode)
}

pub(super) fn file(name: &str, inode: u64) -> ObservedEntry {
    observed(name, ObservedKind::File, 1, inode)
}

pub(super) fn without_metadata(name: &str, inode: u64, error: PortError) -> ObservedEntry {
    let mut entry = file(name, inode);
    entry.metadata = Err(error);
    entry
}
