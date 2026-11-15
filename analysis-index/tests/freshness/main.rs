//! F33 FR3.HD1: the freshness ledger and its notices.
mod support;

use du_analysis_index::SampleSessionKey;
use du_core_contracts::{
    Id, PathBytes, PathEncoding, Timestamp,
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot},
    result_freshness::{FreshnessChanged, MAX_FRESHNESS_RANGES},
};
use support::*;

#[test]
fn first_publication_refreshes_every_root_and_sealing_settles_them() -> Check {
    let (sink, log) = sink()?;
    let l = lineage(&sink, "a", 1)?;
    let mut build = open(&sink, &l, 10, 2, &[0, 1])?;
    build.publish_partial(id(20)?)?;
    build.publish_partial(id(21)?)?;
    seal(build, 22, ResultCompleteness::Complete)?;
    let fresh = log.fresh();
    assert_eq!(
        fresh.len(),
        2,
        "the second partial publication changes nothing"
    );
    let first = &fresh[0];
    assert!(first.validate());
    assert_eq!(first.result_version, Some(id(20)?));
    assert_eq!(first.body.lineage_id, l);
    assert_eq!(first.body.scope_version_id, id(10)?);
    assert_eq!(first.body.sequence, 1);
    assert_eq!(
        ranges(first),
        Some(vec![row(0, "refreshing", None), row(1, "refreshing", None)])
    );
    assert_eq!(first.body.summary.representative.as_str(), "refreshing");
    assert_eq!(first.body.summary.counts.refreshing, 2);
    assert_eq!(
        first.body.observed_at, None,
        "a refresh start is no observation"
    );
    let sealed = &fresh[1];
    assert_eq!(sealed.result_version, Some(id(22)?));
    assert_eq!(sealed.body.sequence, 2);
    assert_eq!(
        ranges(sealed),
        Some(vec![row(0, "unwatched", None), row(1, "unwatched", None)])
    );
    assert_eq!(sealed.body.summary.representative.as_str(), "unwatched");
    assert_eq!(sealed.body.summary.counts.unwatched, 2);
    assert_eq!(sealed.body.observed_at, Timestamp::from_unix_millis(5_000));
    // Each freshness notice follows the version notice of the same commit.
    assert_eq!(log.kinds(), "VFVVF");
    assert!(
        log.readable
            .lock()
            .map_err(|_| "poisoned")?
            .iter()
            .all(|r| *r)
    );
    Ok(())
}

#[test]
fn a_root_that_could_not_be_opened_is_unverifiable_with_the_gap_reason() -> Check {
    let (sink, log) = sink()?;
    let l = lineage(&sink, "a", 1)?;
    let mut build = open(&sink, &l, 10, 3, &[])?;
    build.record_gap_at(1, Vec::new(), "scan.denied".into(), None)?;
    // A gap below a root does not make the root unverifiable.
    build.record_gap_at(2, vec![b"d".to_vec()], "scan.skipped".into(), None)?;
    build.add_directory_at(2, vec![b"d".to_vec()])?;
    seal(build, 20, ResultCompleteness::Complete)?;
    let sealed = log.fresh().remove(0);
    assert_eq!(
        ranges(&sealed),
        Some(vec![
            row(0, "unwatched", None),
            row(1, "unverifiable", Some("scan.denied")),
            row(2, "unwatched", None),
        ])
    );
    assert_eq!(sealed.body.summary.representative.as_str(), "unverifiable");
    assert_eq!(sealed.body.summary.counts.unverifiable, 1);
    assert_eq!(sealed.body.summary.counts.total(), 3);
    Ok(())
}

