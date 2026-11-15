//! Folders a cancellation leaves unopened or partly read are marked once.
use super::fixture::*;
use super::*;
use du_analysis_index::folder_collection::ExcludedChildren;

const A: u64 = 30;
const B: u64 = 31;
const DENIED: u64 = 32;
const BROKEN: u64 = 33;
const MOUNT: u64 = 34;
const B1: u64 = 40;
const LATE: u64 = 38;

/// A tree with a refused folder, a folder whose read fails, a folder on
/// another volume and an excluded file below the root.
fn nodes() -> HashMap<u64, Node> {
    let root = vec![
        vec![
            dir("a", A),
            dir("b", B),
            dir("denied", DENIED),
            dir("broken", BROKEN),
            entry("mnt", ObservedKind::Directory, 2, MOUNT),
            file("x1", 35),
            file("f", 36),
        ],
        vec![file("g", 37), dir("late", LATE)],
    ];
    HashMap::from([
        (ROOT, Node::Batches(root)),
        (A, Node::Batches(vec![vec![file("a1", 301)]])),
        (
            B,
            Node::Batches(vec![vec![dir("b1", B1)], vec![file("b2", 311)]]),
        ),
        (B1, Node::Batches(vec![vec![file("z", 401)]])),
        (DENIED, Node::Refused),
        (BROKEN, Node::FailsAfter(vec![vec![file("k", 331)]])),
        (MOUNT, Node::Batches(vec![vec![file("m", 341)]])),
        (LATE, Node::Batches(vec![vec![file("l", 381)]])),
    ])
}

fn inode(path: &[Vec<u8>]) -> Option<u64> {
    let parts: Vec<&[u8]> = path.iter().map(Vec::as_slice).collect();
    Some(match parts.as_slice() {
        [] => ROOT,
        [b"a"] => A,
        [b"b"] => B,
        [b"b", b"b1"] => B1,
        [b"denied"] => DENIED,
        [b"broken"] => BROKEN,
        [b"mnt"] => MOUNT,
        [b"late"] => LATE,
        _ => return None,
    })
}

const TRIGGERS: [Trigger; 8] = [
    Trigger::Read(ROOT, 0),
    Trigger::Read(ROOT, 1),
    Trigger::Read(B, 0),
    Trigger::Read(B, 1),
    Trigger::Read(B1, 0),
    Trigger::Open(B1),
    Trigger::Open(LATE),
    Trigger::Read(BROKEN, 0),
];

/// A folder is marked exactly when the collector entered it and did not read
/// it to its end, and its open or read did not fail; a failure is a gap.
/// `root_listed` is false when the root's first batch was never returned.
fn check(
    tree: &Tree,
    version: &ResultVersion,
    root_listed: bool,
    label: &str,
) -> Result<(), String> {
    let marked = cancelled(version)?;
    // A folder with a gap and a mark would be counted here but shown as the gap.
    assert_eq!(
        version.unread_folder_counts().cancelled as usize,
        marked.len(),
        "{label}"
    );
    let log = tree.log.lock().map_err(|_| "log")?;
    for path in folders(version).into_keys() {
        let inode = inode(&path).ok_or_else(|| format!("{label}: unknown {path:?}"))?;
        let entered = inode != MOUNT;
        let failed =
            log.failed.contains(&inode) || (inode == DENIED && log.opened.contains(&inode));
        let expected = entered && !failed && !log.ended.contains(&inode);
        assert_eq!(marked.contains(&path), expected, "{label}: {path:?}");
        // Only the root excluded an entry; the others report nothing.
        let wanted = (inode == ROOT && root_listed).then_some(ExcludedChildren {
            count: 1,
            exact: log.ended.contains(&ROOT),
        });
        let parts: Vec<&str> = path
            .iter()
            .map(|part| std::str::from_utf8(part).unwrap_or(""))
            .collect();
        assert_eq!(excluded(version, &parts)?, wanted, "{label}: {path:?}");
    }
    assert!(!log.opened.contains(&MOUNT), "{label}");
    Ok(())
}

