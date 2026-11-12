use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

use du_analysis_index::{IngestSink, Observation, visualization::bundle};
use du_core_contracts::{
    CallEnvelope, ENVELOPE_VERSION, ErrorCode, Id, ItemRef, PathBytes, PathEncoding,
    result::{
        ResultCompleteness, ResultItem, ResultNativePath, ResultScopeRoot, ResultVersionRead,
    },
    visualization::request::{VizRenderRequest, VizSourceRef},
};
use du_engine_api::{
    api::{CallerCtx, Channel, EngineBuilder},
    current_analysis::CurrentAnalysis,
    error::EngineError,
    required_data::{Evidence, FactValues},
    viz_availability, viz_catalog,
    viz_runtime::{EvidenceProducer, ItemEvidence},
};
use serde_json::{Value, json};

struct Provider {
    facts: FactValues,
    availability_supported: bool,
    calls: AtomicUsize,
    scopes: Mutex<Vec<ItemRef>>,
}

impl Provider {
    fn new(facts: FactValues) -> Self {
        Self {
            facts,
            availability_supported: true,
            calls: AtomicUsize::new(0),
            scopes: Mutex::new(Vec::new()),
        }
    }

    fn render_only(facts: FactValues) -> Self {
        Self {
            facts,
            availability_supported: false,
            calls: AtomicUsize::new(0),
            scopes: Mutex::new(Vec::new()),
        }
    }
}

impl EvidenceProducer for Provider {
    fn supports_required_data_availability(&self) -> bool {
        self.availability_supported
    }

    fn required_data_facts_for_scope(
        &self,
        _: &CurrentAnalysis,
        aggregate_scope_ref: &ItemRef,
    ) -> Result<FactValues, EngineError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.scopes
            .lock()
            .map_err(|_| EngineError::new("support.internal_error", vec![]))?
            .push(aggregate_scope_ref.clone());
        Ok(self.facts.clone())
    }

    fn required_data_facts(
        &self,
        _: &CurrentAnalysis,
        _: &VizRenderRequest,
    ) -> Result<Option<FactValues>, EngineError> {
        Ok(Some(self.facts.clone()))
    }

    fn child(
        &self,
        _: &CurrentAnalysis,
        _: &ResultItem,
        _: &VizRenderRequest,
    ) -> Result<ItemEvidence, EngineError> {
        Err(EngineError::new("ref.unavailable", vec![]))
    }
}

fn id(n: u32) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}

fn root() -> ResultScopeRoot {
    ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(b"/"),
        },
        volume_id: vec![1],
        file_id: vec![1],
    }
}

fn publish(
    store: &IngestSink,
    lineage: &Id,
    scope_revision: u32,
    result_revision: u32,
) -> Result<Id, String> {
    store
        .bind_current_analysis("viz-availability-test", lineage.clone())
        .map_err(|error| format!("bind lineage: {error:?}"))?;
    let mut build = store
        .open_forest_in_lineage(id(scope_revision)?, vec![root()], lineage.clone())
        .map_err(|error| format!("open: {error:?}"))?;
    build
        .add_directory(vec![])
        .map_err(|error| format!("directory: {error:?}"))?;
    build
        .add_file(Observation {
            path: vec![b"file".to_vec()],
            size: None,
        })
        .map_err(|error| format!("file: {error:?}"))?;
    let version = build
        .seal(id(result_revision)?, ResultCompleteness::Complete)
        .map_err(|error| format!("seal: {error:?}"))?;
    Ok(version.version_id().clone())
}

fn known_facts() -> FactValues {
    FactValues {
        folder_hierarchy: Evidence::Known(()),
        item_sizes: Evidence::Known(()),
        numeric_values: Evidence::Known(()),
        trend_observations: Evidence::Known(2),
        comparison: Evidence::Absent,
        histogram_bins: Evidence::Known(()),
        attribution: Evidence::Absent,
    }
}

fn call(
    engine: &du_engine_api::api::Engine,
    channel: Channel,
    body: Value,
) -> du_core_contracts::ResultEnvelope {
    let correlation_id = id(99).expect("fixed test identifier is valid");
    engine.call(
        CallEnvelope {
            name: "viz.availability".into(),
            contract_version: ENVELOPE_VERSION,
            correlation_id: correlation_id.clone(),
            body,
        },
        &CallerCtx::new(channel, correlation_id, None),
    )
}

fn request(lineage: &Id, revision: &Id, scope_revision: &Id) -> Value {
    request_scope(lineage, revision, scope_revision, 0)
}

