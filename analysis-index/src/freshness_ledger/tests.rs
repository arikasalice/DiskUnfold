//! The ledger settles from stored outcomes only; it has no access to items or gaps.
use super::*;
use crate::{IngestSink, ResultVersion};
use du_core_contracts::{
    PathBytes, PathEncoding,
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot},
    result_events::{ResultVersionNotice, ResultVersionPublisher},
    result_freshness::{ResultFreshnessNotice, ResultFreshnessPublisher},
};
use std::sync::Arc;

type Check = Result<(), String>;

fn id(n: u32) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}

fn outcome(processed: bool, failed: Option<&str>) -> RootOutcome {
    RootOutcome {
        failed: failed.map(str::to_owned),
        processed,
    }
}

/// Rows as (state, reason) of a notice that lists its changed ranges.
fn listed(notice: &ResultFreshnessNotice) -> Option<Vec<(String, Option<String>)>> {
    match &notice.body.changed {
        FreshnessChanged::Ranges { ranges } => Some(
            ranges
                .iter()
                .map(|r| (r.state.as_str().to_owned(), r.reason_key.clone()))
                .collect(),
        ),
        _ => None,
    }
}

fn state(name: &str, reason: Option<&str>) -> (String, Option<String>) {
    (name.to_owned(), reason.map(str::to_owned))
}

#[test]
fn sealing_settles_every_root_from_the_stored_outcomes_as_they_are() -> Check {
    let (lineage, scope, version, sealed) = (id(1)?, id(10)?, id(20)?, id(21)?);
    // Root 0 was processed, root 1 failed as a whole, root 2 was not processed.
    let stored = [
        outcome(true, None),
        outcome(true, Some("scan.denied")),
        outcome(false, None),
    ];
    let header = |version_id, outcomes| LedgerVersion {
        version_id,
        scope_version_id: &scope,
        root_count: 3,
        observed_at: None,
        outcomes,
    };
    let mut ledgers = Ledgers::default();
    let refreshing = ledgers
        .publish(&lineage, &header(&version, None), true, false)
        .ok_or("no refresh notice")?;
    assert_eq!(refreshing.body.summary.counts.refreshing, 3);
    let notice = ledgers
        .publish(&lineage, &header(&sealed, Some(&stored)), false, true)
        .ok_or("no seal notice")?;
    assert_eq!(
        listed(&notice),
        Some(vec![
            state("unwatched", None),
            state("unverifiable", Some("scan.denied")),
            state("unverifiable", None),
        ]),
        "the ledger follows the stored array and nothing else"
    );
    assert_eq!(notice.body.summary.counts.unwatched, 1);
    assert_eq!(notice.body.summary.counts.unverifiable, 2);
    Ok(())
}

#[test]
fn a_version_without_stored_outcomes_settles_every_root_as_not_processed() -> Check {
    let (lineage, scope) = (id(1)?, id(10)?);
    let (first, second, third) = (id(20)?, id(21)?, id(22)?);
    let stored = [outcome(true, None), outcome(true, None)];
    let header = |version_id, outcomes| LedgerVersion {
        version_id,
        scope_version_id: &scope,
        root_count: 2,
        observed_at: None,
        outcomes,
    };
    let mut ledgers = Ledgers::default();
    // No earlier state: unverifiable, and no reason key.
    let bare = ledgers
        .publish(&lineage, &header(&first, None), true, true)
        .ok_or("no notice")?;
    assert_eq!(
        listed(&bare),
        Some(vec![
            state("unverifiable", None),
            state("unverifiable", None)
        ])
    );
    // With an earlier state, the rows return to it and nothing is announced.
    ledgers.publish(&lineage, &header(&second, Some(&stored)), true, true);
    let again = ledgers.publish(&lineage, &header(&third, None), true, true);
    assert!(
        again.is_none(),
        "unprocessed roots keep their earlier state"
    );
    Ok(())
}

#[test]
fn an_abort_that_derives_the_ledger_again_uses_the_baselines_stored_outcomes() -> Check {
    let (lineage, scope, build_scope, version) = (id(1)?, id(10)?, id(11)?, id(20)?);
    let stored = [outcome(true, None), outcome(true, Some("scan.gone"))];
    let baseline = |outcomes| LedgerVersion {
        version_id: &version,
        scope_version_id: &scope,
        root_count: 2,
        observed_at: None,
        outcomes,
    };
    // The baseline's scope version has no ledger, so the abort derives it from the outcomes.
    let mut ledgers = Ledgers::default();
    let notice = ledgers
        .abort(&lineage, &build_scope, &baseline(Some(&stored)))
        .ok_or("no notice")?;
    assert_eq!(
        listed(&notice),
        Some(vec![
            state("unwatched", None),
            state("unverifiable", Some("scan.gone"))
        ])
    );
    // Without stored outcomes the same derivation settles every root as not processed.
    let mut bare = Ledgers::default();
    let notice = bare
        .abort(&lineage, &build_scope, &baseline(None))
        .ok_or("no notice")?;
    assert_eq!(
        listed(&notice),
        Some(vec![
            state("unverifiable", None),
            state("unverifiable", None)
        ])
    );
    Ok(())
}

struct Silent;
impl ResultVersionPublisher for Silent {
    fn publish_result_version(&self, _: ResultVersionNotice) {}
}
impl ResultFreshnessPublisher for Silent {
    fn publish_result_freshness(&self, _: ResultFreshnessNotice) {}
}

fn roots(count: u8) -> Vec<ResultScopeRoot> {
    (0..count)
        .map(|n| ResultScopeRoot {
            path: ResultNativePath {
                encoding: PathEncoding::WindowsUtf16le,
                raw: PathBytes::from_bytes(&[67 + n, 0]),
            },
            volume_id: vec![n + 1],
            file_id: vec![n + 1],
        })
        .collect()
}

fn sealed_partial(sink: &IngestSink) -> Result<Arc<ResultVersion>, String> {
    let fail = |e: crate::BuildError| format!("{e:?}");
    let mut build = sink.open_forest(id(10)?, roots(3)).map_err(fail)?;
    for root in 0..3 {
        build.add_directory_at(root, Vec::new()).map_err(fail)?;
    }
    // Only root 1 has an item below its directory; root 2 failed as a whole.
    build
        .add_directory_at(1, vec![b"d".to_vec()])
        .map_err(fail)?;
    build
        .record_gap_at(2, Vec::new(), "scan.denied".into(), None)
        .map_err(fail)?;
    build
        .seal(id(20)?, ResultCompleteness::Partial)
        .map_err(fail)
}

#[test]
fn a_sealed_result_stores_one_outcome_per_root_when_freshness_is_tracked() -> Check {
    let publisher = Arc::new(Silent);
    let sink = IngestSink::with_publishers(publisher.clone(), publisher);
    let result = sealed_partial(&sink)?;
    let stored = result.freshness_outcomes.as_deref().ok_or("not stored")?;
    assert_eq!(
        stored,
        [
            outcome(false, None),
            outcome(true, None),
            outcome(false, Some("scan.denied")),
        ]
    );
    Ok(())
}

#[test]
fn a_store_without_a_freshness_publisher_stores_no_outcomes() -> Check {
    let result = sealed_partial(&IngestSink::default())?;
    assert!(result.freshness_outcomes.is_none());
    Ok(())
}
