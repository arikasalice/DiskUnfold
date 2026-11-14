//! FR1.HD1.app-host.DD1 and FR12.HD1.app-host.DD1: a failed open keeps its reservation.
//!
//! The enter row reserves the volume and file identity of a directory when it
//! is admitted. If opening that directory fails later, the failure is listed
//! at the open-directory stage and the identity stays reserved, so another
//! path to the same directory found after the failure is a revisit and is not
//! opened either.
//!
//! The parallel collector also keeps the refused range as a scope gap. The
//! sequential collector lists the same failure without a gap, so only items
//! and listed failures are compared with it.
use super::*;
use std::{
    sync::atomic::{AtomicBool, Ordering::SeqCst},
    time::Instant,
};

/// The directory whose open is refused.
const REFUSED: u64 = 101;
/// A directory that lists a second path to [`REFUSED`].
const DETOUR: u64 = 105;

/// `a` is refused; `c/again` is another path to the same directory.
fn aliased() -> Listings {
    use ObservedKind::{Directory as D, File as F};
    BTreeMap::from([
        (
            ROOT,
            vec![
                node("a", D, 1, REFUSED),
                node("c", D, 1, DETOUR),
                node("top.txt", F, 1, 205),
            ],
        ),
        (REFUSED, vec![node("deep.txt", F, 1, 201)]),
        (DETOUR, vec![node("again", D, 1, REFUSED)]),
    ])
}

#[derive(Default)]
struct Refusals {
    opens: AtomicUsize,
    /// Set when the detour was listed before the refused open happened.
    late: AtomicBool,
}

/// Wraps the fixture directory, refuses to open [`REFUSED`] and holds the
/// listing of [`DETOUR`] until that refusal has happened.
struct Refusing {
    inner: Box<dyn AnchoredDirectory>,
    inode: u64,
    refusals: Arc<Refusals>,
}

impl Refusing {
    fn root(tree: &Arc<Tree>, refusals: &Arc<Refusals>) -> Box<dyn AnchoredDirectory> {
        Box::new(Self {
            inner: Dir::root(tree, true),
            inode: ROOT,
            refusals: Arc::clone(refusals),
        })
    }

    fn after_refusal(&self) {
        if self.inode != DETOUR {
            return;
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        while self.refusals.opens.load(SeqCst) == 0 {
            if Instant::now() >= deadline {
                self.refusals.late.store(true, SeqCst);
                return;
            }
            thread::sleep(Duration::from_millis(1));
        }
    }
}

impl DirectoryObservations for Refusing {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        self.after_refusal();
        self.inner.next_batch()
    }

    fn next_filtered(
        &mut self,
        include: &mut dyn FnMut(&[u8]) -> FilterDecision,
    ) -> Result<FilteredBatch, PortError> {
        self.after_refusal();
        self.inner.next_filtered(include)
    }
}

impl AnchoredDirectory for Refusing {
    fn open_child(&self, entry: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        let inode = entry.metadata.as_ref().map_or(0, |meta| meta.inode);
        if inode == REFUSED {
            self.refusals.opens.fetch_add(1, SeqCst);
            return Err(PortError {
                kind: PortErrorKind::PermissionDenied,
                reason_key: "os.denied".into(),
                os_code: None,
            });
        }
        Ok(Box::new(Self {
            inner: self.inner.open_child(entry)?,
            inode,
            refusals: Arc::clone(&self.refusals),
        }))
    }

    fn observed_identity(&self) -> Option<HandleIdentity> {
        self.inner.observed_identity()
    }
}

fn sequential(root: Box<dyn AnchoredDirectory>) -> Result<Run, String> {
    let ids = Ids(AtomicUsize::new(1));
    let sink = IngestSink::default();
    let outcome = crate::scan::collect(
        &sink,
        ids.next_id(),
        root,
        &ids,
        || Checkpoint::Continue,
        |_| true,
    );
    let version = sink.pin().map_err(|e| format!("{e:?}"))?.ok_or("missing")?;
    Ok(Run {
        outcome,
        summary: summary(&version),
        completeness: version.completeness(),
    })
}

fn check(run: &Run, tree: &Tree, refusals: &Refusals, label: &str) {
    let mut issues = run.summary.1.clone();
    issues.sort();
    let open_failed = (
        0,
        path(&["a"]),
        Kind::UnreadRange,
        Stage::OpenDirectory,
        "scan.path_denied".to_string(),
    );
    assert_eq!(
        issues,
        [open_failed, revisit(0, &["c", "again"])],
        "{label}"
    );
    assert!(run.outcome.completed_scope, "{label}");
    assert_eq!(run.outcome.counts.failed, 1, "{label}");
    // Only the first path was opened; the alias found later was not tried.
    assert_eq!(refusals.opens.load(SeqCst), 1, "{label}");
    assert!(!refusals.late.load(SeqCst), "{label}: detour listed first");
    assert_eq!(tree.enumerated(REFUSED), 0, "{label}");
    assert_eq!(tree.enumerated(DETOUR), 1, "{label}");
    let items = &run.summary.0;
    assert!(
        items
            .iter()
            .any(|item| item.1 == path(&["a"]) && item.2 == ResultItemKind::Directory),
        "{label}"
    );
    assert!(
        !items
            .iter()
            .any(|item| item.1.len() > 1 && item.1[0] == b"a")
    );
}

#[test]
fn a_failed_open_keeps_the_admitted_identity_reserved() -> Result<(), String> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "lock")?;
    let tree = tree_of(aliased(), 0);
    let refusals = Arc::new(Refusals::default());
    let baseline = sequential(Refusing::root(&tree, &refusals))?;
    check(&baseline, &tree, &refusals, "sequential");
    let gaps = [(0, path(&["a"]), "scan.path_denied".to_string())];
    for (workers, seed) in RUNS {
        let label = format!("workers={workers} seed={seed}");
        let tree = tree_of(aliased(), seed);
        let refusals = Arc::new(Refusals::default());
        let run = filtered_forest(vec![Refusing::root(&tree, &refusals)], workers)?;
        check(&run, &tree, &refusals, &label);
        assert_eq!(run.summary.0, baseline.summary.0, "{label}");
        assert_eq!(run.summary.1, baseline.summary.1, "{label}");
        assert_eq!(run.summary.2, gaps, "{label}");
        assert_eq!(run.completeness, ResultCompleteness::Missing, "{label}");
    }
    Ok(())
}
