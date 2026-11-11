use super::*;
use du_analysis_index::visualization::{
    bundle, registry,
    typed_layout::{Dispatcher, Strategy},
};
use std::sync::atomic::AtomicUsize;

struct Alternate(Arc<AtomicUsize>);
impl Strategy for Alternate {
    fn bar(
        &self,
        input: du_analysis_index::visualization::typed_layout::BarInput<'_>,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<
        du_analysis_index::visualization::bar_projection::Ranking,
        du_analysis_index::visualization::typed_layout::BarError,
    > {
        self.0.fetch_add(1, Ordering::SeqCst);
        Strategy::bar(
            &du_analysis_index::visualization::typed_layout::RankedBar,
            input,
            cancelled,
        )
    }
}

fn fixture(sink: IngestSink) -> Result<Fixture, String> {
    fixture_with_alternate(sink, None)
}
fn fixture_with_alternate(
    sink: IngestSink,
    calls: Option<Arc<AtomicUsize>>,
) -> Result<Fixture, String> {
    let mut builder = registry::Builder::<Box<dyn Strategy>>::default();
    bundle::register_implemented(&mut builder).map_err(|e| format!("{e:?}"))?;
    bundle::register_ranked_bar(&mut builder).map_err(|e| format!("{e:?}"))?;
    if let Some(calls) = calls {
        let mut descriptor = du_analysis_index::visualization::typed_layout::bar_descriptor();
        descriptor.strategy.revision = 2;
        builder
            .register(descriptor, Box::new(Alternate(calls)))
            .map_err(|e| format!("{e:?}"))?;
    }
    histogram_strategy::fixture(
        sink,
        Duration::ZERO,
        NonZeroUsize::MIN.saturating_add(3),
        Dispatcher::new(builder.build(&[]).map_err(|e| format!("{e:?}"))?),
    )
}

#[test]
fn selected_bar_strategy_revision_is_executed_and_returned() -> Result<(), String> {
    let sink = IngestSink::default();
    publish_sizes(&sink, id(2), id(3), [25, 75])?;
    let calls = Arc::new(AtomicUsize::new(0));
    let f = fixture_with_alternate(sink, Some(calls.clone()))?;
    let view = f.open()?;
    let result = ready(&f, &view)?;
    assert_eq!(result["kind"], "model");
    assert_eq!(
        result["model"]["strategy"],
        json!({"id":"viz.bar","revision":2})
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    Ok(())
}
fn request(view: &Value, seq: u32) -> Value {
    let mut body = render_body(view, seq);
    body["format"] = json!("bar");
    body
}
fn ready(f: &Fixture, view: &Value) -> Result<Value, String> {
    let physical = job(&f.ok("viz.render", request(view, 1))?)?;
    f.entered
        .recv_timeout(Duration::from_secs(3))
        .map_err(|e| e.to_string())?;
    f.release.send(()).map_err(|e| e.to_string())?;
    f.finished(&physical, JobState::Done)?;
    f.ok("viz.render", request(view, 2))
}

#[test]
fn ranked_bar_publishes_fixed_rows_with_tie_order_and_denominator() -> Result<(), String> {
    let sink = IngestSink::default();
    publish_sizes(&sink, id(2), id(3), [25, 25])?;
    let f = fixture(sink)?;
    let view = f.open()?;
    let result = ready(&f, &view)?;
    assert_eq!(result["kind"], "model");
    assert_eq!(result["resultRevision"], json!(id(3)));
    assert_eq!(
        result["model"]["strategy"],
        json!({"id":"viz.bar","revision":1})
    );
    assert_eq!(
        result["model"]["shapes"].as_array().ok_or("shapes")?.len(),
        2
    );
    let reference = result["model"]["modelRef"].as_str().ok_or("model ref")?;
    let rows = f.ok(
        "viz.rows",
        json!({"modelRef":reference,"resultRevision":id(3),"start":0,"count":10}),
    )?;
    assert_eq!(rows["resultRevision"], json!(id(3)));
    assert_eq!(rows["rows"].as_array().ok_or("rows")?.len(), 2);
    assert_eq!(rows["rows"][0]["itemRef"]["itemNo"], 1);
    assert_eq!(rows["rows"][1]["itemRef"]["itemNo"], 2);
    assert_eq!(rows["rows"][0]["denominator"]["value"]["value"], "50");
    assert_eq!(
        result["model"]["shapes"][0]["itemRef"],
        rows["rows"][0]["itemRef"]
    );
    Ok(())
}

#[test]
fn ranked_bar_cancel_and_source_replacement_do_not_publish() -> Result<(), String> {
    for replace in [false, true] {
        let sink = IngestSink::default();
        publish_sizes(&sink, id(2), id(3), [25, 75])?;
        let f = fixture(sink)?;
        let view = f.open()?;
        let physical = job(&f.ok("viz.render", request(&view, 1))?)?;
        f.entered
            .recv_timeout(Duration::from_secs(3))
            .map_err(|e| e.to_string())?;
        if replace {
            publish(&f.sink, id(2), id(4))?;
        } else {
            assert_eq!(f.detach(&view, 1)?["controlResult"], "accepted");
        }
        f.release.send(()).map_err(|e| e.to_string())?;
        f.finished(
            &physical,
            if replace {
                JobState::Failed
            } else {
                JobState::Cancelled
            },
        )?;
        if replace {
            assert_eq!(
                f.call("viz.render", request(&view, 2))
                    .error
                    .ok_or("stale accepted")?
                    .reason_key,
                "result.version_stale"
            );
        }
    }
    Ok(())
}

#[test]
fn unknown_size_publishes_same_revision_fallback_rows() -> Result<(), String> {
    let sink = IngestSink::default();
    let lineage = sink
        .bind_current_analysis("scope", id(1))
        .map_err(|e| format!("{e:?}"))?;
    let root = ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(b"/fixture"),
        },
        volume_id: vec![1],
        file_id: vec![2],
    };
    let mut build = sink
        .open_forest_in_lineage(id(2), vec![root], lineage)
        .map_err(|e| format!("{e:?}"))?;
    build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    for (name, size) in [
        (
            b'a',
            Some(SizeValue {
                basis: SizeBasis::Logical,
                evidence: SizeEvidence::Measured,
                amount: SizeAmount::Exact {
                    bytes: 25_u64.into(),
                },
            }),
        ),
        (b'b', None),
    ] {
        build
            .add_file(Observation {
                path: vec![vec![name]],
                size,
            })
            .map_err(|e| format!("{e:?}"))?;
    }
    build
        .seal(id(3), ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let f = fixture(sink)?;
    let view = f.open()?;
    let result = ready(&f, &view)?;
    assert_eq!(result["kind"], "unavailable");
    assert_eq!(result["reasonKey"], "viz.unavailable.no_attribution");
    let fallback = &result["fallback"];
    assert_eq!(fallback["resultRevision"], json!(id(3)));
    let rows = f.ok(
        "viz.rows",
        json!({"modelRef":fallback["modelRef"],"resultRevision":id(3),"start":0,"count":10}),
    )?;
    assert_eq!(rows["resultRevision"], json!(id(3)));
    assert_eq!(rows["totalCount"], 2);
    Ok(())
}
