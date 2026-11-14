//! Worker counts, lane counts and delay orders do not change the result.
use super::*;

const SHAPE: Shape = Shape {
    depth: 2,
    dirs: 4,
    files: 3,
    batch: 3,
};

fn context() -> Context {
    Context {
        completed: Arc::new(AtomicUsize::new(0)),
        abandoned: Arc::new(AtomicUsize::new(0)),
    }
}

/// A tree of three roots with one denied read and one rejected open. A
/// nonzero `seed` delays about a third of the calls by a seed-dependent time.
fn probe(seed: u64) -> Arc<Probe> {
    Probe::new(
        SHAPE,
        3,
        Box::new(move |root, path, call| {
            if root == 2 && call == Call::Read && path == [b"d1".to_vec()] {
                return Err(PortError {
                    kind: PortErrorKind::PermissionDenied,
                    reason_key: "os.denied".into(),
                    os_code: None,
                });
            }
            if root == 1 && call == Call::Open && path == [b"d3".to_vec()] {
                return Err(io_error("os.query_failed"));
            }
            let spread = identity(root, path) ^ seed.wrapping_mul(0x9e37_79b9_7f4a_7c15);
            if seed != 0 && spread.is_multiple_of(3) {
                thread::sleep(Duration::from_micros(100 + spread % 700));
            }
            Ok(())
        }),
    )
}

#[test]
fn workers_lanes_and_delays_do_not_change_the_result() -> Result<(), TestError> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "worker test lock")?;
    let baseline = run_forest(&probe(0), vec![0, 0, 0], 1, 4, &context())?;
    // Root 1 keeps "d3" and root 2 keeps "d1" without anything below them.
    let lost = (1 + SHAPE.dirs) * (1 + SHAPE.files) - 1;
    assert_eq!(baseline.rows.len(), 3 * SHAPE.items() - 2 * lost);
    assert_eq!(baseline.completeness, ResultCompleteness::Missing);
    assert!(
        baseline
            .issues
            .iter()
            .any(|issue| issue.contains("scan.path_denied"))
    );
    assert_eq!(baseline.issues.len(), 4, "{:?}", baseline.issues);
    for workers in [1, 2, 4, 8] {
        for groups in [vec![0, 0, 0], vec![0, 0, 1], vec![0, 1, 2]] {
            for seed in [0, 1, 2] {
                let run = run_forest(&probe(seed), groups.clone(), workers, 4, &context())?;
                let case = format!("workers {workers}, groups {groups:?}, seed {seed}");
                assert!(run.outcome.completed_scope, "{case}");
                assert_eq!(run.outcome.failure_reason_key, None, "{case}");
                assert_eq!(run.completeness, baseline.completeness, "{case}");
                assert_eq!(run.rows, baseline.rows, "{case}");
                assert_eq!(run.issues, baseline.issues, "{case}");
            }
        }
    }
    Ok(())
}
