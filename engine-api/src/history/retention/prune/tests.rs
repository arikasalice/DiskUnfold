use super::*;

fn candidate(seed: u8, row_count: u64) -> HistoryPruneCandidate {
    let mut bytes = [seed; 16];
    bytes[6] = 0x70;
    bytes[8] = 0x80;
    HistoryPruneCandidate {
        job_id: Id::from_v7_bytes(bytes),
        position: du_core_contracts::job::HistoryPrunePosition {
            reckon_at_ms: i64::from(seed),
            record_seq: u64::from(seed),
        },
        has_nonterminal_follow_up: false,
        row_count,
    }
}

#[test]
fn batch_obeys_row_and_job_limits_and_keeps_one_oversized_job_alone() -> Result<(), String> {
    let below_limit = select_batch(vec![candidate(1, 30_000), candidate(2, 20_000)])
        .map_err(|_| "the first 50,000 rows should fit".to_owned())?;
    assert_eq!(below_limit.len(), 2);

    let over_limit = select_batch(vec![candidate(1, 30_001), candidate(2, 20_000)])
        .map_err(|_| "the first job should fit in its own batch".to_owned())?;
    assert_eq!(over_limit.len(), 1);

    let oversized = select_batch(vec![candidate(1, 50_001), candidate(2, 1)])
        .map_err(|_| "an oversized job should form its own batch".to_owned())?;
    assert_eq!(oversized.len(), 1);
    assert_eq!(oversized[0].row_count, 50_001);

    let capped = select_batch(
        (0..=MAX_BATCH_JOBS)
            .map(|index| candidate(index as u8, 1))
            .collect(),
    )
    .map_err(|_| "the page should be bounded by the job limit".to_owned())?;
    assert_eq!(capped.len(), MAX_BATCH_JOBS);
    Ok(())
}

#[test]
fn zero_row_candidate_is_rejected() {
    assert!(select_batch(vec![candidate(1, 0)]).is_err());
}
