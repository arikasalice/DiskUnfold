use du_analysis_index::visualization::{
    bundle, registry,
    typed_layout::{self, Dispatcher, Input, Strategy},
};
use du_core_contracts::{
    CallEnvelope, ENVELOPE_VERSION, ErrorCode, Id,
    visualization::{LayoutDescriptor, LayoutStrategyKey},
};
use du_engine_api::{
    api::{CallerCtx, Channel, EngineBuilder},
    viz_catalog::{self, Error, Policy},
};
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

struct NeverCalled(Arc<AtomicUsize>);
impl Strategy for NeverCalled {
    fn layout(
        &self,
        _: &Input,
        _: &mut dyn FnMut() -> bool,
    ) -> Result<typed_layout::Output, typed_layout::Error> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Err(typed_layout::Error::InvalidInput)
    }
}
fn descriptor(format: &str, id: &str, revision: u32) -> LayoutDescriptor {
    LayoutDescriptor {
        format: format.into(),
        strategy: LayoutStrategyKey {
            id: id.into(),
            revision,
        },
    }
}

fn policy() -> Policy {
    Policy {
        measures: vec!["size".into()],
        color_criteria: vec!["type".into(), "size".into()],
        denominator_criteria: vec!["scope".into()],
        interpretation_table_revision: 1,
        required_data_table_revision: 2,
        legend_rule_revision: 3,
    }
}

#[test]
fn gui_catalog_reads_the_registered_formats_and_policy_snapshot()
-> Result<(), Box<dyn std::error::Error>> {
    let dispatcher = bundle::default_dispatcher().map_err(|error| format!("{error:?}"))?;
    let mut builder = EngineBuilder::default();
    viz_catalog::register(&mut builder, &dispatcher, policy())
        .map_err(|error| format!("{error:?}"))?;
    let engine = builder.build()?;
    let correlation = Id::parse("018f0000-0000-7000-8000-000000000001")?;
    let call = |channel, body| {
        engine.call(
            CallEnvelope {
                name: "viz.catalog".into(),
                contract_version: ENVELOPE_VERSION,
                correlation_id: correlation.clone(),
                body,
            },
            &CallerCtx::new(channel, correlation.clone(), None),
        )
    };
    let reply = call(Channel::Gui, json!({}));
    assert!(reply.error.is_none());
    assert_eq!(
        reply.body["formats"],
        json!(
            viz_catalog::snapshot(&dispatcher)
                .map_err(|error| format!("{error:?}"))?
                .formats
        )
    );
    assert_eq!(
        reply.body["strategies"]
            .as_array()
            .ok_or("missing strategy list")?
            .len(),
        6
    );
    assert!(
        reply.body["strategies"].as_array().is_some_and(
            |strategies| strategies.contains(&json!({"id":"viz.treemap","revision":1}))
        )
    );
    assert_eq!(reply.body["measures"], json!(["size"]));
    assert_eq!(reply.body["colorCriteria"], json!(["type", "size"]));
    assert_eq!(reply.body["denominatorCriteria"], json!(["scope"]));
    assert_eq!(reply.body["interpretationTableRevision"], 1);
    assert_eq!(reply.body["requiredDataTableRevision"], 2);
    assert_eq!(reply.body["legendRuleRevision"], 3);
    assert_eq!(
        call(Channel::Gui, json!({"format":"treemap"}))
            .error
            .ok_or("unexpected argument accepted")?
            .code,
        ErrorCode::InvalidArgument
    );
    for channel in [Channel::Cli, Channel::Mcp] {
        assert_eq!(
            call(channel, json!({}))
                .error
                .ok_or("non-GUI call accepted")?
                .reason_key,
            "command.not_provided"
        );
    }
    Ok(())
}

#[test]
fn malformed_policy_does_not_register_a_catalog() -> Result<(), String> {
    let dispatcher = bundle::default_dispatcher().map_err(|error| format!("{error:?}"))?;
    for invalid in [
        Policy {
            measures: vec![],
            ..policy()
        },
        Policy {
            color_criteria: vec![" ".into()],
            ..policy()
        },
        Policy {
            denominator_criteria: vec!["scope".into(), "scope".into()],
            ..policy()
        },
        Policy {
            legend_rule_revision: 0,
            ..policy()
        },
    ] {
        let mut builder = EngineBuilder::default();
        assert_eq!(
            viz_catalog::register(&mut builder, &dispatcher, invalid),
            Err(Error::InvalidPolicy)
        );
        assert!(viz_catalog::register(&mut builder, &dispatcher, policy()).is_ok());
        assert!(builder.build().is_ok());
    }
    Ok(())
}

#[test]
fn catalog_uses_registered_open_formats_and_selects_without_execution() -> Result<(), String> {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut builder = registry::Builder::<Box<dyn Strategy>>::default();
    let entries = [
        descriptor("treemap", "viz.treemap", 1),
        descriptor("treemap", "viz.treemap", 2),
        descriptor("future", "viz.future", 1),
    ];
    for entry in &entries {
        builder
            .register(entry.clone(), Box::new(NeverCalled(calls.clone())))
            .map_err(|e| format!("{e:?}"))?;
    }
    let dispatcher = Dispatcher::new(builder.build(&entries).map_err(|e| format!("{e:?}"))?);
    let result = viz_catalog::snapshot(&dispatcher).map_err(|e| format!("{e:?}"))?;
    assert_eq!(result.formats, ["future", "treemap"]);
    assert_eq!(
        result.strategies,
        [entries[2].clone(), entries[0].clone(), entries[1].clone()]
    );
    assert_eq!(
        viz_catalog::select(&dispatcher, "treemap", None),
        Ok(entries[1].clone())
    );
    assert_eq!(
        viz_catalog::select(&dispatcher, "treemap", Some(&entries[0].strategy)),
        Ok(entries[0].clone())
    );
    assert_eq!(
        viz_catalog::select(&dispatcher, "future", None),
        Ok(entries[2].clone())
    );
    assert_eq!(
        viz_catalog::select(&dispatcher, "unknown", None),
        Err(Error::UnknownFormat)
    );
    assert_eq!(
        viz_catalog::select(&dispatcher, "treemap", Some(&entries[2].strategy)),
        Err(Error::UnknownStrategy)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    Ok(())
}

#[test]
fn empty_registry_is_not_a_successful_catalog() -> Result<(), String> {
    let builder = registry::Builder::<Box<dyn Strategy>>::default();
    let dispatcher = Dispatcher::new(builder.build(&[]).map_err(|e| format!("{e:?}"))?);
    assert_eq!(
        viz_catalog::snapshot(&dispatcher),
        Err(Error::EmptyRegistry)
    );
    assert_eq!(
        viz_catalog::select(&dispatcher, "treemap", None),
        Err(Error::UnknownFormat)
    );
    Ok(())
}

#[test]
fn development_catalog_reports_the_actual_registered_bundle() -> Result<(), String> {
    let dispatcher = bundle::implemented_dispatcher().map_err(|e| format!("{e:?}"))?;
    let result = viz_catalog::snapshot(&dispatcher).map_err(|e| format!("{e:?}"))?;
    assert_eq!(result.formats, ["treemap"]);
    assert_eq!(result.strategies, [typed_layout::treemap_descriptor()]);
    Ok(())
}
