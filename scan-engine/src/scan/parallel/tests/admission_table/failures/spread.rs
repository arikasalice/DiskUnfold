//! FR12.HD1.app-host.DD1: a disconnection anywhere in a wider tree.
//!
//! For cousins, a folder five levels deep and a folder whose middle batch is
//! cut, every worker count and delay order gives the sequential rows exactly
//! for the folder that reported the disconnection and for its ancestors.
//! Every other folder is either read to its end or left as a gap without
//! contents, and each result counts every item once.
use super::*;
use std::collections::BTreeSet;

/// Delayed parallel runs per fixture.
const REPEATS: u64 = 48;

fn lost(inode: u64) -> ObservedEntry {
    without_metadata("lost", inode, disconnected())
}

/// `p/a` disconnects beside its sibling `p/b` and its cousins `q/c`, `q/d`.
fn cousins() -> HashMap<u64, Listing> {
    HashMap::from([
        (
            ROOT,
            Listing::Entries(vec![dir("p", 160), dir("q", 170), file("r.txt", 180)]),
        ),
        (
            160,
            Listing::Entries(vec![dir("a", 161), dir("b", 162), file("p.txt", 163)]),
        ),
        (
            161,
            Listing::Entries(vec![file("x.txt", 164), lost(165), dir("sub", 166)]),
        ),
        (162, Listing::Entries(vec![file("b.txt", 167)])),
        (166, Listing::Entries(vec![file("never.txt", 168)])),
        (170, Listing::Entries(vec![dir("c", 171), dir("d", 172)])),
        (171, Listing::Entries(vec![file("c.txt", 173)])),
        (172, Listing::Entries(vec![file("d.txt", 174)])),
    ])
}

/// `l1/l2/l3/l4/l5` disconnects; each level also lists a sibling folder.
fn deep() -> HashMap<u64, Listing> {
    let mut listings = HashMap::new();
    let mut parent = ROOT;
    for level in 0..5_u64 {
        let next = 201 + level;
        let sibling = 211 + level;
        let entries = vec![
            dir(&format!("l{}", level + 1), next),
            dir(&format!("s{level}"), sibling),
            file(&format!("f{level}.txt"), 221 + level),
        ];
        listings.insert(parent, Listing::Entries(entries));
        listings.insert(sibling, Listing::Entries(vec![file("s.txt", 241 + level)]));
        parent = next;
    }
    let cut = vec![file("x.txt", 230), lost(231), dir("sub", 232)];
    listings.insert(parent, Listing::Entries(cut));
    listings.insert(232, Listing::Entries(vec![file("never.txt", 233)]));
    listings
}

/// `m` lists three batches; the middle one carries the disconnection.
fn middle_batch() -> HashMap<u64, Listing> {
    let batches = vec![
        vec![file("m1.txt", 311), dir("s1", 312)],
        vec![file("m2.txt", 313), lost(314), dir("s2", 315)],
        vec![file("m3.txt", 316), dir("s3", 317)],
    ];
    HashMap::from([
        (
            ROOT,
            Listing::Entries(vec![dir("m", 310), dir("n", 320), file("top.txt", 330)]),
        ),
        (310, Listing::Batches(batches, None)),
        (312, Listing::Entries(vec![file("s1.txt", 318)])),
        (315, Listing::Entries(vec![file("never.txt", 319)])),
        (317, Listing::Entries(vec![file("never3.txt", 322)])),
        (320, Listing::Entries(vec![file("n.txt", 321)])),
    ])
}

/// `m` returns two batches, then its next read reports the disconnection.
fn read_after_middle_batch() -> HashMap<u64, Listing> {
    let batches = vec![
        vec![file("m1.txt", 311), dir("s1", 312)],
        vec![file("m2.txt", 313), dir("s2", 315)],
    ];
    let mut listings = middle_batch();
    listings.insert(310, Listing::Batches(batches, Some(disconnected())));
    listings
}

/// Directory paths with the names each lists, from the fixture listings.
fn listed(listings: &HashMap<u64, Listing>) -> HashMap<Vec<Vec<u8>>, Vec<Vec<u8>>> {
    let mut tree = HashMap::new();
    let mut pending = vec![(Vec::new(), ROOT)];
    while let Some((path, inode)) = pending.pop() {
        let entries: Vec<ObservedEntry> = match listings.get(&inode) {
            Some(Listing::Entries(entries) | Listing::FailsAfter(entries, _)) => entries.clone(),
            Some(Listing::Batches(batches, _)) => batches.concat(),
            _ => Vec::new(),
        };
        for entry in &entries {
            if let Ok(meta) = &entry.metadata
                && meta.kind == ObservedKind::Directory
            {
                let mut child: Vec<Vec<u8>> = path.clone();
                child.push(entry.name.clone());
                pending.push((child, meta.inode));
            }
        }
        let mut names: Vec<_> = entries.into_iter().map(|entry| entry.name).collect();
        names.sort();
        tree.insert(path, names);
    }
    tree
}

