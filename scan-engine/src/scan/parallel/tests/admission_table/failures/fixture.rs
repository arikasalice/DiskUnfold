//! Fixture directories that refuse opens, fail reads and break metadata.
use super::*;

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
}

pub(super) struct Faults {
    listings: HashMap<u64, Listing>,
    /// Set once the root returns its first batch.
    pub(super) cancel: Arc<AtomicBool>,
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
        let faults = Arc::new(Faults {
            listings,
            cancel: Arc::new(AtomicBool::new(false)),
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

    fn delay(&self, inode: u64, step: u64) {
        if self.seed != 0 {
            let micros = (inode * 7 + step * 13 + self.seed * 31) % 1500;
            thread::sleep(Duration::from_micros(micros));
        }
    }
}

impl FaultDir {
    fn read(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        let first = std::mem::replace(&mut self.reads, 1) == 0;
        self.faults.delay(self.inode, u64::from(first));
        let listing = self.faults.listings.get(&self.inode).cloned();
        match listing {
            Some(Listing::Entries(entries)) if first => Ok(entries),
            Some(Listing::FailsAfter(entries, _)) if first && !entries.is_empty() => Ok(entries),
            Some(Listing::FailsAfter(_, error)) => Err(error),
            Some(Listing::Oversized) => Ok((0..8193).map(|_| file("many", 900)).collect()),
            _ => Ok(Vec::new()),
        }
    }

    /// Requests cancellation once the root hands over its first entries.
    fn returned(&self, entries: usize) {
        if self.inode == ROOT && entries > 0 {
            self.faults.cancel.store(true, SeqCst);
        }
    }
}

impl DirectoryObservations for FaultDir {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        let batch = self.read()?;
        self.returned(batch.len());
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
        self.returned(entries.len());
        Ok(if entries.is_empty() {
            FilteredBatch::Progress
        } else {
            FilteredBatch::Entries(entries)
        })
    }
}

impl AnchoredDirectory for FaultDir {
    fn open_child(&self, entry: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
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
            allocated: None,
            link_count: None,
            space_identity: None,
            modified_at: None,
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
