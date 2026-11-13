use super::*;

const MIB: u64 = 1024 * 1024;
const DAY: i64 = 86_400_000;
fn row(id: u8, time: Option<i64>, scope: &[u8], bytes: u64) -> Snapshot {
    let mut key = [0; 16];
    key[15] = id;
    Snapshot {
        id: key,
        scope_label: scope.to_vec(),
        observed_ms: time,
        bytes,
        pinned: false,
        catalog_referenced: false,
    }
}
fn ids(values: &[[u8; 16]]) -> Vec<u8> {
    values.iter().map(|id| id[15]).collect()
}

#[test]
fn default_and_disabled_policy_never_submit_cleanup() -> Result<(), Error> {
    let rows = [
        row(1, Some(0), b"x", 300 * MIB),
        row(2, Some(1), b"x", 300 * MIB),
    ];
    let default = evaluate(&Policy::default(), 10, &rows)?;
    assert_eq!(default.status, Status::NoLimits);
    assert!(default.planned.is_empty());
    assert!(!default.should_submit_prune);
    let preview = evaluate(
        &Policy {
            count: Some(1),
            ..Policy::default()
        },
        10,
        &rows,
    )?;
    assert_eq!(preview.status, Status::OverLimit);
    assert_eq!(ids(&preview.planned), vec![1]);
    assert!(!preview.should_submit_prune);
    Ok(())
}

#[test]
fn deterministic_tie_break_and_combined_limits() -> Result<(), Error> {
    let policy = Policy {
        revision: 9,
        automatic: true,
        count: Some(2),
        bytes: Some(600 * MIB),
        age_days: None,
    };
    let mut rows = vec![
        row(3, Some(1), b"b", 300 * MIB),
        row(2, Some(1), b"a", 300 * MIB),
        row(1, Some(1), b"a", 300 * MIB),
        row(4, Some(2), b"a", 300 * MIB),
    ];
    let decision = evaluate(&policy, 5, &rows)?;
    assert_eq!(ids(&decision.planned), vec![1, 2]);
    assert_eq!(decision.remaining_excess, Excess::default());
    assert!(decision.should_submit_prune);
    rows.reverse();
    assert_eq!(evaluate(&policy, 5, &rows)?, decision);
    Ok(())
}

#[test]
fn protects_and_excludes_in_priority_order_and_counts_them_in_totals() -> Result<(), Error> {
    let mut pinned = row(1, None, b"a", 200 * MIB);
    pinned.pinned = true;
    pinned.catalog_referenced = true;
    let mut catalog = row(2, Some(-10 * DAY), b"a", 200 * MIB);
    catalog.catalog_referenced = true;
    let rows = vec![
        pinned,
        catalog,
        row(3, None, b"a", 200 * MIB),
        row(4, Some(1), b"a", 200 * MIB),
        row(5, Some(-5 * DAY), b"a", 200 * MIB),
    ];
    let policy = Policy {
        automatic: true,
        count: Some(1),
        bytes: Some(256 * MIB),
        age_days: Some(1),
        ..Policy::default()
    };
    let result = evaluate(&policy, 0, &rows)?;
    assert_eq!(ids(&result.planned), vec![5]);
    assert_eq!(
        result
            .protected_or_excluded
            .iter()
            .map(|item| item.reason)
            .collect::<Vec<_>>(),
        vec![
            Classification::Pinned,
            Classification::Catalog,
            Classification::UnknownTime,
            Classification::UnknownTime
        ]
    );
    assert_eq!(result.remaining_excess.count, 3);
    assert_eq!(
        result.remaining_excess.bytes,
        u128::from(800 * MIB - 256 * MIB)
    );
    assert_eq!(result.remaining_excess.expired, 1);
    assert_eq!(result.unknown_age_count, 3);
    assert_eq!(
        result.planned_totals,
        Totals {
            count: 1,
            bytes: u128::from(200 * MIB)
        }
    );
    assert_eq!(
        result
            .reason_totals
            .iter()
            .map(|row| row.totals.count)
            .collect::<Vec<_>>(),
        vec![1, 1, 2]
    );
    assert!(result.protected_shortfall);
    Ok(())
}

#[test]
fn exact_age_and_capacity_boundaries_are_not_over_limit() -> Result<(), Error> {
    let policy = Policy {
        count: Some(1),
        bytes: Some(256 * MIB),
        age_days: Some(1),
        ..Policy::default()
    };
    let exact = [row(1, Some(0), b"x", 256 * MIB)];
    let result = evaluate(&policy, DAY, &exact)?;
    assert_eq!(result.status, Status::Within);
    assert!(result.planned.is_empty());
    let result = evaluate(&policy, DAY + 1, &exact)?;
    assert_eq!(ids(&result.planned), vec![1]);
    Ok(())
}

#[test]
fn rejects_invalid_limits_and_duplicate_ids() {
    for policy in [
        Policy {
            count: Some(0),
            ..Policy::default()
        },
        Policy {
            count: Some(10_001),
            ..Policy::default()
        },
        Policy {
            bytes: Some(256 * MIB - 1),
            ..Policy::default()
        },
        Policy {
            bytes: Some(MAX_BYTES + 1),
            ..Policy::default()
        },
        Policy {
            age_days: Some(0),
            ..Policy::default()
        },
        Policy {
            age_days: Some(3_651),
            ..Policy::default()
        },
    ] {
        assert_eq!(evaluate(&policy, 0, &[]), Err(Error::InvalidPolicy));
    }
    let rows = [row(1, Some(0), b"a", 0), row(1, Some(0), b"b", 0)];
    assert_eq!(
        evaluate(&Policy::default(), 0, &rows),
        Err(Error::DuplicateId)
    );
}

#[test]
fn extreme_observation_times_do_not_overflow() -> Result<(), Error> {
    let rows = [
        row(1, Some(i64::MIN), b"x", 1),
        row(2, Some(i64::MAX), b"x", 1),
    ];
    let policy = Policy {
        age_days: Some(3_650),
        bytes: Some(MAX_BYTES),
        ..Policy::default()
    };
    let result = evaluate(&policy, 0, &rows)?;
    assert_eq!(ids(&result.planned), vec![1]);
    assert_eq!(result.unknown_age_count, 1);
    Ok(())
}
