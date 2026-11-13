//! Exercises the desktop's shared scan and visualization composition on Windows.
use super::*;
use du_core_contracts::{
    result::{ResultItem, ResultVersionRead},
    visualization::{
        model::{VizMatchKey, VizStaleness, VizStalenessState},
        request::VizRenderRequest,
    },
};
use du_engine_api::{
    current_analysis::CurrentAnalysis,
    desktop::{DesktopEvents, VisualizationPreview},
    error::EngineError,
    viz_catalog,
    viz_pages::session::ChildPolicyEvidence,
    viz_runtime::{
        EvidenceProducer, ItemEvidence, LegendClass, SourcePolicyFacts, ViewPolicy,
        selection::{SelectionError, WindowScanSelection},
    },
};
use std::{
    collections::HashMap,
    fs,
    num::NonZeroUsize,
    sync::{Mutex, mpsc},
};

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

#[derive(Default)]
struct Selection(Mutex<Option<Id>>);

impl WindowScanSelection for Selection {
    fn selected_result_revision(&self, _: &Id) -> Result<Option<Id>, SelectionError> {
        self.0
            .lock()
            .map(|revision| revision.clone())
            .map_err(|_| SelectionError::Unavailable)
    }
}

/// Supplies test-only policies; the scan and its rule revision are real observations.
struct FixtureEvidence(Mutex<Option<mpsc::Receiver<()>>>);

impl EvidenceProducer for FixtureEvidence {
    fn source_policy_facts(
        &self,
        _: &CurrentAnalysis,
        _: &VizRenderRequest,
    ) -> Result<Option<SourcePolicyFacts>, EngineError> {
        Ok(Some(SourcePolicyFacts {
            staleness: VizStaleness {
                state: VizStalenessState::Current,
                reason_key: None,
            },
            interpretation_revision: 1,
            required_data_revision: 1,
            legend_revision: 1,
            view_policy_revision: 1,
            classes: HashMap::from([(
                "fixture.class".into(),
                LegendClass {
                    label_key: "fixture.label".into(),
                    condition: None,
                },
            )]),
            available_formats: vec![],
        }))
    }

    fn child(
        &self,
        _: &CurrentAnalysis,
        _: &ResultItem,
        _: &VizRenderRequest,
    ) -> Result<ItemEvidence, EngineError> {
        let unavailable = || EngineError::new("support.internal_error", Vec::new());
        let gate = self.0.lock().map_err(|_| unavailable())?.take();
        if let Some(gate) = gate {
            gate.recv_timeout(Duration::from_secs(10))
                .map_err(|_| unavailable())?;
        }
        Ok(ItemEvidence {
            presentation: ChildPolicyEvidence {
                path_compare_key: None,
                legend_class_key: "fixture.class".into(),
                display_flags: vec![],
            },
            match_key: VizMatchKey::Unknown {
                reason_key: "fixture.unavailable".into(),
            },
        })
    }
}

fn ok(engine: &Engine, name: &str, body: Value) -> TestResult<Value> {
    let reply = ipc(engine, name, body)?;
    if let Some(error) = reply.error {
        return Err(format!("{name}: {error:?}").into());
    }
    Ok(reply.body)
}

fn scan_two_roots(engine: &Engine, events: &DesktopEvents) -> TestResult<Id> {
    let fixture = tempfile::tempdir()?;
    let mut includes = Vec::new();
    for (name, size) in [("a", 17), ("b", 31)] {
        let directory = fixture.path().join(name);
        fs::create_dir(&directory)?;
        fs::write(directory.join("file.bin"), vec![0_u8; size])?;
        let bytes: Vec<_> = directory
            .as_os_str()
            .encode_wide()
            .flat_map(u16::to_le_bytes)
            .collect();
        includes.push(json!({
            "encoding":"windows_utf16le","raw":PathBytes::from_bytes(&bytes),
            "display":directory,"lossy":false,
        }));
    }
    let draft = json!({
        "includes":includes,"exclusions":[],"boundary":"current_volume","includedMounts":[],"source":"gui",
    });
    let review = ok(
        engine,
        "scope.review",
        json!({"draft":draft,"requestSeq":1}),
    )?;
    let scan: ScanStart = serde_json::from_value(ok(
        engine,
        "scan.start",
        json!({
            "draft":draft,"reviewFingerprint":review["reviewFingerprint"],
        }),
    )?)?;
    let batch = terminal_batch(events, &scan.job_ref)?;
    let finished = batch
        .iter()
        .find(|event| {
            event.topic == "job.finished" && event.job_ref.as_ref() == Some(&scan.job_ref)
        })
        .ok_or("missing scan completion")?;
    assert_eq!(finished.body["state"], "done");
    Ok(finished
        .result_version
        .clone()
        .ok_or("missing result revision")?)
}