#[test]
fn unprocessed_roots_of_a_partial_seal_keep_their_earlier_state() -> Check {
    let (sink, log) = sink()?;
    let l = lineage(&sink, "a", 1)?;
    // First analysis: root 1 is unreadable, root 0 is fine.
    let mut first = open(&sink, &l, 10, 2, &[0, 1])?;
    first.record_gap_at(1, Vec::new(), "scan.denied".into(), None)?;
    seal(first, 20, ResultCompleteness::Complete)?;
    // A second analysis is cancelled before either root was read below its directory.
    let mut second = open(&sink, &l, 10, 2, &[])?;
    second.publish_partial(id(21)?)?;
    seal(second, 22, ResultCompleteness::Cancelled)?;
    let after = log.fresh().pop().ok_or("no notice")?;
    assert_eq!(after.body.sequence, 3, "refresh start, then settle");
    // The refreshing rows return to their earlier states, including the reason.
    assert_eq!(
        ranges(&after),
        Some(vec![
            row(0, "unwatched", None),
            row(1, "unverifiable", Some("scan.denied")),
        ])
    );
    // Without an earlier state an unprocessed root is unverifiable without a reason.
    let l2 = lineage(&sink, "b", 2)?;
    let mut fresh = open(&sink, &l2, 30, 2, &[0])?;
    fresh.publish_partial(id(40)?)?;
    seal(fresh, 41, ResultCompleteness::Partial)?;
    let count = log.fresh().len();
    // A cancelled analysis that settles to the earlier states again sends nothing more.
    seal(
        open(&sink, &l2, 30, 2, &[0])?,
        42,
        ResultCompleteness::Partial,
    )?;
    assert_eq!(log.fresh().len(), count);
    let sealed = log.fresh().pop().ok_or("no notice")?;
    assert_eq!(
        ranges(&sealed),
        Some(vec![
            row(0, "unwatched", None),
            row(1, "unverifiable", None)
        ])
    );
    Ok(())
}

#[test]
fn reanalysis_refreshes_again_and_an_abort_restores_the_earlier_state() -> Check {
    let (sink, log) = sink()?;
    let l = lineage(&sink, "a", 1)?;
    seal(
        open(&sink, &l, 10, 2, &[0, 1])?,
        20,
        ResultCompleteness::Complete,
    )?;
    let mut again = open(&sink, &l, 10, 2, &[0, 1])?;
    again.publish_partial(id(21)?)?;
    drop(again);
    let fresh = log.fresh();
    // seal of the first build: the first publication and the seal are one change.
    assert_eq!(fresh.len(), 3);
    assert_eq!(fresh[0].body.sequence, 1);
    assert_eq!(
        ranges(&fresh[1]),
        Some(vec![row(0, "refreshing", None), row(1, "refreshing", None)])
    );
    assert_eq!(fresh[1].result_version, Some(id(21)?));
    assert_eq!(fresh[1].body.observed_at, None);
    // The abort restores the baseline version and the earlier states.
    assert_eq!(fresh[2].result_version, Some(id(20)?));
    assert_eq!(fresh[2].body.sequence, 3);
    assert_eq!(
        ranges(&fresh[2]),
        Some(vec![row(0, "unwatched", None), row(1, "unwatched", None)])
    );
    assert_eq!(fresh[2].body.observed_at, None);
    Ok(())
}

#[test]
fn withdrawing_the_only_version_removes_the_ledger_and_the_sequence_continues() -> Check {
    let (sink, log) = sink()?;
    let l = lineage(&sink, "a", 1)?;
    let mut first = open(&sink, &l, 10, 2, &[])?;
    first.publish_partial(id(20)?)?;
    drop(first);
    let gone = log.fresh().remove(1);
    assert!(gone.validate());
    assert_eq!(gone.result_version, None);
    assert_eq!(gone.body.sequence, 2);
    assert_eq!(gone.body.summary.counts.total(), 0);
    assert_eq!(
        gone.body.summary.representative.as_str(),
        "unverifiable",
        "a removed ledger has no rows, so a receiver must not read it as observed"
    );
    assert_eq!(gone.body.changed, FreshnessChanged::Requery);
    assert_eq!(gone.body.scope_version_id, id(10)?);
    assert_eq!(gone.body.observed_at, None);
    // The same scope version analysed again continues its sequence.
    seal(
        open(&sink, &l, 10, 2, &[])?,
        30,
        ResultCompleteness::Complete,
    )?;
    let again = log.fresh().pop().ok_or("no notice")?;
    assert_eq!(again.body.sequence, 3);
    assert_eq!(log.kinds(), "VFVFVF");
    Ok(())
}

