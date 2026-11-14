//! A disconnection inside a child closes every unfinished directory.
//!
//! The sequential collector still has the root open when the child reports
//! the disconnection, so the root is a gap too. The parallel collector may
//! have finished reading the root by then, so its result holds the child's
//! rows and at most the root's row.
use super::*;

const REASON: &str = "scan.disconnected";

fn child_batch_disconnects() -> HashMap<u64, Listing> {
    let child = vec![
        file("x.txt", 131),
        without_metadata("lost", 132, disconnected()),
        dir("sub", 133),
    ];
    HashMap::from([
        (
            ROOT,
            Listing::Entries(vec![dir("a", 130), file("z.txt", 134)]),
        ),
        (130, Listing::Entries(child)),
        (133, Listing::Entries(vec![file("never.txt", 135)])),
    ])
}

fn child_open_disconnects() -> HashMap<u64, Listing> {
    HashMap::from([
        (
            ROOT,
            Listing::Entries(vec![dir("a", 130), file("z.txt", 134)]),
        ),
        (130, Listing::Refused(disconnected())),
    ])
}

fn sorted_issues(mut issues: Vec<Issue>) -> Vec<Issue> {
    issues.sort_by(|a, b| (&a.1, a.2).cmp(&(&b.1, b.2)));
    issues
}

/// Checks the sequential rows exactly and each parallel run against them.
fn check(
    listings: fn() -> HashMap<u64, Listing>,
    child: Vec<Issue>,
    gaps: Vec<Gap>,
) -> Result<(), String> {
    let baseline = sequential(listings(), false)?;
    let mut issues = child.clone();
    issues.push(range(&[], Stage::Enumerate, REASON));
    assert_eq!(
        sorted_issues(baseline.run.summary.1.clone()),
        sorted_issues(issues)
    );
    let mut all_gaps = gaps.clone();
    all_gaps.push(gap(&[], REASON));
    all_gaps.sort();
    assert_eq!(baseline.run.summary.2, all_gaps);
    assert_eq!(baseline.run.outcome.counts.failed, child.len() as u64 + 1);
    assert_eq!(baseline.run.completeness, ResultCompleteness::Missing);
    assert!(baseline.run.outcome.completed_scope);
    for workers in WORKERS {
        for filtered in [false, true] {
            let label = format!("workers={workers} filtered={filtered}");
            let (run, faults) = parallel(listings(), workers, filtered, false)?;
            let (items, listed, kept) = &run.run.summary;
            assert_eq!(items, &baseline.run.summary.0, "{label}");
            for row in &child {
                assert!(listed.contains(row), "{label}: {row:?}");
            }
            for row in listed {
                assert!(baseline.run.summary.1.contains(row), "{label}: {row:?}");
            }
            for row in &gaps {
                assert!(kept.contains(row), "{label}: {row:?}");
            }
            for row in kept {
                assert!(all_gaps.contains(row), "{label}: {row:?}");
            }
            assert_eq!(run.run.completeness, ResultCompleteness::Missing, "{label}");
            assert!(!faults.opened(133), "{label}");
        }
    }
    Ok(())
}

#[test]
fn a_disconnected_child_batch_closes_the_unfinished_directories() -> Result<(), String> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "lock")?;
    let child = vec![
        range(&["a"], Stage::Enumerate, REASON),
        entry_issue(&["a", "lost"], Kind::CollectionFailed, REASON),
    ];
    let gaps = vec![gap(&["a"], REASON), gap(&["a", "lost"], REASON)];
    check(child_batch_disconnects, child, gaps)
}

#[test]
fn a_disconnected_child_open_closes_the_unfinished_directories() -> Result<(), String> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "lock")?;
    let child = vec![range(&["a"], Stage::OpenDirectory, REASON)];
    check(child_open_disconnects, child, vec![gap(&["a"], REASON)])
}
