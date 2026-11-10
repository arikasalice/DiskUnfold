use super::*;
use du_analysis_index::{Observation, SharingEvidence, SpaceObservation};
use du_core_contracts::{
    ByteSize, CallEnvelope, ENVELOPE_VERSION, Id, ItemRef, PathBytes,
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot},
};
use serde_json::{Value, json};

fn id(n: u8) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}

fn wide(value: &str) -> Vec<u8> {
    value.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

fn root(encoding: PathEncoding, raw: &[u8]) -> ResultScopeRoot {
    ResultScopeRoot {
        path: ResultNativePath {
            encoding,
            raw: PathBytes::from_bytes(raw),
        },
        volume_id: vec![1],
        file_id: vec![2],
    }
}

fn call_detail(
    engine: &crate::api::Engine,
    version: &Id,
    item_version: &Id,
    item_no: u32,
) -> Result<du_core_contracts::ResultEnvelope, String> {
    let correlation_id = id(9)?;
    let caller = CallerCtx::new(Channel::Gui, correlation_id.clone(), None);
    Ok(engine.call(CallEnvelope {
        name: "browse.detail".into(),
        contract_version: ENVELOPE_VERSION,
        correlation_id,
        body: json!({"resultVersion": version, "item": {"resultVersion": item_version, "itemNo": item_no}}),
    }, &caller))
}

#[test]
fn detail_uses_each_pinned_root_encoding_and_preserves_field_states() -> Result<(), String> {
    let sink = IngestSink::default();
    let version = id(2)?;
    let mut build = sink
        .open_forest(
            id(1)?,
            vec![
                root(PathEncoding::WindowsUtf16le, &wide("C:\\첫째")),
                root(PathEncoding::Unix, "/둘째".as_bytes()),
            ],
        )
        .map_err(|e| format!("{e:?}"))?;
    for index in 0..2 {
        build
            .add_directory_at(index, vec![])
            .map_err(|e| format!("{e:?}"))?;
    }
    let known = SizeValue {
        basis: SizeBasis::Logical,
        amount: SizeAmount::Exact {
            bytes: ByteSize::from(17),
        },
        evidence: SizeEvidence::Measured,
    };
    let entries = [
        (0, wide("한글"), Some(known)),
        (0, vec![0x00, 0xd8], None),
        (0, vec![0x61], None),
        (1, "이름".as_bytes().to_vec(), None),
        (1, vec![0xff], None),
    ];
    for (root_index, name, size) in &entries {
        let observation = Observation {
            path: vec![name.clone()],
            size: size.clone(),
        };
        if size.is_some() {
            build.add_with_space_at(
                *root_index,
                observation,
                SpaceObservation {
                    allocated: Some(SizeValue {
                        basis: SizeBasis::Allocated,
                        amount: SizeAmount::Exact {
                            bytes: ByteSize::from(32),
                        },
                        evidence: SizeEvidence::OsReported,
                    }),
                    identity: None,
                    link_count: None,
                    sharing: SharingEvidence::None,
                },
            )
        } else {
            build.add_file_at(*root_index, observation)
        }
        .map_err(|e| format!("{e:?}"))?;
        if size.is_some() {
            build
                .set_modified_at(
                    *root_index,
                    std::slice::from_ref(name),
                    du_core_contracts::Timestamp::parse("2020-01-02T03:04:05.006Z")?,
                )
                .map_err(|e| format!("{e:?}"))?;
        }
    }
    let pinned = build
        .seal(version.clone(), ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let mut builder = EngineBuilder::default();
    register(&mut builder, sink.clone());
    let engine = builder.build().map_err(|e| e.to_string())?;
    assert_eq!(pinned.items().len(), 7);
    for (index, expected) in [(0, "C:\\첫째"), (1, "/둘째")] {
        let result = call_detail(&engine, &version, &version, index)?;
        assert!(result.error.is_none());
        assert_eq!(result.body["displayName"]["value"], expected);
        assert_eq!(result.body["displayPath"]["value"], expected);
    }
    for item in pinned.items().iter().filter(|item| item.parent.is_some()) {
        let reference = ItemRef {
            result_version: version.clone(),
            item_no: item.item_no,
        };
        let (index, _) = pinned
            .observed_root_for(&reference)
            .map_err(|e| format!("{e:?}"))?;
        let response = call_detail(&engine, &version, &version, item.item_no)?;
        assert!(response.error.is_none());
        check_item(index, item, response.body)?;
    }
    assert_eq!(
        call_detail(&engine, &version, &version, 99)?
            .error
            .ok_or("missing error")?
            .reason_key,
        "ref.unavailable"
    );
    assert_eq!(
        call_detail(&engine, &version, &id(3)?, 0)?
            .error
            .ok_or("missing error")?
            .reason_key,
        "result.requery_required"
    );
    assert_eq!(
        call_detail(&engine, &id(3)?, &id(3)?, 0)?
            .error
            .ok_or("missing error")?
            .reason_key,
        "result.requery_required"
    );
    Ok(())
}

fn check_item(
    index: u32,
    item: &du_core_contracts::result::ResultItem,
    body: Value,
) -> Result<(), String> {
    let (name, path) = match (index, item.name.as_slice()) {
        (0, bytes) if bytes == wide("한글") => (Some("한글"), Some("C:\\첫째\\한글")),
        (0, [0x00, 0xd8]) => (Some("�"), Some("C:\\첫째\\�")),
        (0, [0x61]) => (None, None),
        (1, bytes) if bytes == "이름".as_bytes() => (Some("이름"), Some("/둘째/이름")),
        (1, [0xff]) => (Some("�"), Some("/둘째/�")),
        _ => return Err("unexpected item".into()),
    };
    for (field, expected) in [("displayName", name), ("displayPath", path)] {
        if let Some(value) = expected {
            assert_eq!(body[field]["value"], value);
        } else {
            assert_eq!(body[field]["state"], "unknown");
        }
    }
    assert_eq!(
        body["logicalSize"]["state"],
        if item.size.is_some() {
            "present"
        } else {
            "unknown"
        }
    );
    if item.size.is_some() {
        assert_eq!(body["logicalSize"]["value"]["bytes"], "17");
    }
    if item.size.is_some() {
        assert_eq!(body["allocatedSize"]["value"]["bytes"], "32");
    } else {
        assert_eq!(body["allocatedSize"]["state"], "unknown");
    }
    assert_eq!(
        body["modifiedAt"]["value"].as_str(),
        item.size.as_ref().map(|_| "2020-01-02T03:04:05.006Z")
    );
    assert_eq!(body["createdAt"]["state"], "unknown");
    assert_eq!(body["observedAt"]["state"], "unknown");
    assert_eq!(body["observationOnly"], true);
    assert!(!body.to_string().contains("\"raw\""));
    Ok(())
}
