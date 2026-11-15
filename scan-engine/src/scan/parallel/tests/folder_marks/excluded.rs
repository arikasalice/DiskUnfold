//! Entries an exclusion rule dropped directly below each folder.
use super::fixture::*;
use super::*;
use du_analysis_index::folder_collection::ExcludedChildren;

/// The root drops `x1`, `x2` and `x3` over two batches; `u1` cannot be
/// decided by the filter. `d` drops nothing and `e` drops `x4` before its
/// read fails.
fn tree() -> Arc<Tree> {
    let root = vec![
        vec![
            file("a", 2),
            file("x1", 3),
            dir("d", 10),
            dir("x2", 11),
            file("u1", 12),
        ],
        vec![file("x3", 4), file("b", 5), dir("e", 13)],
    ];
    Tree::new(
        HashMap::from([
            (ROOT, Node::Batches(root)),
            (10, Node::Batches(vec![vec![file("c", 101)]])),
            (
                13,
                Node::FailsAfter(vec![vec![file("x4", 131), file("f", 132)]]),
            ),
        ]),
        Trigger::Never,
    )
}

fn exact(count: u32) -> Option<ExcludedChildren> {
    Some(ExcludedChildren { count, exact: true })
}

fn lower_bound(count: u32) -> Option<ExcludedChildren> {
    Some(ExcludedChildren {
        count,
        exact: false,
    })
}

#[test]
fn each_folder_reports_its_direct_exclusions_once_over_all_batches() -> Result<(), String> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "lock")?;
    for collector in COLLECTORS {
        let label = format!("{collector:?}");
        let (outcome, version) = collect(&tree(), collector)?;
        assert!(outcome.completed_scope, "{label}");
        // `e` failed, and with the filter `u1` is an unverifiable range.
        assert_eq!(
            version.completeness(),
            ResultCompleteness::Missing,
            "{label}"
        );
        // Undecided `u1` is not counted; `x2` counts as an entry like any other.
        assert_eq!(excluded(&version, &[])?, exact(3), "{label}");
        // An unreported folder of a finished scan answers an exact zero.
        assert_eq!(excluded(&version, &["d"])?, exact(0), "{label}");
        // The read of `e` failed after `x4` was excluded.
        assert_eq!(excluded(&version, &["e"])?, lower_bound(1), "{label}");
        assert_eq!(version.unread_folder_counts().cancelled, 0, "{label}");
    }
    Ok(())
}

/// In a cancelled version a folder without a report answers `None`, so a
/// folder that excluded nothing is shown not to be reported.
#[test]
fn folders_without_exclusions_are_not_reported() -> Result<(), String> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "lock")?;
    for collector in COLLECTORS {
        let label = format!("{collector:?}");
        let mut nodes = HashMap::from([(ROOT, Node::Batches(vec![vec![dir("d", 10)]]))]);
        nodes.insert(10, Node::Batches(vec![vec![file("c", 101)], vec![]]));
        let tree = Tree::new(nodes, Trigger::Read(10, 1));
        let (_, version) = collect(&tree, collector)?;
        assert_eq!(
            version.completeness(),
            ResultCompleteness::Cancelled,
            "{label}"
        );
        assert_eq!(excluded(&version, &[])?, None, "{label}");
        // `d` was read to its end with nothing excluded: neither marked nor reported.
        assert!(!cancelled(&version)?.contains(&path(&["d"])), "{label}");
        assert_eq!(excluded(&version, &["d"])?, None, "{label}");
    }
    Ok(())
}

/// `c` reports the disconnection after `x2`; the root drops `x1` and is
/// read to its end, as a started folder on a disconnected volume is.
#[test]
fn a_folder_closed_by_a_disconnection_reports_a_lower_bound() -> Result<(), String> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "lock")?;
    let mut lost = file("lost", 202);
    lost.metadata = Err(PortError {
        kind: PortErrorKind::Io,
        reason_key: "scan.disconnected".into(),
        os_code: None,
    });
    for collector in COLLECTORS {
        let label = format!("{collector:?}");
        let tree = Tree::new(
            HashMap::from([
                (ROOT, Node::Batches(vec![vec![file("x1", 2), dir("c", 20)]])),
                (
                    20,
                    Node::Batches(vec![vec![file("x2", 201), lost.clone(), file("k", 203)]]),
                ),
            ]),
            Trigger::Never,
        );
        let (outcome, version) = collect(&tree, collector)?;
        assert!(outcome.completed_scope, "{label}");
        assert_eq!(excluded(&version, &[])?, exact(1), "{label}");
        assert_eq!(excluded(&version, &["c"])?, lower_bound(1), "{label}");
        let unread = unread(&version)?;
        assert_eq!(
            unread.get(&path(&["c"])).map(String::as_str),
            Some("scan.disconnected"),
            "{label}"
        );
        assert_eq!(version.unread_folder_counts().cancelled, 0, "{label}");
    }
    Ok(())
}
