//! #226: the reasons a browse page gives for its lower bounds, on the wire.
use du_core_contracts::{
    browse_ipc::BrowseValue,
    browse_page_ipc::{FolderFlags, UnreadInside},
};
use serde_json::{json, to_value};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn flags(unread_inside: Option<BrowseValue<UnreadInside>>) -> FolderFlags {
    FolderFlags {
        unenumerated: BrowseValue::Present { value: false },
        excluded: BrowseValue::Present { value: false },
        boundary: Some(BrowseValue::Absent),
        unread_inside,
    }
}

#[test]
fn unread_ranges_inside_carry_their_count_only_when_it_is_known() -> TestResult {
    let counted = Some(BrowseValue::Present {
        value: UnreadInside {
            count: Some("3".into()),
        },
    });
    assert_eq!(
        to_value(flags(counted))?["unreadInside"],
        json!({ "state": "present", "value": { "count": "3" } })
    );
    // A folder below was left unread by a mark, so the number is not known.
    let uncounted = Some(BrowseValue::Present {
        value: UnreadInside { count: None },
    });
    assert_eq!(
        to_value(flags(uncounted))?["unreadInside"],
        json!({ "state": "present", "value": {} })
    );
    assert_eq!(
        to_value(flags(Some(BrowseValue::Absent)))?["unreadInside"],
        json!({ "state": "absent" })
    );
    // Omitted when the fact does not apply; an older producer never sends it.
    let omitted = to_value(flags(None))?;
    assert!(omitted.get("unreadInside").is_none());
    let decoded: FolderFlags = serde_json::from_value(omitted)?;
    assert_eq!(decoded.unread_inside, None);
    Ok(())
}
