//! FR12.HD1.app-host.DD1: how unentered ranges set completeness and failures.
//!
//! Another volume, a mount point, a mounted file and a revisit are listed
//! only; the result stays complete. An unverifiable range is listed, counted
//! as a failure and kept as a scope gap, so the result has missing ranges.
//! Each case runs through the sequential collector and through every worker
//! count.
use super::*;

/// A verified root with another volume, a mount point, a mounted file and a
/// same-volume file.
fn boundaries() -> Listings {
    use ObservedKind::{Directory as D, File as F, MountPoint as M, MountedFile as MF};
    BTreeMap::from([
        (
            ROOT,
            vec![
                node("a", D, 1, 101),
                node("mnt", D, 2, MOUNT),
                node("foreign.bin", F, 2, 301),
                node("vol", M, 3, MOUNTED_ROOT),
                node("hosts", MF, 4, 501),
                node("top.txt", F, 1, 205),
            ],
        ),
        (101, vec![node("deep.txt", F, 1, 201)]),
        (MOUNT, vec![node("other.txt", F, 2, 302)]),
        (MOUNTED_ROOT, vec![node("mounted.txt", F, 3, 402)]),
        (COVERED, vec![node("covered.txt", F, 1, 403)]),
    ])
}

/// An alias pair and a cycle back to the root, on one volume.
fn revisits() -> Listings {
    use ObservedKind::{Directory as D, File as F};
    BTreeMap::from([
        (
            ROOT,
            vec![
                node("a", D, 1, 101),
                node("twin", D, 1, 101),
                node("top.txt", F, 1, 205),
            ],
        ),
        (
            101,
            vec![node("deep.txt", F, 1, 201), node("loop", D, 1, ROOT)],
        ),
    ])
}

/// btrfs subvolumes with the IDs 256, 257 and 258; every subvolume root
/// reports inode 256.
fn subvolumes() -> Listings {
    use ObservedKind::{Directory as D, File as F};
    BTreeMap::from([
        (
            ROOT,
            vec![
                node("a", D, 1, 101),
                subvolume("first", 5, 256),
                subvolume("second", 6, 257),
                subvolume("third", 7, 258),
                node("top.txt", F, 1, 205),
            ],
        ),
        (101, vec![node("deep.txt", F, 1, 201)]),
        (SUBVOLUME_ROOT, vec![node("inside.txt", F, 5, 601)]),
    ])
}

/// A child directory, a file and a name that the exclusion cannot decide.
fn undecidable() -> Listings {
    use ObservedKind::{Directory as D, File as F};
    BTreeMap::from([
        (
            ROOT,
            vec![
                node("a", D, 1, 101),
                node("odd", D, 1, ODD),
                node("top.txt", F, 1, 205),
            ],
        ),
        (101, vec![node("deep.txt", F, 1, 201)]),
        (ODD, vec![node("odd.txt", F, 1, 207)]),
    ])
}

