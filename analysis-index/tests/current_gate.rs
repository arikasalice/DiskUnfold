use du_analysis_index::{IngestSink, QueryError};
use du_core_contracts::{
    Id, PathBytes, PathEncoding,
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot},
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