fn render_request(view: &Value, version: &Id, item: u32, sequence: u32) -> Value {
    let parent = json!({"resultVersion":version,"itemNo":item});
    json!({
        "viewChannel":view,"requestSeq":sequence,"format":"treemap","resultRevision":version,
        "aggregateScopeRef":parent,"navigationScopeRef":parent,"measure":"size","sizeBasis":"logical",
        "classificationCriterion":"fixture.criterion","colorCriterion":"fixture.color","denominatorBasis":"scope",
        "viewport":{"width":800,"height":600,"scale":1.0},
        "visibleWindow":{"left":0,"top":0,"right":1000000,"bottom":1000000},"minAreaPx2":16,"depthLimit":1,
    })
}

fn completed_model(
    engine: &Engine,
    events: &DesktopEvents,
    mut request: Value,
    initial: Value,
) -> TestResult<Value> {
    if initial["kind"] == "model" {
        return Ok(initial);
    }
    assert_eq!(initial["kind"], "job");
    let job: Id = serde_json::from_value(initial["jobRef"].clone())?;
    terminal_batch(events, &job)?;
    let status = ok(engine, "job.get", json!({"jobRef":job}))?;
    assert_eq!(status["state"], "done");
    request["requestSeq"] = json!(request["requestSeq"].as_u64().ok_or("missing sequence")? + 1);
    ok(engine, "viz.render", request)
}

fn assert_model(
    engine: &Engine,
    model: &Value,
    version: &Id,
    item: u32,
    bytes: &str,
) -> TestResult<()> {
    assert_eq!(model["kind"], "model", "{model}");
    let rows = ok(
        engine,
        "viz.rows",
        json!({
            "modelRef":model["model"]["modelRef"],"resultRevision":version,"start":0,"count":10,
        }),
    )?;
    assert_eq!(rows["resultRevision"], json!(version));
    assert_eq!(rows["observedAt"], model["observedAt"]);
    assert_eq!(rows["completeness"], model["completeness"]);
    assert_eq!(rows["totalCount"], 1);
    let row = &rows["rows"][0];
    let shape = &model["model"]["shapes"][0];
    assert_eq!(row["size"]["bytes"], bytes);
    assert_eq!(row["size"]["evidence"], "os_reported");
    let total = ok(
        engine,
        "space.total",
        json!({
            "resultVersion":version,"requestSeq":item+1,
            "target":{"kind":"folder","item":{"resultVersion":version,"itemNo":item}},
        }),
    )?;
    let logical = total["sizes"]
        .as_array()
        .ok_or("missing F03 totals")?
        .iter()
        .find(|entry| entry["size"]["basis"] == "logical")
        .ok_or("missing F03 logical total")?;
    assert_eq!(row["denominator"]["size"], logical["size"]);
    assert_eq!(shape["itemRef"], row["itemRef"]);
    assert_eq!(shape["size"], row["size"]);
    Ok(())
}