#[test]
fn a_scan_that_is_not_cancelled_leaves_no_mark() -> Result<(), String> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "lock")?;
    for collector in COLLECTORS {
        let (outcome, version) = collect(&Tree::new(nodes(), Trigger::Never), collector)?;
        assert!(outcome.completed_scope, "{collector:?}");
        assert_eq!(version.completeness(), ResultCompleteness::Missing);
        assert_eq!(version.unread_folder_counts().cancelled, 0);
        assert!(cancelled(&version)?.is_empty(), "{collector:?}");
    }
    Ok(())
}

#[test]
fn a_cancelled_scan_marks_exactly_the_folders_not_read_to_their_end() -> Result<(), String> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "lock")?;
    for collector in COLLECTORS {
        for trigger in TRIGGERS {
            let tree = Tree::new(nodes(), trigger);
            let (outcome, version) = collect(&tree, collector)?;
            let label = format!("{collector:?} trigger={}", describe(trigger));
            assert!(!outcome.completed_scope, "{label}");
            assert_eq!(
                version.completeness(),
                ResultCompleteness::Cancelled,
                "{label}"
            );
            // A filtered read that sees the cancellation returns none of its entries.
            let filtered = matches!(collector, Collector::Filtered(_));
            let root_listed = !(filtered && trigger == Trigger::Read(ROOT, 0));
            check(&tree, &version, root_listed, &label)?;
        }
    }
    Ok(())
}

fn describe(trigger: Trigger) -> String {
    match trigger {
        Trigger::Never => "never".into(),
        Trigger::Read(inode, index) => format!("read({inode},{index})"),
        Trigger::Open(inode) => format!("open({inode})"),
    }
}

/// With one worker, each kind of folder a cancellation leaves unread is met:
/// queued and never opened, opened and never read, in the middle of its
/// listing, and recorded from a returned batch while draining.
#[test]
fn each_kind_of_unread_folder_is_marked_once() -> Result<(), String> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "lock")?;
    type Marked = HashSet<Vec<Vec<u8>>>;
    let run = |trigger| -> Result<(Marked, Arc<Tree>), String> {
        let tree = Tree::new(nodes(), trigger);
        let (_, version) = collect(&tree, Collector::Legacy(1))?;
        check(&tree, &version, true, &describe(trigger))?;
        Ok((cancelled(&version)?, tree))
    };
    // The root's first entries were all scheduled before its second read.
    let (marked, tree) = run(Trigger::Read(ROOT, 1))?;
    let queued = ["a", "b", "denied", "broken"]
        .into_iter()
        .filter(|name| marked.contains(&path(&[name])))
        .count();
    assert!(queued > 0, "no queued folder was left: {marked:?}");
    // `late` comes from the root's second batch, drained after cancellation.
    assert!(marked.contains(&path(&["late"])));
    assert!(!tree.with_log(|log| log.opened.contains(&LATE))?);
    // Opening b1 returned it to its lane unread.
    let (marked, tree) = run(Trigger::Open(B1))?;
    assert!(marked.contains(&path(&["b", "b1"])));
    assert_eq!(tree.with_log(|log| log.reads.get(&B1).copied())?, None);
    // b had returned its first batch and not its end.
    let (marked, tree) = run(Trigger::Read(B, 0))?;
    assert!(marked.contains(&path(&["b"])));
    assert_eq!(tree.with_log(|log| log.reads.get(&B).copied())?, Some(1));
    Ok(())
}

/// Cancelling during the root's first read: its returned folders are
/// recorded while it is drained and marked with it, without being opened.
#[test]
fn folders_recorded_while_draining_are_marked_with_their_parent() -> Result<(), String> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "lock")?;
    let collectors = [
        Collector::Legacy(1),
        Collector::Legacy(4),
        Collector::Sequential,
    ];
    for collector in collectors {
        let tree = Tree::new(nodes(), Trigger::Read(ROOT, 0));
        let (_, version) = collect(&tree, collector)?;
        let expected: HashSet<_> = [
            path(&[]),
            path(&["a"]),
            path(&["b"]),
            path(&["denied"]),
            path(&["broken"]),
        ]
        .into();
        assert_eq!(cancelled(&version)?, expected, "{collector:?}");
        // Nothing was opened, so the folders whose open or read would fail
        // are marked like the others; the other volume's folder is not.
        assert!(tree.with_log(|log| log.opened.is_empty())?, "{collector:?}");
    }
    Ok(())
}

