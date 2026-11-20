//! Nested, repeated and aliased directories across workers and delay orders.
use super::*;

/// The outer root and a root nested at its child `a` (inode 101).
fn nested(tree: &Arc<Tree>, inner_first: bool) -> Vec<Box<dyn AnchoredDirectory>> {
    let outer = Dir::at(tree, ROOT, Some(Vec::new()));
    let inner = Dir::at(tree, 101, Some(vec![ROOT]));
    if inner_first {
        vec![inner, outer]
    } else {
        vec![outer, inner]
    }
}

fn swap_roots(summary: Summary) -> Summary {
    let swap = |root: u32| 1 - root;
    let (items, issues, gaps) = summary;
    let mut items: Vec<Item> = items
        .into_iter()
        .map(|(root, path, kind, size)| (swap(root), path, kind, size))
        .collect();
    items.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
    let mut issues: Vec<Issue> = issues
        .into_iter()
        .map(|(root, path, kind, stage, key)| (swap(root), path, kind, stage, key))
        .collect();
    issues.sort();
    let mut gaps: Vec<Gap> = gaps
        .into_iter()
        .map(|(root, path, key)| (swap(root), path, key))
        .collect();
    gaps.sort();
    (items, issues, gaps)
}

#[test]
fn a_nested_root_is_collected_only_by_itself_in_any_order_and_timing() -> Result<(), String> {
    let _guard = worker_test_lock();
    let baseline = filtered_forest(nested(&tree(0), false), 1)?.summary;
    let (items, issues, _) = &baseline;
    // The outer root reaches the inner root and leaves it as a revisit.
    assert!(items.contains(&(0, path(&["a"]), ResultItemKind::Directory, false)));
    assert!(issues.contains(&revisit(0, &["a"])));
    assert!(
        !items
            .iter()
            .any(|item| item.0 == 0 && item.1.len() > 1 && item.1[0] == b"a")
    );
    // The inner root collects its own entries; its path back up is a revisit.
    assert!(items.contains(&(1, path(&["deep.txt"]), ResultItemKind::File, true)));
    assert!(issues.contains(&revisit(1, &["loop"])));
    assert!(items.contains(&(0, path(&["b", "b.txt"]), ResultItemKind::File, true)));
    for inner_first in [false, true] {
        for (workers, seed) in RUNS {
            let tree = tree(seed);
            let run = filtered_forest(nested(&tree, inner_first), workers)?;
            let summary = if inner_first {
                swap_roots(run.summary)
            } else {
                run.summary
            };
            assert_eq!(
                summary, baseline,
                "inner_first={inner_first} workers={workers} seed={seed}"
            );
            assert_eq!(tree.enumerated(ROOT), 1);
            assert_eq!(tree.enumerated(101), 1);
            assert_eq!(tree.enumerated(102), 1);
        }
    }
    Ok(())
}

#[test]
fn an_ancestor_of_a_nested_root_stays_collectable_by_the_outer_root() -> Result<(), String> {
    let _guard = worker_test_lock();
    // The nested root lies at b; its verified ancestors include b's parent and
    // an unrelated directory 101 that only the outer root may enter.
    let roots = |tree: &Arc<Tree>| {
        vec![
            Dir::at(tree, ROOT, Some(Vec::new())),
            Dir::at(tree, 102, Some(vec![ROOT, 101])),
        ]
    };
    for (workers, seed) in RUNS {
        let tree = tree(seed);
        let (items, issues, _) = filtered_forest(roots(&tree), workers)?.summary;
        assert!(items.contains(&(0, path(&["a", "deep.txt"]), ResultItemKind::File, true)));
        assert!(items.contains(&(1, path(&["b.txt"]), ResultItemKind::File, true)));
        assert!(issues.contains(&revisit(0, &["b"])) && issues.contains(&revisit(0, &["twin"])));
        assert_eq!(tree.enumerated(101), 1, "workers={workers} seed={seed}");
        assert_eq!(tree.enumerated(102), 1, "workers={workers} seed={seed}");
    }
    Ok(())
}