#[test]
fn a_replacement_that_changes_nothing_sends_no_freshness_notice() -> Check {
    let (sink, log) = sink()?;
    let l = lineage(&sink, "a", 1)?;
    seal(
        open(&sink, &l, 10, 1, &[])?,
        20,
        ResultCompleteness::Complete,
    )?;
    let base = sink.pin_current(&id(20)?).map_err(|e| format!("{e:?}"))?;
    let mut candidate = sink.open_replacement(&base).map_err(|e| format!("{e:?}"))?;
    candidate.add_directory_at(0, Vec::new())?;
    seal(candidate, 21, ResultCompleteness::Complete)?;
    assert_eq!(log.fresh().len(), 1);
    assert_eq!(log.kinds(), "VFV");
    Ok(())
}

#[test]
fn a_replacement_that_finds_an_unreadable_root_reports_it() -> Check {
    let (sink, log) = sink()?;
    let l = lineage(&sink, "a", 1)?;
    seal(
        open(&sink, &l, 10, 2, &[])?,
        20,
        ResultCompleteness::Complete,
    )?;
    let base = sink.pin_current(&id(20)?).map_err(|e| format!("{e:?}"))?;
    let mut candidate = sink.open_replacement(&base).map_err(|e| format!("{e:?}"))?;
    candidate.add_directory_at(0, Vec::new())?;
    candidate.add_directory_at(1, Vec::new())?;
    candidate.record_gap_at(1, Vec::new(), "scan.disconnected".into(), None)?;
    seal(candidate, 21, ResultCompleteness::Complete)?;
    let notice = log.fresh().pop().ok_or("no notice")?;
    assert_eq!(notice.body.sequence, 2);
    assert_eq!(
        ranges(&notice),
        Some(vec![row(1, "unverifiable", Some("scan.disconnected"))]),
        "only the changed row is listed"
    );
    assert_eq!(notice.body.summary.counts.unwatched, 1);
    assert_eq!(notice.body.summary.counts.unverifiable, 1);
    Ok(())
}

#[test]
fn sequences_belong_to_the_scope_version_and_not_to_the_lineage() -> Check {
    let (sink, log) = sink()?;
    let a = lineage(&sink, "a", 1)?;
    let b = lineage(&sink, "b", 2)?;
    let mut build = open(&sink, &a, 10, 1, &[])?;
    build.publish_partial(id(20)?)?;
    build.publish_partial(id(21)?)?;
    seal(build, 22, ResultCompleteness::Complete)?;
    seal(
        open(&sink, &b, 11, 1, &[])?,
        23,
        ResultCompleteness::Complete,
    )?;
    let entries = log.entries.lock().map_err(|_| "poisoned")?;
    let version_sequences: Vec<u64> = entries
        .iter()
        .filter_map(|e| match e {
            Entry::Version(n) if n.body.lineage_id == a => Some(n.body.sequence),
            _ => None,
        })
        .collect();
    assert_eq!(version_sequences, [1, 2, 3]);
    let fresh: Vec<(Id, u64)> = entries
        .iter()
        .filter_map(|e| match e {
            Entry::Fresh(n) => Some((n.body.scope_version_id.clone(), n.body.sequence)),
            Entry::Version(_) => None,
        })
        .collect();
    assert_eq!(
        fresh,
        [(id(10)?, 1), (id(10)?, 2), (id(11)?, 1)],
        "the lineage reached 3 while the scope version is at 2"
    );
    Ok(())
}