/// Rows owned by folder `at`: its direct entries, its own gap and the gaps
/// of its entries that are not folders.
fn owned(collected: &Collected, at: &[Vec<u8>]) -> (Vec<Item>, Vec<Gap>) {
    let (items, _, gaps) = &collected.run.summary;
    let child = |path: &[Vec<u8>]| path.len() == at.len() + 1 && path.starts_with(at);
    let children: Vec<Item> = items
        .iter()
        .filter(|item| child(&item.1))
        .cloned()
        .collect();
    let folder = |path: &[Vec<u8>]| {
        children
            .iter()
            .any(|item| item.1 == path && item.2 == ResultItemKind::Directory)
    };
    let gaps = gaps
        .iter()
        .filter(|gap| gap.1 == at || (child(&gap.1) && !folder(&gap.1)))
        .cloned()
        .collect();
    (children, gaps)
}

/// Every folder outside `exact` is read to its end or is a gap without contents.
fn read_or_gap(
    collected: &Collected,
    exact: &[Vec<Vec<u8>>],
    tree: &HashMap<Vec<Vec<u8>>, Vec<Vec<u8>>>,
    label: &str,
) {
    let (items, _, gaps) = &collected.run.summary;
    let gapped: BTreeSet<&Vec<Vec<u8>>> = gaps.iter().map(|gap| &gap.1).collect();
    let folders = items
        .iter()
        .filter(|item| item.2 == ResultItemKind::Directory && !exact.contains(&item.1));
    for folder in folders {
        let at = &folder.1;
        let mut names: Vec<Vec<u8>> = items
            .iter()
            .filter(|item| item.1.len() == at.len() + 1 && item.1.starts_with(at))
            .filter_map(|item| item.1.last().cloned())
            .collect();
        names.sort();
        let below = items
            .iter()
            .any(|item| item.1.len() > at.len() && item.1.starts_with(at));
        if gapped.contains(at) {
            assert!(!below, "{label}: gap {at:?} has contents");
        } else {
            let expected = tree.get(at).cloned().unwrap_or_default();
            assert_eq!(names, expected, "{label}: {at:?} not read to its end");
        }
    }
}

/// Checks every delayed parallel run against the sequential baseline.
///
/// `never` is a folder returned in the batch that reported the disconnection,
/// which no collector may open.
fn spread(
    listings: fn() -> HashMap<u64, Listing>,
    cut: &[&str],
    never: Option<u64>,
) -> Result<(), String> {
    let cut = path(cut);
    let exact: Vec<Vec<Vec<u8>>> = (0..=cut.len()).map(|len| cut[..len].to_vec()).collect();
    let tree = listed(&listings());
    let baseline = sequential(listings(), false)?;
    assert_counted_once(&baseline, "sequential");
    read_or_gap(&baseline, &exact, &tree, "sequential");
    let gapped = |collected: &Collected, at: &Vec<Vec<u8>>| {
        collected.run.summary.2.iter().any(|gap| &gap.1 == at)
    };
    assert!(gapped(&baseline, &cut), "the cut folder is a gap");
    for ancestor in &exact[..cut.len()] {
        assert!(
            !gapped(&baseline, ancestor),
            "ancestor {ancestor:?} is read"
        );
    }
    for run in 0..REPEATS {
        let workers = WORKERS[run as usize % WORKERS.len()];
        let filtered = run % 3 == 1;
        let label = format!("run={run} workers={workers} filtered={filtered}");
        let (collected, faults) = parallel_seeded(listings(), (workers, run + 1), filtered, false)?;
        assert_counted_once(&collected, &label);
        for at in &exact {
            assert_eq!(
                owned(&collected, at),
                owned(&baseline, at),
                "{label}: {at:?}"
            );
        }
        read_or_gap(&collected, &exact, &tree, &label);
        if let Some(inode) = never {
            assert!(
                !faults.opened(inode),
                "{label}: opened a folder of the cut batch"
            );
        }
        assert_eq!(
            collected.run.completeness,
            ResultCompleteness::Missing,
            "{label}"
        );
        assert!(collected.run.outcome.completed_scope, "{label}");
    }
    Ok(())
}

#[test]
fn cousins_of_a_disconnected_folder_are_read_or_left_as_gaps() -> Result<(), String> {
    let _guard = worker_test_lock();
    spread(cousins, &["p", "a"], Some(166))
}

#[test]
fn a_disconnection_five_levels_deep_keeps_every_ancestor_exact() -> Result<(), String> {
    let _guard = worker_test_lock();
    spread(deep, &["l1", "l2", "l3", "l4", "l5"], Some(232))
}

#[test]
fn a_middle_batch_disconnection_keeps_the_earlier_batches() -> Result<(), String> {
    let _guard = worker_test_lock();
    spread(middle_batch, &["m"], Some(315))?;
    // The batch after the disconnection is never read.
    let (collected, faults) = parallel_seeded(middle_batch(), (4, 9), false, false)?;
    let names: Vec<_> = collected
        .run
        .summary
        .0
        .iter()
        .map(|item| item.1.clone())
        .collect();
    assert!(!names.contains(&path(&["m", "m3.txt"])));
    assert!(!faults.opened(317));
    Ok(())
}

#[test]
fn a_read_failing_after_the_middle_batch_keeps_both_batches() -> Result<(), String> {
    let _guard = worker_test_lock();
    // Both batches were returned without disconnection evidence, so their
    // folders may be opened before the failing read.
    spread(read_after_middle_batch, &["m"], None)
}
