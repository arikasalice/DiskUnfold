//! Exercise shared search paging, folder paging, and totals through registered commands.

use super::*;
use du_analysis_index::{
    Observation,
    query::{self, Basis, Condition, Kind, Predicate, Registry, Sort},
};
use du_core_contracts::{SizeAmount, SizeBasis, SizeEvidence, SizeValue};

#[test]
fn registered_page_and_space_total_share_the_same_search_admission()
-> Result<(), Box<dyn std::error::Error>> {
    let store = IngestSink::default();
    let mut scan = store
        .open(Ids.next_id())
        .map_err(|error| format!("{error:?}"))?;
    scan.add_directory(vec![])
        .map_err(|error| format!("{error:?}"))?;
    for (name, bytes) in [(b"a", 4_u64), (b"b", 6)] {
        scan.add_file(Observation {
            path: vec![name.to_vec()],
            size: Some(SizeValue {
                basis: SizeBasis::Logical,
                evidence: SizeEvidence::Measured,
                amount: SizeAmount::Exact {
                    bytes: bytes.into(),
                },
            }),
        })
        .map_err(|error| format!("{error:?}"))?;
    }
    let version = Ids.next_id();
    let pinned = scan
        .seal(version.clone(), ResultCompleteness::Complete)
        .map_err(|error| format!("{error:?}"))?;
    let search = query::run(
        &Registry::baseline(),
        pinned,
        &Condition::Predicate(Predicate::Kind(Kind::File)),
        Sort {
            id: "sort.path",
            revision: 1,
            basis: Basis::Logical,
            descending: false,
        },
        || false,
    )
    .map_err(|error| format!("{error:?}"))?;
    let sets = Arc::new(ResultSets::new(store));
    let search_ref = Ids.next_id();
    sets.register(search_ref.clone(), search)
        .map_err(|error| format!("{error:?}"))?;
    let (engine, _) = register_fixture(sets.clone())?;
    let gui = |name, body| call(&engine, name, body, Channel::Gui);
    let first = gui(
        "query.page",
        json!({"queryRef": search_ref, "resultVersion": version, "start": 0, "count": 1}),
    );
    assert!(first.error.is_none(), "{:?}", first.error);
    assert_eq!(first.body["rowKind"], "search");
    assert_eq!(first.body["queryRef"], json!(search_ref));
    assert_eq!(first.body["rows"][0]["item"]["itemNo"], 1);
    assert!(first.body.get("browseContext").is_none());
    let cursor = first.body["nextCursor"]
        .as_str()
        .ok_or("missing search cursor")?;
    let second = gui("query.page", json!({"nextCursor": cursor, "count": 1}));
    assert!(second.error.is_none(), "{:?}", second.error);
    assert_eq!(second.body["rowKind"], "search");
    assert_eq!(second.body["queryRef"], json!(search_ref));
    assert_eq!(second.body["rows"][0]["item"]["itemNo"], 2);
    assert_eq!(second.body["nextCursor"], Value::Null);
    let total = gui(
        "space.total",
        json!({"resultVersion": version, "requestSeq": 1,
            "target": {"kind": "query_result", "queryRef": search_ref}}),
    );
    assert!(total.error.is_none(), "{:?}", total.error);
    assert_eq!(total.body["sizes"][0]["size"]["bytes"], "10");

    let altered = gui("query.page", json!({"nextCursor": format!("{cursor}x")}));
    assert_eq!(
        altered.error.ok_or("tampered cursor accepted")?.reason_key,
        "cursor.invalid"
    );
    sets.release(&search_ref)
        .map_err(|error| format!("{error:?}"))?;
    let retired = gui("query.page", json!({"nextCursor": cursor}));
    assert_eq!(
        retired.error.ok_or("retired cursor accepted")?.reason_key,
        "cursor.stale"
    );
    let folder = gui(
        "browse.open",
        json!({"viewKind": "list", "requestSeq": 2,
        "resultVersion": version, "pageSize": 1}),
    );
    assert!(folder.error.is_none(), "{:?}", folder.error);
    assert_eq!(folder.body["page"]["rowKind"], "folder");
    Ok(())
}
