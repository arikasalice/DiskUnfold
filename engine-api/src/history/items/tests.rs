use super::*;

#[test]
fn item_cursor_cannot_cross_sessions_or_history_domains() -> Result<(), String> {
    let job = Id::parse("0195f856-ab83-7a59-9343-10d29775c9a9")?;
    let scope = scope_digest(&job, Some(HistoryItemOutcome::Overwritten));
    let key = [7; 32];
    let cursor = Cursor {
        revision: Revision {
            delete_generation: 8,
            max_record_seq: 9,
        },
        last: 0,
        upper: 3,
    };
    let token = encode(&key, scope, cursor);
    assert_eq!(decode(&key, &token, &scope), Some(cursor));
    assert!(decode(&[8; 32], &token, &scope).is_none());
    assert!(decode(&key, &token, &scope_digest(&job, None)).is_none());
    assert!(super::super::decode(&key, &token, scope).is_err());
    Ok(())
}

#[test]
fn missing_summary_is_unknown_and_reconciliation_does_not_count_partial_pages() {
    let revision = Revision {
        delete_generation: 0,
        max_record_seq: 1,
    };
    let response = project(
        Page {
            revision,
            rows: vec![(8, None)],
            next: None,
            summary: None,
            recorded: 10,
            item_history_expected: true,
        },
        None,
    );
    assert_eq!(response.counts.recorded, "10");
    assert!(response.counts.summary.is_none());
    assert!(response.counts.unavailable.is_none());
    assert_eq!(
        response.missing_reasons,
        ["read_failed", "items_incomplete"]
    );
    assert_eq!(response.completeness, HistoryDetailCompleteness::Partial);
}
