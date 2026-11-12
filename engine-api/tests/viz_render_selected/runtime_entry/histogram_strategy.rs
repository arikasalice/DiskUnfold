use super::*;
use du_analysis_index::visualization::{
    histogram_dispatch, registry,
    typed_layout::{CommonInput, Dispatcher, Error as LayoutError, Output, Strategy},
};
use du_core_contracts::visualization::{LayoutDescriptor, LayoutStrategyKey};
use std::sync::atomic::AtomicUsize;

struct Alternate(Arc<AtomicUsize>);
impl Strategy for Alternate {
    fn layout_with_axis(
        &self,
        input: &CommonInput<'_>,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<(Output, Option<u64>), LayoutError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        let (mut output, axis) =
            Strategy::layout_with_axis(&histogram_dispatch::HistogramLayout, input, cancelled)?;
        if let Some(shape) = output.shapes.first_mut() {
            let mut points = shape.geometry.points().to_vec();
            if points[0].x + 1 < points[1].x {
                points[0].x += 1;
                shape.geometry =
                    du_core_contracts::visualization::geometry::Geometry::new("rect", points)
                        .map_err(LayoutError::Geometry)?;
            }
        }
        Ok((output, axis))
    }
}

fn alternate_dispatcher(calls: Arc<AtomicUsize>) -> Result<Dispatcher, String> {
    let mut builder = registry::Builder::<Box<dyn Strategy>>::default();
    bundle::register_implemented(&mut builder).map_err(|e| format!("{e:?}"))?;
    bundle::register_histogram_candidate(&mut builder).map_err(|e| format!("{e:?}"))?;
    builder
        .register(
            LayoutDescriptor {
                format: "histogram".into(),
                strategy: LayoutStrategyKey {
                    id: "viz.histogram".into(),
                    revision: 2,
                },
            },
            Box::new(Alternate(calls)),
        )
        .map_err(|e| format!("{e:?}"))?;
    Ok(Dispatcher::new(
        builder.build(&[]).map_err(|e| format!("{e:?}"))?,
    ))
}

fn body(view: &Value, seq: u32, revision: Option<u32>) -> Value {
    let mut body = render_body(view, seq);
    body["format"] = json!("histogram");
    if let Some(revision) = revision {
        body["strategy"] = json!({"id":"viz.histogram","revision":revision});
    }
    body
}