#[test]
fn builds_without_a_lineage_and_sample_builds_send_nothing() -> Check {
    let (sink, log) = sink()?;
    let mut anonymous = sink
        .open_forest(id(10)?, roots(1))
        .map_err(|e| format!("{e:?}"))?;
    anonymous.add_directory_at(0, Vec::new())?;
    seal(anonymous, 20, ResultCompleteness::Complete)?;
    let mut sample = sink
        .open_sample_forest(id(11)?, roots(1), &SampleSessionKey::new(id(500)?))
        .map_err(|e| format!("{e:?}"))?;
    sample.add_directory_at(0, Vec::new())?;
    seal(sample, 21, ResultCompleteness::Complete)?;
    assert!(log.fresh().is_empty());
    Ok(())
}

#[test]
fn a_ledger_goes_when_no_current_version_uses_its_scope_version() -> Check {
    let (sink, log) = sink()?;
    let l = lineage(&sink, "a", 1)?;
    seal(
        open(&sink, &l, 10, 1, &[])?,
        20,
        ResultCompleteness::Complete,
    )?;
    // The lineage moves to another scope version.
    let mut moved = open(&sink, &l, 11, 1, &[])?;
    moved.publish_partial(id(21)?)?;
    let fresh = log.fresh();
    let removed = fresh.last().ok_or("no notice")?;
    assert_eq!(removed.body.scope_version_id, id(10)?);
    assert_eq!(removed.result_version, None);
    assert_eq!(removed.body.sequence, 2);
    assert_eq!(removed.body.summary.counts.total(), 0);
    assert_eq!(removed.body.summary.representative.as_str(), "unverifiable");
    let starts = &fresh[fresh.len() - 2];
    assert_eq!(starts.body.scope_version_id, id(11)?);
    assert_eq!(starts.body.sequence, 1);
    // Withdrawing the move restores the old scope version's ledger from its result.
    drop(moved);
    let fresh = log.fresh();
    let restored = &fresh[fresh.len() - 2];
    assert_eq!(restored.body.scope_version_id, id(10)?);
    assert_eq!(restored.result_version, Some(id(20)?));
    assert_eq!(restored.body.sequence, 3);
    assert_eq!(ranges(restored), Some(vec![row(0, "unwatched", None)]));
    let gone = fresh.last().ok_or("no notice")?;
    assert_eq!(gone.body.scope_version_id, id(11)?);
    assert_eq!(gone.result_version, None);
    assert_eq!(gone.body.sequence, 2);
    Ok(())
}

#[test]
fn more_changed_rows_than_the_limit_are_sent_as_a_requery() -> Check {
    let (sink, log) = sink()?;
    let l = lineage(&sink, "a", 1)?;
    // 257 roots do not fit the helper's u8 count, so build the roots here.
    let many: Vec<ResultScopeRoot> = (0..=MAX_FRESHNESS_RANGES as u32)
        .map(|n| ResultScopeRoot {
            path: ResultNativePath {
                encoding: PathEncoding::WindowsUtf16le,
                raw: PathBytes::from_bytes(&n.to_le_bytes()),
            },
            volume_id: vec![1],
            file_id: n.to_le_bytes().to_vec(),
        })
        .collect();
    let rows = many.len() as u64;
    let mut build = sink
        .open_forest_in_lineage(id(10)?, many, l)
        .map_err(|e| format!("{e:?}"))?;
    for root in 0..=MAX_FRESHNESS_RANGES as u32 {
        build.add_directory_at(root, Vec::new())?;
    }
    build.publish_partial(id(20)?)?;
    let first = log.fresh().remove(0);
    assert!(first.validate());
    assert_eq!(first.body.changed, FreshnessChanged::Requery);
    assert_eq!(first.body.summary.counts.refreshing, rows);
    assert_eq!(first.body.summary.counts.total(), rows);
    Ok(())
}