#[test]
fn a_repeated_root_is_a_revisit_and_the_lower_index_collects() -> Result<(), String> {
    let _guard = worker_test_lock();
    let single = filtered_run(&tree(0), 1, true)?.summary;
    let repeated = |tree: &Arc<Tree>| {
        vec![
            Dir::at(tree, ROOT, Some(Vec::new())),
            Dir::at(tree, ROOT, Some(Vec::new())),
        ]
    };
    for (workers, seed) in RUNS {
        let tree = tree(seed);
        let run = filtered_forest(repeated(&tree), workers)?;
        let (items, issues, gaps) = run.summary;
        let root_items: Vec<_> = items.iter().filter(|item| item.0 == 0).cloned().collect();
        let root_issues: Vec<_> = issues
            .iter()
            .filter(|issue| issue.0 == 0)
            .cloned()
            .collect();
        assert_eq!(root_items, single.0, "workers={workers} seed={seed}");
        assert_eq!(root_issues, single.1, "workers={workers} seed={seed}");
        let repeat_items: Vec<_> = items.iter().filter(|item| item.0 == 1).collect();
        assert_eq!(
            repeat_items,
            [&(1, Vec::new(), ResultItemKind::Directory, false)]
        );
        let repeat_issues: Vec<_> = issues.iter().filter(|issue| issue.0 == 1).collect();
        assert_eq!(repeat_issues, [&revisit(1, &[])]);
        // A revisit is listed only: no gap and no failure for the repeated root.
        assert_eq!(gaps, [unverifiable_gap(0, &["odd"])]);
        assert_eq!(tree.enumerated(ROOT), 1);
        assert_eq!(run.outcome.counts.failed, 1);
    }
    Ok(())
}

/// Two parents, `p` and `q`, that list one directory under different names.
fn aliases() -> Listings {
    use ObservedKind::{Directory as D, File as F};
    BTreeMap::from([
        (500, vec![node("p", D, 1, 501), node("q", D, 1, 502)]),
        (
            501,
            vec![node("shared", D, 1, 510), node("p.txt", F, 1, 511)],
        ),
        (
            502,
            vec![node("alias", D, 1, 510), node("q.txt", F, 1, 512)],
        ),
        (510, vec![node("s.txt", F, 1, 513)]),
    ])
}

#[test]
fn an_aliased_directory_is_entered_once_and_one_worker_is_deterministic() -> Result<(), String> {
    let _guard = worker_test_lock();
    let run_alias = |workers, seed| -> Result<(Arc<Tree>, Summary), String> {
        let tree = tree_of(aliases(), seed);
        let root = Dir::at(&tree, 500, Some(Vec::new()));
        Ok((
            Arc::clone(&tree),
            filtered_forest(vec![root], workers)?.summary,
        ))
    };
    let (_, single_worker) = run_alias(1, 0)?;
    for (workers, seed) in RUNS {
        let (tree, summary) = run_alias(workers, seed)?;
        assert_eq!(tree.enumerated(510), 1, "workers={workers} seed={seed}");
        let (items, issues, _) = &summary;
        let entered =
            |parts: &[&str]| items.contains(&(0, path(parts), ResultItemKind::File, true));
        let via_p = entered(&["p", "shared", "s.txt"]);
        let via_q = entered(&["q", "alias", "s.txt"]);
        // Exactly one parent enters; the other keeps a revisit unread range.
        assert!(via_p != via_q, "workers={workers} seed={seed}");
        let blocked = if via_p {
            revisit(0, &["q", "alias"])
        } else {
            revisit(0, &["p", "shared"])
        };
        assert!(issues.contains(&blocked));
        // Across parents the first reservation wins, so only a single worker
        // reproduces one result for every delay order.
        if workers == 1 {
            assert_eq!(summary, single_worker, "seed={seed}");
        }
    }
    Ok(())
}

/// The parent of the scope root (inode 600), reached from the root through a
/// child directory, lists the root again.
fn parent_cycle() -> Listings {
    use ObservedKind::{Directory as D, File as F};
    BTreeMap::from([
        (
            ROOT,
            vec![node("up", D, 1, 600), node("top.txt", F, 1, 601)],
        ),
        (
            600,
            vec![node("scope", D, 1, ROOT), node("beside.txt", F, 1, 602)],
        ),
    ])
}

// #43: a root without ancestor evidence (as macOS reports an unreadable chain)
// is reserved before enumeration, so a child that leads to an ancestor and back
// to the root stops at the root. The ancestor itself is entered once.
#[test]
fn a_root_without_ancestors_stops_a_path_back_through_its_parent() -> Result<(), String> {
    let _guard = worker_test_lock();
    for (workers, seed) in RUNS {
        let tree = tree_of(parent_cycle(), seed);
        let root = Dir::at(&tree, ROOT, Some(Vec::new()));
        let run = filtered_forest(vec![root], workers)?;
        let context = format!("workers={workers} seed={seed}");
        assert_eq!(tree.enumerated(ROOT), 1, "{context}");
        assert_eq!(tree.enumerated(600), 1, "{context}");
        let (items, issues, gaps) = &run.summary;
        assert_eq!(issues, &vec![revisit(0, &["up", "scope"])], "{context}");
        assert!(gaps.is_empty(), "{context}");
        let file = |parts: &[&str]| (0, path(parts), ResultItemKind::File, true);
        assert!(items.contains(&file(&["up", "beside.txt"])), "{context}");
        assert!(!items.iter().any(|item| item.1.len() > 2), "{context}");
    }
    Ok(())
}
