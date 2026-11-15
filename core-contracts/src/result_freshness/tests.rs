use super::*;
use serde_json::json;

type Check = Result<(), Box<dyn std::error::Error>>;

const LINEAGE: &str = "0195f856-ab83-7a59-9343-10d29775c9a9";
const SCOPE: &str = "0195f856-ab84-7a59-9343-10d29775c9a9";

fn body(changed: FreshnessChanged) -> Result<ResultFreshnessChanged, Box<dyn std::error::Error>> {
    Ok(ResultFreshnessChanged {
        lineage_id: Id::parse(LINEAGE)?,
        scope_version_id: Id::parse(SCOPE)?,
        sequence: 3,
        summary: FreshnessSummary {
            representative: FreshnessState::new(FreshnessState::UNVERIFIABLE),
            counts: FreshnessCounts {
                unwatched: 1,
                unverifiable: 1,
                ..FreshnessCounts::default()
            },
        },
        changed,
        observed_at: None,
    })
}

fn ranges() -> FreshnessChanged {
    FreshnessChanged::Ranges {
        ranges: vec![FreshnessRange {
            root_index: 1,
            state: FreshnessState::new(FreshnessState::UNVERIFIABLE),
            reason_key: Some("scan.denied".into()),
        }],
    }
}

#[test]
fn wire_shape_is_fixed_and_round_trips() -> Check {
    let value = body(ranges())?;
    let expected = json!({
        "lineageId": LINEAGE,
        "scopeVersionId": SCOPE,
        "sequence": 3,
        "summary": {
            "representative": "unverifiable",
            "counts": {
                "observed": 0, "unwatched": 1, "changeNotified": 0,
                "gapUnknown": 0, "refreshing": 0, "unverifiable": 1
            }
        },
        "changed": {
            "kind": "ranges",
            "ranges": [{"rootIndex": 1, "state": "unverifiable", "reasonKey": "scan.denied"}]
        }
    });
    assert_eq!(serde_json::to_value(&value)?, expected);
    assert_eq!(
        serde_json::from_value::<ResultFreshnessChanged>(expected)?,
        value
    );
    let requery = serde_json::to_value(body(FreshnessChanged::Requery)?)?;
    assert_eq!(requery["changed"], json!({"kind": "requery"}));
    Ok(())
}

#[test]
fn receivers_accept_unknown_fields_missing_counts_and_unknown_values() -> Check {
    let parsed: ResultFreshnessChanged = serde_json::from_value(json!({
        "lineageId": LINEAGE, "scopeVersionId": SCOPE, "sequence": 9, "future": true,
        "summary": {
            "representative": "future_state",
            "counts": {"refreshing": 2, "futureCount": 5},
            "extra": 1
        },
        "changed": {
            "kind": "ranges",
            "ranges": [{"rootIndex": 0, "state": "future_state", "future": 1}]
        },
        "observedAt": "2026-11-15T00:00:00.000Z"
    }))?;
    assert_eq!(parsed.summary.representative.as_str(), "future_state");
    assert_eq!(parsed.summary.counts.refreshing, 2);
    assert_eq!(parsed.summary.counts.observed, 0);
    assert_eq!(parsed.summary.counts.total(), 2);
    let FreshnessChanged::Ranges { ranges } = parsed.changed else {
        return Err("expected ranges".into());
    };
    assert_eq!(ranges[0].state.as_str(), "future_state");
    assert_eq!(ranges[0].reason_key, None);
    Ok(())
}

#[test]
fn an_unknown_change_kind_is_read_as_unknown_and_must_be_requeried() -> Check {
    let mut wire = serde_json::to_value(body(FreshnessChanged::Requery)?)?;
    wire["changed"] = json!({"kind": "diff_v2", "patch": []});
    let parsed: ResultFreshnessChanged = serde_json::from_value(wire)?;
    assert_eq!(parsed.changed, FreshnessChanged::Unknown);
    let notice = ResultFreshnessNotice {
        result_version: Some(Id::parse(SCOPE)?),
        body: parsed,
    };
    assert!(!notice.validate(), "producers never emit an unknown kind");
    Ok(())
}

#[test]
fn validate_bounds_sequence_ranges_and_the_current_version() -> Check {
    let current = Some(Id::parse(SCOPE)?);
    let notice = |result_version: Option<Id>, edit: &dyn Fn(&mut ResultFreshnessChanged)| {
        let mut value = body(ranges())?;
        edit(&mut value);
        Ok::<_, Box<dyn std::error::Error>>(ResultFreshnessNotice {
            result_version,
            body: value,
        })
    };
    assert!(notice(current.clone(), &|_| {})?.validate());
    assert!(!notice(current.clone(), &|b| b.sequence = 0)?.validate());
    assert!(notice(current.clone(), &|b| b.sequence = (1 << 53) - 1)?.validate());
    assert!(!notice(current.clone(), &|b| b.sequence = 1 << 53)?.validate());
    assert!(
        !notice(current.clone(), &|b| {
            b.changed = FreshnessChanged::Ranges { ranges: vec![] };
        })?
        .validate()
    );
    let range = FreshnessRange {
        root_index: 0,
        state: FreshnessState::new(FreshnessState::OBSERVED),
        reason_key: None,
    };
    assert!(
        notice(current.clone(), &|b| {
            b.changed = FreshnessChanged::Ranges {
                ranges: vec![range.clone(); MAX_FRESHNESS_RANGES],
            };
        })?
        .validate()
    );
    assert!(
        !notice(current.clone(), &|b| {
            b.changed = FreshnessChanged::Ranges {
                ranges: vec![range.clone(); MAX_FRESHNESS_RANGES + 1],
            };
        })?
        .validate()
    );
    assert!(!notice(current, &|b| b.summary.counts = FreshnessCounts::default())?.validate());
    // No current version: every count is zero, the change is a requery, no observation time.
    let gone = |b: &mut ResultFreshnessChanged| {
        b.summary.counts = FreshnessCounts::default();
        b.changed = FreshnessChanged::Requery;
    };
    assert!(notice(None, &gone)?.validate());
    assert!(!notice(None, &|_| {})?.validate());
    Ok(())
}
