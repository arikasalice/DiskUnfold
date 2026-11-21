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
    assert_eq!(
        response.item_recording,
        Some(HistoryItemRecording::Recorded)
    );
}

#[test]
fn summary_only_page_is_not_recorded_and_rejects_item_evidence() {
    let page = |recorded, rows, summary| Page {
        revision: Revision {
            delete_generation: 0,
            max_record_seq: 1,
        },
        rows,
        next: None,
        summary,
        recorded,
        item_history_expected: false,
    };
    assert!(validate(&page(0, Vec::new(), None), None, 10).is_ok());
    let response = project(page(0, Vec::new(), None), None);
    assert_eq!(
        response.item_recording,
        Some(HistoryItemRecording::NotRecorded)
    );
    assert!(response.missing_reasons.is_empty());
    assert_eq!(response.completeness, HistoryDetailCompleteness::Complete);
    for invalid in [
        page(1, Vec::new(), None),
        page(1, vec![(0, None)], None),
        page(0, Vec::new(), Some(0)),
    ] {
        assert!(validate(&invalid, None, 10).is_err());
    }
}

#[test]
fn recorded_item_history_reconciles_terminal_counts_including_zero_items() {
    let revision = Revision {
        delete_generation: 0,
        max_record_seq: 1,
    };
    let incomplete = project(
        Page {
            revision,
            rows: vec![(0, None)],
            next: None,
            summary: Some(3),
            recorded: 1,
            item_history_expected: true,
        },
        None,
    );
    assert_eq!(
        incomplete.item_recording,
        Some(HistoryItemRecording::Recorded)
    );
    assert_eq!(incomplete.counts.summary.as_deref(), Some("3"));
    assert_eq!(incomplete.counts.recorded, "1");
    assert_eq!(incomplete.counts.unavailable.as_deref(), Some("2"));

    let zero = project(
        Page {
            revision,
            rows: Vec::new(),
            next: None,
            summary: Some(0),
            recorded: 0,
            item_history_expected: true,
        },
        None,
    );
    assert_eq!(zero.item_recording, Some(HistoryItemRecording::Recorded));
    assert_eq!(zero.counts.summary.as_deref(), Some("0"));
    assert_eq!(zero.counts.recorded, "0");
    assert_eq!(zero.counts.unavailable.as_deref(), Some("0"));
}