/// Uses registered app-host commands and the desktop F24 workers to visualize an NTFS result.
#[test]
fn windows_scan_store_and_jobs_feed_the_selected_visualization() -> TestResult<()> {
    let profile = tempfile::tempdir()?;
    let database = crate::startup_database::open(profile.path())?;
    let mut builder = EngineBuilder::default();
    let random = &du_platform_os::entropy::OsRandom;
    let desktop = du_engine_api::desktop::register_services(
        &mut builder,
        Arc::new(du_platform_os::windows_observation::WindowsScopeRoots),
        Arc::new(du_platform_os::volume::WindowsVolumes),
        Arc::new(database.history(|| 400)),
        random,
        Arc::new(du_core_contracts::settings::SharedTableLimit::default),
    )?;
    let selection = Arc::new(Selection::default());
    let view_policy = ViewPolicy {
        revision: 1,
        max_scale: 4.0,
        max_shapes: 100,
        max_channels: NonZeroUsize::MIN.saturating_add(3),
        max_work: NonZeroUsize::MIN.saturating_add(3),
        synchronous_wait: Duration::from_millis(100),
    };
    let catalog_policy = || viz_catalog::Policy {
        measures: vec!["size".into()],
        color_criteria: vec!["type".into()],
        denominator_criteria: vec!["scope".into()],
        interpretation_table_revision: 1,
        required_data_table_revision: 1,
        legend_rule_revision: 1,
    };
    assert_eq!(
        desktop.visualization.register_preview_with_catalog(
            &mut builder,
            random,
            VisualizationPreview {
                producer: Arc::new(FixtureEvidence(Mutex::new(None))),
                selection: selection.clone(),
                policy: ViewPolicy {
                    max_shapes: 0,
                    ..view_policy.clone()
                },
                model_capacity: NonZeroUsize::MIN,
            },
            catalog_policy(),
        ),
        Err("invalid visualization policy")
    );
    let (release, gate) = mpsc::channel();
    desktop.visualization.register_preview_with_catalog(
        &mut builder,
        random,
        VisualizationPreview {
            producer: Arc::new(FixtureEvidence(Mutex::new(Some(gate)))),
            selection: selection.clone(),
            policy: view_policy,
            model_capacity: NonZeroUsize::MIN.saturating_add(3),
        },
        catalog_policy(),
    )?;
    let engine = builder.build()?;
    let catalog = ok(&engine, "viz.catalog", json!({}))?;
    assert_eq!(catalog["formats"], json!(["treemap"]));
    assert_eq!(
        catalog["strategies"],
        json!([{"id":"viz.treemap","revision":1}])
    );
    let version = scan_two_roots(&engine, &desktop.events)?;
    let completed = desktop
        .completed_scans
        .admit(&version)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(completed.version().observed_roots().len(), 2);
    let view = ok(&engine, "viz.view.open", json!({}))?["viewChannel"].clone();
    let request = render_request(&view, &version, 0, 1);
    assert!(ipc(&engine, "viz.render", request.clone())?.error.is_some());
    *selection.0.lock().map_err(|_| "selection lock poisoned")? = Some(version.clone());
    let wrong = Id::from_v7_bytes([83; 16]);
    let mismatch = ipc(&engine, "viz.render", render_request(&view, &wrong, 0, 1))?;
    assert_eq!(
        mismatch.error.ok_or("accepted wrong revision")?.reason_key,
        "result.version_stale"
    );
    let job = ok(&engine, "viz.render", request.clone())?;
    assert_eq!(job["kind"], "job");
    release.send(())?;
    let first = completed_model(&engine, &desktop.events, request, job)?;
    assert_model(&engine, &first, &version, 0, "17")?;
    assert_eq!(first["model"]["strategy"], catalog["strategies"][0]);
    for (model, catalog_field) in [
        ("interpretation", "interpretationTableRevision"),
        ("requiredData", "requiredDataTableRevision"),
        ("legend", "legendRuleRevision"),
    ] {
        assert_eq!(
            first["model"]["tableRevisions"][model],
            catalog[catalog_field]
        );
    }
    let request = render_request(&view, &version, 1, 3);
    let result = ok(&engine, "viz.render", request.clone())?;
    let second = completed_model(&engine, &desktop.events, request, result)?;
    let rule = &first["model"]["tableRevisions"]["sizeRule"];
    assert!(rule["id"].as_str().is_some_and(|id| !id.is_empty()));
    assert!(
        rule["revision"]
            .as_u64()
            .is_some_and(|revision| revision > 0)
    );
    assert_eq!(second["model"]["tableRevisions"]["sizeRule"], *rule);
    assert_model(&engine, &second, &version, 1, "31")
}
