use std::sync::Arc;

use du_analysis_index::{
    IngestSink, Observation,
    query::{self, Basis, Condition, Predicate, Registry, Sort, page::ResultSets},
};
use du_core_contracts::{
    CallEnvelope, ENVELOPE_VERSION, Id, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    result::ResultCompleteness,
};
use du_engine_api::{
    api::{CallerCtx, Channel, EngineBuilder},
    space,
};
use serde_json::json;

fn id(n: u8) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}

fn sort() -> Sort<'static> {
    Sort {
        id: "sort.path",
        revision: 1,
        basis: Basis::Logical,
        descending: false,
    }
}

#[test]
fn registered_total_consumes_only_the_admitted_search_snapshot()
-> Result<(), Box<dyn std::error::Error>> {
    let store = IngestSink::default();
    let mut build = store.open(id(1)?).map_err(|e| format!("{e:?}"))?;
    build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    for (name, bytes) in [(b"one".as_slice(), 7), (b"two", 23)] {
        build
            .add_file(Observation {
                path: vec![name.to_vec()],
                size: Some(SizeValue {
                    basis: SizeBasis::Logical,
                    evidence: SizeEvidence::Measured,
                    amount: SizeAmount::Exact {
                        bytes: bytes.into(),
                    },
                }),
            })
            .map_err(|e| format!("{e:?}"))?;
    }
    let version = build
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let selected = query::run(
        &Registry::baseline(),
        version.clone(),
        &Condition::Predicate(Predicate::Name(
            query::NameTest::Equal(b"one".to_vec()),
            query::NameEncoding::Bytes,
        )),
        sort(),
        || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    let folder = query::run_folder(
        &Registry::baseline(),
        version.clone(),
        None,
        &Condition::All(vec![]),
        sort(),
        || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    let unknown = query::run(
        &Registry::baseline(),
        version,
        &Condition::All(vec![]),
        sort(),
        || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    let sets = Arc::new(ResultSets::new(store.clone()));
    sets.register(id(3)?, selected)
        .map_err(|e| format!("{e:?}"))?;
    sets.register(id(4)?, folder)
        .map_err(|e| format!("{e:?}"))?;
    sets.register(id(5)?, unknown)
        .map_err(|e| format!("{e:?}"))?;
    let mut builder = EngineBuilder::default();
    space::register_total_with_queries(&mut builder, sets.clone());
    let engine = builder.build()?;
    let call = |reference: Id, result_version: Option<Id>, caller: &CallerCtx| {
        engine.call(
            CallEnvelope {
                name: "space.total".into(),
                contract_version: ENVELOPE_VERSION,
                correlation_id: caller.correlation_id.clone(),
                body: json!({
                    "target":{"kind":"query_result","queryRef":reference},
                    "resultVersion":result_version, "requestSeq":9
                }),
            },
            caller,
        )
    };
    let caller = CallerCtx::new(Channel::Gui, id(8)?, None);
    let response = call(id(3)?, None, &caller);
    assert!(response.error.is_none(), "{:?}", response.error);
    assert_eq!(response.body["requestSeq"], 9);
    assert_eq!(response.body["resultVersion"], json!(id(2)?));
    assert_eq!(response.body["sizes"][0]["size"]["bytes"], "7");
    assert_eq!(
        call(id(4)?, None, &caller)
            .error
            .ok_or("folder accepted")?
            .reason_key,
        "space.target_out_of_scope"
    );
    let missing = call(id(5)?, None, &caller);
    assert_eq!(missing.body["sizes"][0]["size"]["state"], "unknown");
    assert_eq!(
        call(id(3)?, Some(id(9)?), &caller)
            .error
            .ok_or("wrong version accepted")?
            .reason_key,
        "result.requery_required"
    );
    let cancelled = CallerCtx::new(Channel::Gui, id(8)?, None);
    cancelled.cancel();
    assert!(call(id(3)?, None, &cancelled).error.is_some());
    let admitted = sets
        .open_search(&id(3)?, None)
        .map_err(|e| format!("{e:?}"))?;
    sets.release(&id(3)?).map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        admitted
            .space_total(|| false)
            .map_err(|e| format!("{e:?}"))?
            .logical
            .amount,
        SizeAmount::Exact {
            bytes: 7_u64.into()
        }
    );
    assert_eq!(
        call(id(3)?, None, &caller)
            .error
            .ok_or("released query accepted")?
            .reason_key,
        "result.requery_required"
    );
    let mut replacement = store.open(id(10)?).map_err(|e| format!("{e:?}"))?;
    replacement
        .add_directory(vec![])
        .map_err(|e| format!("{e:?}"))?;
    replacement
        .seal(id(11)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        call(id(5)?, None, &caller)
            .error
            .ok_or("old query accepted")?
            .reason_key,
        "result.requery_required"
    );
    assert_eq!(
        admitted
            .space_total(|| false)
            .map_err(|e| format!("{e:?}"))?
            .logical
            .amount,
        SizeAmount::Exact {
            bytes: 7_u64.into()
        }
    );
    Ok(())
}
