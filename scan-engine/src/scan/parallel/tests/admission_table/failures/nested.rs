//! A disconnection inside a child closes only that child.
//!
//! FR12.HD1.app-host.DD1: the root was being read when its child reported the
//! disconnection, so both collectors read the root to its end and leave it
//! without a gap. The child keeps the entries it returned, and an entered
//! directory among them is a gap of its own. Every worker count and delay
//! order gives the sequential rows exactly. A sibling directory may or may
//! not have started when the volume closed; it is then either read to its
//! end or a gap without contents, never partly read.
use super::*;

const REASON: &str = "scan.disconnected";
/// Delayed parallel runs that must all give the sequential result.
const REPEATS: u64 = 64;

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

/// The child batch case with a sibling directory beside the child.
fn sibling_beside_disconnect() -> HashMap<u64, Listing> {
    let mut listings = child_batch_disconnects();
    let root = vec![dir("a", 130), dir("b", 140), file("z.txt", 134)];
    listings.insert(ROOT, Listing::Entries(root));
    listings.insert(140, Listing::Entries(vec![file("b.txt", 141)]));
    listings
}

fn sorted_issues(mut issues: Vec<Issue>) -> Vec<Issue> {
    issues.sort_by(|a, b| (&a.1, a.2).cmp(&(&b.1, b.2)));
    issues
}

/// Compares every collector exactly and checks the expected rows and counts.
fn check(
    listings: fn() -> HashMap<u64, Listing>,
    issues: Vec<Issue>,
    mut gaps: Vec<Gap>,
    (completed, failed): (u64, u64),
) -> Result<Collected, String> {
    let baseline = compare(listings, false)?;
    assert_eq!(
        sorted_issues(baseline.run.summary.1.clone()),
        sorted_issues(issues)
    );
    gaps.sort();
    assert_eq!(baseline.run.summary.2, gaps);
    let counts = &baseline.run.outcome.counts;
    assert_eq!((counts.completed, counts.failed), (completed, failed));
    assert_eq!(baseline.run.completeness, ResultCompleteness::Missing);
    assert!(baseline.run.outcome.completed_scope);
    for workers in WORKERS {
        let (_, faults) = parallel(listings(), workers, false, false)?;
        assert!(!faults.opened(133), "workers={workers}");
    }
    Ok(baseline)
}

#[test]
fn a_disconnected_child_batch_closes_only_the_child() -> Result<(), String> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "lock")?;
    let issues = vec![
        range(&["a"], Stage::Enumerate, REASON),
        entry_issue(&["a", "lost"], Kind::CollectionFailed, REASON),
        range(&["a", "sub"], Stage::Enumerate, REASON),
    ];
    let gaps = vec![
        gap(&["a"], REASON),
        gap(&["a", "lost"], REASON),
        gap(&["a", "sub"], REASON),
    ];
    // z.txt and x.txt are completed; a, lost and sub are each one failure.
    let baseline = check(child_batch_disconnects, issues, gaps, (2, 3))?;
    let names: Vec<_> = baseline.run.summary.0.iter().map(|item| &item.1).collect();
    let expected = [
        path(&[]),
        path(&["a"]),
        path(&["a", "lost"]),
        path(&["a", "sub"]),
        path(&["a", "x.txt"]),
        path(&["z.txt"]),
    ];
    assert_eq!(names, expected.iter().collect::<Vec<_>>());
    Ok(())
}

#[test]
fn a_disconnected_child_open_closes_only_the_child() -> Result<(), String> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "lock")?;
    let issues = vec![range(&["a"], Stage::OpenDirectory, REASON)];
    check(
        child_open_disconnects,
        issues,
        vec![gap(&["a"], REASON)],
        (1, 1),
    )?;
    Ok(())
}

/// The rows and counts that identify one collection result.
type Fingerprint = (Summary, u64, u64, ResultCompleteness);

fn fingerprint(collected: &Collected) -> Fingerprint {
    let counts = &collected.run.outcome.counts;
    (
        collected.run.summary.clone(),
        counts.completed,
        counts.failed,
        collected.run.completeness,
    )
}

#[test]
fn delayed_parallel_runs_give_one_result_for_a_disconnected_child() -> Result<(), String> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "lock")?;
    for listings in [child_batch_disconnects, child_open_disconnects] {
        let expected = fingerprint(&sequential(listings(), false)?);
        let mut results = Vec::new();
        for run in 0..REPEATS {
            let workers = WORKERS[run as usize % WORKERS.len()];
            let filtered = run % 2 == 1;
            let label = format!("run={run} workers={workers} filtered={filtered}");
            let (collected, faults) =
                parallel_seeded(listings(), (workers, run + 1), filtered, false)?;
            assert!(!faults.opened(133), "{label}");
            let result = fingerprint(&collected);
            if !results.contains(&result) {
                results.push(result);
            }
        }
        assert_eq!(results, [expected]);
    }
    Ok(())
}

#[test]
fn a_sibling_is_read_to_its_end_or_left_as_a_gap_without_contents() -> Result<(), String> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "lock")?;
    let child = |collected: &Collected| -> Vec<Item> {
        let rows = collected.run.summary.0.iter();
        rows.filter(|item| item.1.first() == Some(&b"a".to_vec()))
            .cloned()
            .collect()
    };
    let baseline = sequential(sibling_beside_disconnect(), false)?;
    assert_counted_once(&baseline, "sequential");
    // The sequential collector opens a first, so b has not started.
    assert!(baseline.run.summary.2.contains(&gap(&["b"], REASON)));
    for run in 0..REPEATS {
        let workers = WORKERS[run as usize % WORKERS.len()];
        let label = format!("run={run} workers={workers}");
        let input = sibling_beside_disconnect();
        let (collected, _) = parallel_seeded(input, (workers, run + 1), false, false)?;
        assert_counted_once(&collected, &label);
        let (items, _, gaps) = &collected.run.summary;
        assert_eq!(child(&collected), child(&baseline), "{label}");
        assert!(!gaps.iter().any(|gap| gap.1.is_empty()), "{label}");
        let b_gap = gaps.contains(&gap(&["b"], REASON));
        let b_file = items.iter().any(|item| item.1 == path(&["b", "b.txt"]));
        assert!(b_gap != b_file, "{label}: gap={b_gap} contents={b_file}");
    }
    Ok(())
}
