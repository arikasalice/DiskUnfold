//! #155: a folder list of a scan's earlier result after its final one is published.
use super::*;

/// A scan publishes an interim result and then its final one. While the scan
/// runs, the interim result opens; once the final one is published, the
/// interim one is refused with `result.requery_required` and only the final
/// one opens. The analysis panel therefore opens the newest result it knows.
#[test]
fn an_interim_result_opens_only_until_the_final_one_is_published()
-> Result<(), Box<dyn std::error::Error>> {
    let store = IngestSink::default();
    let mut scan = store.open(Ids.next_id()).map_err(|e| format!("{e:?}"))?;
    scan.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    scan.add_directory(vec![b"Photos".to_vec()])
        .map_err(|e| format!("{e:?}"))?;
    let interim = Ids.next_id();
    scan.publish_partial(interim.clone())
        .map_err(|e| format!("{e:?}"))?;
    let (engine, _) = register_fixture(Arc::new(ResultSets::new(store)))?;
    let region_ref = region(&engine);
    let open = |sequence: u32, version: &Id| {
        call(
            &engine,
            "browse.open",
            json!({"regionRef": region_ref, "viewKind": "list", "requestSeq": sequence,
                "resultVersion": version, "pageSize": 10}),
            Channel::Gui,
        )
    };
    let running = open(1, &interim);
    assert!(running.error.is_none(), "{:?}", running.error);
    assert_eq!(running.body["kind"], "opened");

    let final_version = Ids.next_id();
    scan.seal(final_version.clone(), ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let earlier = open(2, &interim);
    assert_eq!(
        earlier.error.ok_or("the earlier result opened")?.reason_key,
        "result.requery_required"
    );
    let newest = open(3, &final_version);
    assert!(newest.error.is_none(), "{:?}", newest.error);
    assert_eq!(newest.body["kind"], "opened");
    assert_eq!(newest.body["page"]["resultVersion"], json!(final_version));
    Ok(())
}