/// The sequential collector stops inside `b/b1`: the folders on its stack
/// are marked, with the root's recorded folders it had not opened yet, and
/// `a`, read to its end before, is not.
#[test]
fn sequential_marks_the_folders_on_its_stack() -> Result<(), String> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "lock")?;
    for trigger in [Trigger::Read(B1, 0), Trigger::Open(B1)] {
        let tree = Tree::new(nodes(), trigger);
        let (_, version) = collect(&tree, Collector::Sequential)?;
        let expected: HashSet<_> = [
            path(&[]),
            path(&["b"]),
            path(&["b", "b1"]),
            path(&["denied"]),
            path(&["broken"]),
        ]
        .into();
        assert_eq!(cancelled(&version)?, expected, "{}", describe(trigger));
        assert_eq!(
            excluded(&version, &[])?,
            Some(ExcludedChildren {
                count: 1,
                exact: false
            })
        );
        assert_eq!(excluded(&version, &["a"])?, None);
    }
    Ok(())
}

/// A root whose identity cannot be verified already has a gap; cancelling it
/// leaves no mark, and none of its folders is entered.
#[test]
fn an_unverifiable_root_is_not_marked() -> Result<(), String> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "lock")?;
    for collector in COLLECTORS {
        for trigger in [Trigger::Read(ROOT, 0), Trigger::Read(ROOT, 1)] {
            let tree = Tree::unverified(nodes(), trigger);
            let (_, version) = collect(&tree, collector)?;
            let label = format!("{collector:?} {}", describe(trigger));
            assert_eq!(
                version.completeness(),
                ResultCompleteness::Cancelled,
                "{label}"
            );
            assert!(cancelled(&version)?.is_empty(), "{label}");
            assert_eq!(version.unread_folder_counts().cancelled, 0, "{label}");
        }
    }
    Ok(())
}

/// A scan cancelled before any read marks its root and nothing else.
#[test]
fn a_scan_cancelled_before_it_starts_marks_its_root() -> Result<(), String> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "lock")?;
    for collector in COLLECTORS {
        let tree = Tree::new(nodes(), Trigger::Never);
        tree.cancel.store(true, Ordering::SeqCst);
        let (_, version) = collect(&tree, collector)?;
        assert_eq!(version.completeness(), ResultCompleteness::Cancelled);
        assert_eq!(cancelled(&version)?, HashSet::from([path(&[])]));
        assert!(tree.with_log(|log| log.reads.is_empty())?, "{collector:?}");
    }
    Ok(())
}

/// Cancellation while `c` reports a disconnection: `c` and its folder `d`
/// keep their disconnection gaps, and no folder has both a gap and a mark.
#[test]
fn a_disconnection_gap_is_never_also_marked() -> Result<(), String> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "lock")?;
    let mut lost = file("lost", 502);
    lost.metadata = Err(PortError {
        kind: PortErrorKind::Io,
        reason_key: "scan.disconnected".into(),
        os_code: None,
    });
    // A filtered read that sees the cancellation returns nothing, so the
    // disconnection is never observed there; the legacy reads return it.
    let collectors = [
        Collector::Legacy(1),
        Collector::Legacy(4),
        Collector::Sequential,
    ];
    for collector in collectors {
        let root = vec![vec![dir("s", 50), dir("c", 51)], vec![file("y", 503)]];
        let siblings = vec![vec![file("s1", 511)], vec![file("s2", 512)]];
        let tree = Tree::new(
            HashMap::from([
                (ROOT, Node::Batches(root)),
                (50, Node::Batches(siblings)),
                (51, Node::Batches(vec![vec![lost.clone(), dir("d", 52)]])),
                (52, Node::Batches(vec![vec![file("never", 521)]])),
            ]),
            Trigger::Read(51, 0),
        );
        let (_, version) = collect(&tree, collector)?;
        let label = format!("{collector:?}");
        let unread = unread(&version)?;
        for gap in [path(&["c"]), path(&["c", "d"])] {
            assert_eq!(
                unread.get(&gap).map(String::as_str),
                Some("scan.disconnected"),
                "{label}: {gap:?}"
            );
        }
        let marked = cancelled(&version)?;
        assert_eq!(
            version.unread_folder_counts().cancelled as usize,
            marked.len(),
            "{label}"
        );
        assert!(!tree.with_log(|log| log.opened.contains(&52))?, "{label}");
    }
    Ok(())
}
