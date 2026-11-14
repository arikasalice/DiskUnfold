use du_analysis_index::{IngestSink, QueryError};
use du_core_contracts::{
    Id, PathBytes, PathEncoding,
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot, ResultVersionRead},
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};
fn id(n: u8) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}
fn root(path: &str, volume: u8) -> ResultScopeRoot {
    ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::WindowsUtf16le,
            raw: PathBytes::from_bytes(
                &path
                    .encode_utf16()
                    .flat_map(u16::to_le_bytes)
                    .collect::<Vec<_>>(),
            ),
        },
        volume_id: vec![volume],
        file_id: vec![9],
    }
}

fn current_partial() -> Result<
    (
        IngestSink,
        du_analysis_index::BuildHandle,
        Arc<du_analysis_index::ResultVersion>,
    ),
    String,
> {
    let sink = IngestSink::default();
    let lineage = sink
        .bind_current_analysis("current-gate-race", id(20)?)
        .map_err(|error| format!("bind lineage: {error:?}"))?;
    let mut build = sink
        .open_forest_in_lineage(id(21)?, vec![root("C:\\fixture", 1)], lineage)
        .map_err(|error| format!("open result: {error:?}"))?;
    build
        .add_directory_at(0, Vec::new())
        .map_err(|error| format!("add root: {error:?}"))?;
    let first = build
        .publish_partial(id(22)?)
        .map_err(|error| format!("publish first partial: {error:?}"))?;
    Ok((sink, build, first))
}
#[test]
fn current_operation_uses_exact_pinned_lineage_and_object() -> Result<(), String> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let sink = IngestSink::default();
    let first = sink
        .bind_current_analysis("first", id(30)?)
        .map_err(|e| format!("{e:?}"))?;
    let second = sink
        .bind_current_analysis("second", id(31)?)
        .map_err(|e| format!("{e:?}"))?;
    let build = |store: &IngestSink, lineage: Id, scope: u8, version: u8| -> Result<_, String> {
        let mut builder = store
            .open_forest_in_lineage(id(scope)?, vec![root("C:\\fixture", 1)], lineage)
            .map_err(|e| format!("{e:?}"))?;
        builder
            .add_directory_at(0, vec![])
            .map_err(|e| format!("{e:?}"))?;
        builder
            .seal(id(version)?, ResultCompleteness::Complete)
            .map_err(|e| format!("{e:?}"))
    };
    let old = build(&sink, first.clone(), 32, 33)?;
    let other = build(&sink, second, 34, 35)?;
    let calls = AtomicUsize::new(0);
    assert_eq!(
        sink.with_current(&old, || calls.fetch_add(1, Ordering::SeqCst)),
        Ok(0)
    );
    let current = build(&sink, first.clone(), 36, 37)?;
    assert_eq!(
        sink.with_current(&old, || calls.fetch_add(1, Ordering::SeqCst)),
        Err(QueryError::StaleVersion)
    );
    assert_eq!(
        sink.with_current(&other, || calls.fetch_add(1, Ordering::SeqCst)),
        Ok(1)
    );
    assert_eq!(
        sink.with_current(&current, || calls.fetch_add(1, Ordering::SeqCst)),
        Ok(2)
    );
    let impostor_sink = IngestSink::default();
    let impostor_lineage = impostor_sink
        .bind_current_analysis("first", first)
        .map_err(|e| format!("{e:?}"))?;
    let impostor = build(&impostor_sink, impostor_lineage, 36, 37)?;
    assert_eq!(
        sink.with_current(&impostor, || calls.fetch_add(1, Ordering::SeqCst)),
        Err(QueryError::StaleVersion)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    Ok(())
}

#[test]
fn current_operation_orders_admission_and_publication() -> Result<(), Box<dyn std::error::Error>> {
    let (sink, mut build, first) = current_partial()?;
    build
        .publish_partial(id(23)?)
        .map_err(|error| format!("publish replacement first: {error:?}"))?;
    let mutations = AtomicUsize::new(0);
    assert_eq!(
        sink.with_current(&first, || mutations.fetch_add(1, Ordering::SeqCst)),
        Err(QueryError::StaleVersion)
    );
    assert_eq!(mutations.load(Ordering::SeqCst), 0);

    let (sink, mut build, first) = current_partial()?;
    let next_version = id(23)?;
    let (start_tx, start_rx) = mpsc::channel();
    let (attempt_tx, attempt_rx) = mpsc::channel();
    let (published_tx, published_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let order = Arc::new(AtomicUsize::new(0));
    let writer_order = Arc::clone(&order);
    let writer_version = next_version.clone();
    let writer = thread::spawn(move || -> Result<(), String> {
        start_rx
            .recv_timeout(Duration::from_secs(5))
            .map_err(|error| format!("wait to publish: {error}"))?;
        attempt_tx
            .send(())
            .map_err(|error| format!("signal publication attempt: {error}"))?;
        build
            .publish_partial(writer_version)
            .map_err(|error| format!("publish replacement: {error:?}"))?;
        published_tx
            .send(writer_order.fetch_add(1, Ordering::AcqRel) + 1)
            .map_err(|error| format!("signal publication: {error}"))?;
        release_rx
            .recv_timeout(Duration::from_secs(5))
            .map_err(|error| format!("wait before dropping build: {error}"))?;
        Ok(())
    });

    let admission_order = sink
        .with_current(&first, || -> Result<usize, String> {
            start_tx
                .send(())
                .map_err(|error| format!("start concurrent publication: {error}"))?;
            attempt_rx
                .recv_timeout(Duration::from_secs(5))
                .map_err(|error| format!("wait for publication attempt: {error}"))?;
            Ok(order.fetch_add(1, Ordering::AcqRel) + 1)
        })
        .map_err(|error| format!("guard current result: {error:?}"))??;
    let publication_order = published_rx.recv_timeout(Duration::from_secs(5))?;
    assert!(admission_order < publication_order);
    let current = sink
        .pin_current(&next_version)
        .map_err(|error| format!("pin replacement: {error:?}"))?;
    assert_eq!(current.version_id(), &next_version);
    release_tx.send(())?;
    writer.join().map_err(|_| "publication worker panicked")??;
    Ok(())
}