fn request_scope(lineage: &Id, revision: &Id, scope_revision: &Id, scope_item: u32) -> Value {
    json!({
        "sourceRef":{"kind":"scan","ref":lineage},
        "resultRevision":revision,
        "aggregateScopeRef":{"resultVersion":scope_revision,"itemNo":scope_item}
    })
}

fn engine(
    store: &IngestSink,
    provider: Arc<dyn EvidenceProducer>,
) -> Result<(du_engine_api::api::Engine, Vec<String>), String> {
    let dispatcher =
        bundle::default_dispatcher().map_err(|error| format!("dispatcher: {error:?}"))?;
    let formats = viz_catalog::snapshot(&dispatcher)
        .map_err(|error| format!("snapshot: {error:?}"))?
        .formats;
    let mut builder = EngineBuilder::default();
    viz_availability::register(&mut builder, store.clone(), &dispatcher, 1, provider)
        .map_err(|error| format!("{error:?}"))?;
    Ok((builder.build().map_err(|error| error.to_string())?, formats))
}

#[test]
fn engine_entry_preserves_frozen_registry_order_and_known_absence_reasons() -> Result<(), String> {
    let store = IngestSink::default();
    let lineage = id(1)?;
    let revision = publish(&store, &lineage, 2, 3)?;
    let provider = Arc::new(Provider::new(known_facts()));
    let (engine, registered_formats) = engine(&store, provider.clone())?;

    let reply = call(
        &engine,
        Channel::Gui,
        request(&lineage, &revision, &revision),
    );
    assert!(reply.error.is_none());
    let actual = reply.body["formats"]
        .as_array()
        .ok_or("missing format results")?;
    let actual_formats: Vec<_> = actual
        .iter()
        .map(|format| format["format"].as_str().ok_or("missing format key"))
        .collect::<Result<_, _>>()?;
    assert_eq!(
        actual_formats,
        registered_formats
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
    );
    assert_eq!(reply.body["requiredDataTableRevision"], 1);
    assert_eq!(
        actual.iter().find(|format| format["format"] == "waterfall"),
        Some(&json!({
            "format":"waterfall",
            "available":false,
            "reasonKey":"viz.unavailable.no_baseline"
        }))
    );
    assert_eq!(
        actual.iter().find(|format| format["format"] == "sankey"),
        Some(&json!({
            "format":"sankey",
            "available":false,
            "reasonKey":"viz.unavailable.no_attribution"
        }))
    );
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        provider
            .scopes
            .lock()
            .map_err(|_| "provider lock poisoned")?
            .as_slice(),
        &[ItemRef {
            result_version: revision,
            item_no: 0,
        }]
    );
    Ok(())
}

#[test]
fn engine_entry_rejects_a_stale_revision_from_the_same_lineage() -> Result<(), String> {
    let store = IngestSink::default();
    let lineage = id(10)?;
    let first = publish(&store, &lineage, 11, 12)?;
    let _second = publish(&store, &lineage, 13, 14)?;
    let provider = Arc::new(Provider::new(known_facts()));
    let (engine, _) = engine(&store, provider.clone())?;

    let reply = call(&engine, Channel::Gui, request(&lineage, &first, &first));
    let error = reply.error.ok_or("stale revision was accepted")?;
    assert_eq!(error.code, ErrorCode::StaleVersion);
    assert_eq!(error.reason_key, "result.version_stale");
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    Ok(())
}

#[test]
fn engine_entry_rejects_a_scope_from_another_revision_as_invalid_argument() -> Result<(), String> {
    let store = IngestSink::default();
    let lineage = id(20)?;
    let revision = publish(&store, &lineage, 21, 22)?;
    let other_revision = id(23)?;
    let provider = Arc::new(Provider::new(known_facts()));
    let (engine, _) = engine(&store, provider.clone())?;

    let reply = call(
        &engine,
        Channel::Gui,
        request(&lineage, &revision, &other_revision),
    );
    let error = reply.error.ok_or("mismatched folder scope was accepted")?;
    assert_eq!(error.code, ErrorCode::InvalidArgument);
    assert_eq!(error.reason_key, "command.body_invalid");
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    Ok(())
}