/// Collects one root with the sequential collector and no exclusion.
fn sequential(tree: &Arc<Tree>, identity: bool) -> Result<Run, String> {
    let ids = Ids(AtomicUsize::new(1));
    let sink = IngestSink::default();
    let root = Dir::root(tree, identity);
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

fn sorted(mut issues: Vec<Issue>) -> Vec<Issue> {
    issues.sort_by(|a, b| (a.0, &a.1, a.2).cmp(&(b.0, &b.1, b.2)));
    issues
}

/// Checks completeness, the failure count, the listed rows and the gaps.
fn check(run: &Run, complete: bool, failed: u64, issues: &[Issue], gaps: &[Gap]) {
    let expected = if complete {
        ResultCompleteness::Complete
    } else {
        ResultCompleteness::Missing
    };
    assert_eq!(run.completeness, expected);
    assert!(run.outcome.completed_scope);
    assert_eq!(run.outcome.counts.failed, failed);
    assert_eq!(run.summary.1, issues);
    assert_eq!(run.summary.2, gaps);
}

#[test]
fn other_volumes_and_mount_points_keep_the_result_complete() -> Result<(), String> {
    let _guard = worker_test_lock();
    let issues = sorted(vec![
        other_volume(0, &["foreign.bin"]),
        other_volume(0, &["hosts"]),
        other_volume(0, &["mnt"]),
        other_volume(0, &["vol"]),
    ]);
    let baseline = sequential(&tree_of(boundaries(), 0), true)?;
    check(&baseline, true, 0, &issues, &[]);
    let items = &baseline.summary.0;
    // The mount point stays a directory item without the mounted volume's size.
    assert!(items.contains(&(0, path(&["vol"]), ResultItemKind::Directory, false)));
    assert!(items.contains(&(0, path(&["mnt"]), ResultItemKind::Directory, false)));
    assert!(items.contains(&(0, path(&["foreign.bin"]), ResultItemKind::Unknown, false)));
    // The mounted file stays a file item without the mounted file's size.
    assert!(items.contains(&(0, path(&["hosts"]), ResultItemKind::File, false)));
    for (workers, seed) in RUNS {
        let tree = tree_of(boundaries(), seed);
        let run = filtered_run(&tree, workers, true)?;
        check(&run, true, 0, &issues, &[]);
        assert_eq!(
            run.summary, baseline.summary,
            "workers={workers} seed={seed}"
        );
        for inode in [MOUNT, MOUNTED_ROOT, COVERED] {
            assert_eq!(tree.enumerated(inode), 0, "workers={workers} seed={seed}");
        }
        assert!(!tree.probed("mounted.txt") && !tree.probed("covered.txt"));
    }
    Ok(())
}

/// Each subvolume is one boundary row whatever ID enumeration lists for it:
/// no failure, no gap, a complete result and nothing below it read.
#[test]
fn btrfs_subvolumes_stay_outside_whatever_inode_their_parent_lists() -> Result<(), String> {
    let _guard = worker_test_lock();
    let names = ["first", "second", "third"];
    let issues = sorted(
        names
            .into_iter()
            .map(|name| other_volume(0, &[name]))
            .collect(),
    );
    let baseline = sequential(&tree_of(subvolumes(), 0), true)?;
    check(&baseline, true, 0, &issues, &[]);
    for name in names {
        let item = (0, path(&[name]), ResultItemKind::Directory, false);
        assert!(baseline.summary.0.contains(&item), "{name}");
    }
    for (workers, seed) in RUNS {
        let tree = tree_of(subvolumes(), seed);
        let run = filtered_run(&tree, workers, true)?;
        check(&run, true, 0, &issues, &[]);
        assert_eq!(
            run.summary, baseline.summary,
            "workers={workers} seed={seed}"
        );
        assert_eq!(
            tree.enumerated(SUBVOLUME_ROOT),
            0,
            "workers={workers} seed={seed}"
        );
        assert!(!tree.probed("inside.txt"));
    }
    Ok(())
}

#[test]
fn revisits_and_a_repeated_root_keep_the_result_complete() -> Result<(), String> {
    let _guard = worker_test_lock();
    let single = sorted(vec![revisit(0, &["a", "loop"]), revisit(0, &["twin"])]);
    let baseline = sequential(&tree_of(revisits(), 0), true)?;
    check(&baseline, true, 0, &single, &[]);
    let mut repeated = single.clone();
    repeated.push(revisit(1, &[]));
    let repeated = sorted(repeated);
    for (workers, seed) in RUNS {
        let tree = tree_of(revisits(), seed);
        let run = filtered_run(&tree, workers, true)?;
        check(&run, true, 0, &single, &[]);
        assert_eq!(
            run.summary, baseline.summary,
            "workers={workers} seed={seed}"
        );
        let tree = tree_of(revisits(), seed);
        let roots = vec![Dir::root(&tree, true), Dir::root(&tree, true)];
        let run = filtered_forest(roots, workers)?;
        check(&run, true, 0, &repeated, &[]);
        assert_eq!(tree.enumerated(ROOT), 1, "workers={workers} seed={seed}");
        assert_eq!(tree.enumerated(101), 1, "workers={workers} seed={seed}");
    }
    Ok(())
}

#[test]
fn unverifiable_ranges_are_gaps_and_failures() -> Result<(), String> {
    let _guard = worker_test_lock();
    // A root without an identity: neither it nor its child directory is entered.
    let unidentified = [unverifiable(0, &[]), unverifiable(0, &["a"])];
    let unidentified_gaps = [unverifiable_gap(0, &[]), unverifiable_gap(0, &["a"])];
    let mut issues = unidentified.to_vec();
    issues.push(unverifiable(0, &["odd"]));
    let mut gaps = unidentified_gaps.to_vec();
    gaps.push(unverifiable_gap(0, &["odd"]));
    // Without exclusion rules, the sequential collector cannot enter odd either.
    let baseline = sequential(&tree_of(undecidable(), 0), false)?;
    check(&baseline, false, 3, &issues, &gaps);
    let mut first = None;
    for (workers, seed) in RUNS {
        // A verified root whose exclusion rule cannot decide one name.
        let tree = tree_of(undecidable(), seed);
        let run = filtered_run(&tree, workers, true)?;
        let withheld = [unverifiable(0, &["odd"])];
        check(&run, false, 1, &withheld, &[unverifiable_gap(0, &["odd"])]);
        assert_eq!(tree.enumerated(ODD), 0, "workers={workers} seed={seed}");
        let summary = first.get_or_insert_with(|| run.summary.clone());
        assert_eq!(&run.summary, summary, "workers={workers} seed={seed}");
        let tree = tree_of(undecidable(), seed);
        let run = filtered_run(&tree, workers, false)?;
        check(&run, false, 3, &issues, &gaps);
        assert_eq!(tree.enumerated(101), 0, "workers={workers} seed={seed}");
    }
    Ok(())
}
