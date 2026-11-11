use super::*;
use du_core_contracts::{CallEnvelope, ENVELOPE_VERSION, ErrorCode};
use du_engine_api::{
    api::{CallerCtx, Channel, EngineBuilder},
    viz_pages::commands,
};
use serde_json::json;

#[test]
fn rows_command_reads_the_session_model_and_rejects_stale_and_invalid_calls() -> Result<(), String>
{
    let sink = IngestSink::default();
    publish(&sink, id(2), id(3))?;
    let session = Arc::new(Session::new(
        sink.clone(),
        NonZeroUsize::MIN,
        Arc::new(Identity(AtomicU8::new(11))),
        bundle::implemented_dispatcher().map_err(|e| format!("{e:?}"))?,
    ));
    let parent = ItemRef {
        result_version: id(3),
        item_no: 0,
    };
    let model = session
        .render_selected(
            request(&parent, 1000),
            |_, item, _| Ok(policy(item)),
            &mut || false,
        )
        .map_err(|e| format!("{e:?}"))?;
    session
        .pin_model(&model.model_ref, &id(3))
        .map_err(|e| format!("{e:?}"))?;
    let mut builder = EngineBuilder::default();
    commands::register_rows(&mut builder, session.clone());
    commands::register_locate(&mut builder, session.clone());
    let engine = builder.build().map_err(|e| e.to_string())?;
    let call = |body, channel| {
        engine.call(
            CallEnvelope {
                name: "viz.rows".into(),
                contract_version: ENVELOPE_VERSION,
                correlation_id: id(20),
                body,
            },
            &CallerCtx::new(channel, id(20), None),
        )
    };
    let body = json!({"modelRef": model.model_ref, "resultRevision": id(3), "start":0, "count":10});
    let expected = session
        .rows(&serde_json::from_value(body.clone()).map_err(|e| format!("{e:?}"))?)
        .map_err(|e| format!("{e:?}"))?;
    let response = call(body.clone(), Channel::Gui);
    assert!(response.error.is_none());
    assert_eq!(
        response.body,
        serde_json::to_value(expected).map_err(|e| format!("{e:?}"))?
    );
    let item = model.model.rows[0].item.clone();
    let locate = |item_ref: &ItemRef| {
        engine.call(
            CallEnvelope {
                name: "viz.locate".into(),
                contract_version: ENVELOPE_VERSION,
                correlation_id: id(20),
                body: json!({"modelRef":model.model_ref,"itemRef":item_ref}),
            },
            &CallerCtx::new(Channel::Gui, id(20), None),
        )
    };
    let location = locate(&item);
    assert!(location.error.is_none());
    assert_eq!(location.body["location"], "shape");
    assert!(location.body["shapeIndex"].is_u64());
    let unknown = ItemRef {
        result_version: id(3),
        item_no: u32::MAX,
    };
    assert_eq!(
        locate(&unknown)
            .error
            .ok_or("unknown item accepted")?
            .reason_key,
        "ref.unavailable"
    );
    let missing = call(
        json!({"modelRef":"missing", "resultRevision":id(3), "start":0, "count":10}),
        Channel::Gui,
    );
    assert_eq!(
        missing.error.ok_or("unknown model accepted")?.reason_key,
        "viz.model_expired"
    );
    assert_eq!(
        call(
            json!({"modelRef": model.model_ref, "resultRevision": id(3), "start":0, "count":0}),
            Channel::Gui
        )
        .error
        .ok_or("invalid accepted")?
        .code,
        ErrorCode::InvalidArgument
    );
    assert_eq!(
        call(body.clone(), Channel::Cli)
            .error
            .ok_or("CLI accepted")?
            .reason_key,
        "command.not_provided"
    );
    let mut mismatched = body.clone();
    mismatched["resultRevision"] = json!(id(5));
    let mismatched_rows = call(mismatched, Channel::Gui)
        .error
        .ok_or("mismatched revision accepted for a current model")?;
    assert!(call(body.clone(), Channel::Gui).error.is_none());

    publish(&sink, id(4), id(5))?;
    let stale_location = locate(&item).error.ok_or("stale locate accepted")?;
    let stale_rows = call(body, Channel::Gui)
        .error
        .ok_or("stale rows accepted")?;
    for error in [mismatched_rows, stale_location, stale_rows] {
        assert_eq!(error.code, ErrorCode::StaleVersion);
        assert_eq!(error.reason_key, "result.version_stale");
    }
    Ok(())
}