#[test]
fn engine_entry_rejects_a_file_as_the_aggregate_folder_scope() -> Result<(), String> {
    let store = IngestSink::default();
    let lineage = id(24)?;
    let revision = publish(&store, &lineage, 25, 26)?;
    let provider = Arc::new(Provider::new(known_facts()));
    let (engine, _) = engine(&store, provider.clone())?;

    let reply = call(
        &engine,
        Channel::Gui,
        request_scope(&lineage, &revision, &revision, 1),
    );
    let error = reply
        .error
        .ok_or("file item was accepted as a folder scope")?;
    assert_eq!(error.code, ErrorCode::InvalidArgument);
    assert_eq!(error.reason_key, "browse.not_folder");
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    Ok(())
}

#[test]
fn unknown_evidence_is_internal_and_never_reported_as_absence() -> Result<(), String> {
    let store = IngestSink::default();
    let lineage = id(30)?;
    let revision = publish(&store, &lineage, 31, 32)?;
    let facts = FactValues {
        folder_hierarchy: Evidence::Unknown,
        ..known_facts()
    };
    let provider = Arc::new(Provider::new(facts));
    let (engine, _) = engine(&store, provider.clone())?;

    let reply = call(
        &engine,
        Channel::Gui,
        request(&lineage, &revision, &revision),
    );
    let error = reply
        .error
        .ok_or("unknown evidence produced a public status")?;
    assert_eq!(error.code, ErrorCode::Internal);
    assert_eq!(error.reason_key, "support.internal_error");
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    Ok(())
}

#[test]
fn engine_entry_is_gui_only_and_missing_source_is_not_found() -> Result<(), String> {
    let store = IngestSink::default();
    let lineage = id(40)?;
    let revision = publish(&store, &lineage, 41, 42)?;
    let provider = Arc::new(Provider::new(known_facts()));
    let (engine, _) = engine(&store, provider.clone())?;
    let body = request(&lineage, &revision, &revision);

    for channel in [Channel::Cli, Channel::Mcp] {
        let reply = call(&engine, channel, body.clone());
        let error = reply.error.ok_or("non-GUI call was accepted")?;
        assert_eq!(error.reason_key, "command.not_provided");
    }
    let missing = id(43)?;
    let reply = call(
        &engine,
        Channel::Gui,
        request(&missing, &revision, &revision),
    );
    let error = reply.error.ok_or("missing source lineage was accepted")?;
    assert_eq!(error.code, ErrorCode::NotFound);
    assert_eq!(error.reason_key, "ref.unavailable");
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    Ok(())
}

#[test]
fn registration_requires_the_published_required_data_table_revision() -> Result<(), String> {
    let store = IngestSink::default();
    let lineage = id(50)?;
    let revision = publish(&store, &lineage, 51, 52)?;
    let dispatcher = bundle::default_dispatcher().map_err(|error| format!("{error:?}"))?;
    let mut builder = EngineBuilder::default();
    assert_eq!(
        viz_availability::register(
            &mut builder,
            store,
            &dispatcher,
            2,
            Arc::new(Provider::new(known_facts())),
        ),
        Err(viz_availability::RegistrationError::RequiredDataRevisionMismatch)
    );
    assert_eq!(revision, id(52)?);
    Ok(())
}

#[test]
fn registration_requires_the_render_owner_to_opt_into_scope_evidence() -> Result<(), String> {
    let store = IngestSink::default();
    let dispatcher = bundle::default_dispatcher().map_err(|error| format!("{error:?}"))?;
    let mut builder = EngineBuilder::default();
    assert_eq!(
        viz_availability::register(
            &mut builder,
            store,
            &dispatcher,
            1,
            Arc::new(Provider::render_only(known_facts())),
        ),
        Err(viz_availability::RegistrationError::EvidenceNotSupported)
    );
    Ok(())
}

#[test]
fn request_source_kind_is_not_inferred_from_revision_only() -> Result<(), String> {
    let store = IngestSink::default();
    let lineage = id(60)?;
    let revision = publish(&store, &lineage, 61, 62)?;
    let provider = Arc::new(Provider::new(known_facts()));
    let (engine, _) = engine(&store, provider.clone())?;
    let mut body = request(&lineage, &revision, &revision);
    body["sourceRef"]["kind"] = json!("snapshot");

    let reply = call(&engine, Channel::Gui, body);
    let error = reply.error.ok_or("unsupported source kind was accepted")?;
    assert_eq!(error.code, ErrorCode::NotFound);
    assert_eq!(error.reason_key, "ref.unavailable");
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    let _: VizSourceRef = serde_json::from_value(json!({"kind":"scan","ref":lineage}))
        .map_err(|error| error.to_string())?;
    Ok(())
}