#[test]
fn selected_histogram_revision_executes_and_unknown_preserves_running_work() -> Result<(), String> {
    let sink = IngestSink::default();
    publish_sizes(&sink, id(2), id(3), [25, 75])?;
    let calls = Arc::new(AtomicUsize::new(0));
    let f = fixture(
        sink,
        Duration::ZERO,
        NonZeroUsize::MIN.saturating_add(3),
        alternate_dispatcher(calls.clone())?,
    )?;
    let view = f.open()?;
    let physical = job(&f.ok("viz.render", body(&view, 1, Some(2)))?)?;
    f.entered
        .recv_timeout(Duration::from_secs(3))
        .map_err(|e| e.to_string())?;
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let error = f
        .call("viz.render", body(&view, 2, Some(99)))
        .error
        .ok_or("unknown revision accepted")?;
    assert_eq!(error.reason_key, "viz.strategy_unknown");
    assert_eq!(
        f.jobs.state(&physical).map_err(|e| format!("{e:?}"))?,
        JobState::Running
    );
    f.release.send(()).map_err(|e| e.to_string())?;
    f.finished(&physical, JobState::Done)?;
    let model = f.ok("viz.render", body(&view, 3, Some(2)))?;
    assert_eq!(model["kind"], "model");
    assert_eq!(model["resultRevision"], json!(id(3)));
    assert_eq!(
        model["model"]["strategy"],
        json!({"id":"viz.histogram","revision":2})
    );
    let reference = model["model"]["modelRef"].as_str().ok_or("model ref")?;
    let frame = f
        .session
        .histogram_frame(reference)
        .map_err(|e| format!("{e:?}"))?;
    let rows = f.ok(
        "viz.rows",
        json!({"modelRef":reference,"resultRevision":id(3),"start":0,"count":10}),
    )?;
    assert_eq!(rows["resultRevision"], json!(id(3)));
    assert_eq!(rows["rows"][0]["groupRef"], json!(frame.bins[0].group_ref));
    assert!(frame.bins[0].rect.is_some());
    assert_eq!(frame.bins[0].rect.ok_or("bin geometry")?.left, 1);
    assert_eq!(
        model["model"]["shapes"][0]["groupRef"],
        rows["rows"][0]["groupRef"]
    );
    assert_eq!(
        f.ok("viz.render", body(&view, 4, None))?["model"]["modelRef"],
        model["model"]["modelRef"]
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let first = f.ok("viz.render", body(&view, 5, Some(1)))?;
    let first = if first["kind"] == "job" {
        f.finished(&job(&first)?, JobState::Done)?;
        f.ok("viz.render", body(&view, 6, Some(1)))?
    } else {
        first
    };
    assert_eq!(first["model"]["strategy"]["revision"], 1);
    assert_ne!(first["model"]["modelRef"], model["model"]["modelRef"]);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    Ok(())
}

pub(super) fn fixture(
    sink: IngestSink,
    synchronous_wait: Duration,
    max_work: NonZeroUsize,
    dispatcher: du_analysis_index::visualization::typed_layout::Dispatcher,
) -> Result<Fixture, String> {
    fixture_inner(sink, synchronous_wait, max_work, dispatcher, None)
}

pub(super) fn fixture_with_selection(
    sink: IngestSink,
    synchronous_wait: Duration,
    max_work: NonZeroUsize,
    dispatcher: du_analysis_index::visualization::typed_layout::Dispatcher,
    selection: Arc<dyn du_engine_api::viz_runtime::selection::WindowScanSelection>,
) -> Result<Fixture, String> {
    fixture_inner(
        sink,
        synchronous_wait,
        max_work,
        dispatcher,
        Some(selection),
    )
}

fn fixture_inner(
    sink: IngestSink,
    synchronous_wait: Duration,
    max_work: NonZeroUsize,
    dispatcher: du_analysis_index::visualization::typed_layout::Dispatcher,
    selection: Option<Arc<dyn du_engine_api::viz_runtime::selection::WindowScanSelection>>,
) -> Result<Fixture, String> {
    let ids = Arc::new(Identity(AtomicU8::new(15)));
    let session = Arc::new(Session::new(
        sink.clone(),
        NonZeroUsize::MIN.saturating_add(3),
        ids.clone(),
        dispatcher,
    ));
    let (events_sender, events) = mpsc::channel();
    let jobs = Arc::new(
        JobRuntime::new(
            ids.clone(),
            Arc::new(Clock),
            Arc::new(History),
            Arc::new(Events(events_sender)),
        )
        .map_err(|e| format!("{e:?}"))?,
    );
    let (entered_sender, entered) = mpsc::channel();
    let (release, release_receiver) = mpsc::channel();
    let producer = Arc::new(Producer {
        block: Mutex::new(Some((entered_sender, release_receiver))),
        unit_iec: AtomicBool::new(false),
        required_data_mode: AtomicU8::new(0),
        histogram_bins_absent: AtomicBool::new(false),
        hierarchy_unknown: AtomicBool::new(false),
        child_fails: AtomicBool::new(false),
    });
    let mut builder = EngineBuilder::default();
    let assembly = Assembly {
        source: sink.clone(),
        session: session.clone(),
        jobs: jobs.clone(),
        ids,
        producer: producer.clone(),
        policy: ViewPolicy {
            revision: 1,
            max_scale: 4.0,
            max_shapes: 100,
            max_channels: NonZeroUsize::MIN.saturating_add(3),
            max_work,
            synchronous_wait,
        },
    };
    match selection {
        Some(selection) => {
            Runtime::register_with_selection(&mut builder, assembly, &Entropy, selection)?
        }
        None => Runtime::register(&mut builder, assembly, &Entropy)?,
    };
    let engine = builder.build().map_err(|e| e.to_string())?;
    Ok(Fixture {
        engine,
        jobs,
        sink,
        events,
        entered,
        release,
        session,
        producer,
    })
}
