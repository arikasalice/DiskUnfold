//! Bounds of the freshness ledger: the changed-row limit and the retired sequences.
use super::support::*;
use du_core_contracts::{
    Id, PathBytes, PathEncoding,
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot},
    result_freshness::{FreshnessChanged, MAX_FRESHNESS_RANGES},
};

/// How many removed ledgers the store remembers; mirrors the library's limit.
const RETIRED_LIMIT: u32 = 64;

fn many_roots(count: usize) -> Vec<ResultScopeRoot> {
    (0..count as u32)
        .map(|n| ResultScopeRoot {
            path: ResultNativePath {
                encoding: PathEncoding::WindowsUtf16le,
                raw: PathBytes::from_bytes(&n.to_le_bytes()),
            },
            volume_id: vec![1],
            file_id: n.to_le_bytes().to_vec(),
        })
        .collect()
}

/// Starts a build of `count` roots and returns the notice of its first publication.
fn first_notice(
    count: usize,
) -> Result<du_core_contracts::result_freshness::ResultFreshnessNotice, Failure> {
    let (sink, log) = sink()?;
    let l = lineage(&sink, "a", 1)?;
    let mut build = sink
        .open_forest_in_lineage(id(10)?, many_roots(count), l)
        .map_err(|e| format!("{e:?}"))?;
    for root in 0..count as u32 {
        build.add_directory_at(root, Vec::new())?;
    }
    build.publish_partial(id(20)?)?;
    Ok(log.fresh().remove(0))
}

#[test]
fn exactly_the_limit_of_changed_rows_is_listed_and_one_more_is_a_requery() -> Check {
    let at_limit = first_notice(MAX_FRESHNESS_RANGES)?;
    assert!(at_limit.validate());
    assert_eq!(
        ranges(&at_limit).map(|r| r.len()),
        Some(MAX_FRESHNESS_RANGES),
        "256 changed rows are sent as ranges"
    );
    let over = first_notice(MAX_FRESHNESS_RANGES + 1)?;
    assert!(over.validate());
    assert_eq!(over.body.changed, FreshnessChanged::Requery);
    assert_eq!(
        over.body.summary.counts.refreshing,
        MAX_FRESHNESS_RANGES as u64 + 1
    );
    Ok(())
}

#[test]
fn only_the_oldest_removed_ledger_beyond_the_retired_limit_restarts_its_sequence() -> Check {
    let (sink, log) = sink()?;
    let l = lineage(&sink, "a", 1)?;
    // Each scope version replaces the previous one, which removes the previous ledger.
    // After RETIRED_LIMIT + 2 scope versions, RETIRED_LIMIT + 1 ledgers were removed, so
    // the first one fell out of the remembered list and the second is still in it.
    let scope = |n: u32| 100 + n;
    for n in 0..RETIRED_LIMIT + 2 {
        seal(
            open(&sink, &l, scope(n), 1, &[])?,
            1_000 + n,
            ResultCompleteness::Complete,
        )?;
    }
    let last_of = |scope_number: u32| -> Result<Option<(u64, bool)>, Failure> {
        let wanted: Id = id(scope(scope_number))?;
        Ok(log
            .fresh()
            .iter()
            .rev()
            .find(|n| n.body.scope_version_id == wanted)
            .map(|n| (n.body.sequence, n.result_version.is_some())))
    };
    // Sealing made sequence 1, and its removal made sequence 2.
    assert_eq!(last_of(0)?, Some((2, false)));
    assert_eq!(last_of(1)?, Some((2, false)));
    // Scope version 1 is still remembered, so it continues after its removal notice.
    seal(
        open(&sink, &l, scope(1), 1, &[])?,
        2_000,
        ResultCompleteness::Complete,
    )?;
    assert_eq!(last_of(1)?, Some((3, true)));
    // Scope version 0 was evicted from the list and restarts at 1.
    seal(
        open(&sink, &l, scope(0), 1, &[])?,
        2_001,
        ResultCompleteness::Complete,
    )?;
    assert_eq!(last_of(0)?, Some((1, true)));
    Ok(())
}
