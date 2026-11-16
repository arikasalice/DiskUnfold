use du_analysis_index::{BuildHandle, IngestSink, QueryError, ResultVersion};
use du_core_contracts::{
    Id, PathBytes, PathEncoding,
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot, ResultVersionRead},
};
use std::sync::Arc;

fn id(n: u32) -> Result<Id, String> {
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

fn add_roots(build: &mut BuildHandle) -> Result<(), String> {
    for root_index in 0..2 {
        build
            .add_directory_at(root_index, Vec::new())
            .map_err(|error| format!("{error:?}"))?;
    }
    Ok(())
}

fn assert_current(
    sink: &IngestSink,
    number: u32,
    expected: &Arc<ResultVersion>,
) -> Result<(), String> {
    let current = sink
        .pin_current(&id(number)?)
        .map_err(|error| format!("{error:?}"))?;
    assert!(Arc::ptr_eq(&current, expected));
    Ok(())
}

#[test]
fn pinned_scope_gaps_survive_partial_abort_and_replacement() -> Result<(), String> {
    let sink = IngestSink::default();
    let lineage = sink
        .bind_current_analysis("scope-gap-publication", id(1)?)
        .map_err(|error| format!("{error:?}"))?;
    let roots = vec![root("C:\\A", 1), root("D:\\B", 2)];

    let mut a_build = sink
        .open_forest_in_lineage(id(2)?, roots.clone(), lineage.clone())
        .map_err(|error| format!("{error:?}"))?;
    add_roots(&mut a_build)?;
    a_build
        .record_gap_at(0, Vec::new(), "scan.path_denied".into(), None)
        .map_err(|error| format!("{error:?}"))?;
    let a = a_build
        .seal(id(3)?, ResultCompleteness::Complete)
        .map_err(|error| format!("{error:?}"))?;
    let a_gaps = a.scope_gaps().to_vec();
    assert_eq!(a_gaps.len(), 1);
    assert_eq!(a_gaps[0].root_index, 0);
    assert_current(&sink, 3, &a)?;

    let mut b_build = sink
        .open_forest_in_lineage(id(4)?, roots, lineage)
        .map_err(|error| format!("{error:?}"))?;
    add_roots(&mut b_build)?;
    b_build
        .record_gap_at(1, Vec::new(), "scan.path_denied".into(), None)
        .map_err(|error| format!("{error:?}"))?;
    let b = b_build
        .publish_partial(id(5)?)
        .map_err(|error| format!("{error:?}"))?;
    let b_gaps = b.scope_gaps().to_vec();
    assert_eq!(b_gaps.len(), 1);
    assert_eq!(b_gaps[0].root_index, 1);
    assert_ne!(a_gaps, b_gaps);
    assert_eq!(a.scope_gaps(), a_gaps);
    assert_eq!(b.scope_gaps(), b_gaps);
    assert_current(&sink, 5, &b)?;

    b_build.abort();
    assert_eq!(a.scope_gaps(), a_gaps);
    assert_eq!(b.scope_gaps(), b_gaps);
    assert_current(&sink, 3, &a)?;
    assert_eq!(
        sink.pin_current(&id(5)?).err(),
        Some(QueryError::StaleVersion)
    );

    let mut c_build = sink
        .open_replacement(&a)
        .map_err(|error| format!("{error:?}"))?;
    add_roots(&mut c_build)?;
    c_build
        .record_gap_at(
            1,
            vec![b"missing".to_vec()],
            "scan.path_denied".into(),
            None,
        )
        .map_err(|error| format!("{error:?}"))?;
    let c = c_build
        .seal(id(6)?, ResultCompleteness::Complete)
        .map_err(|error| format!("{error:?}"))?;
    let c_gaps = c.scope_gaps().to_vec();
    assert_eq!(c_gaps.len(), 1);
    assert_eq!(c_gaps[0].root_index, 1);
    assert_ne!(a_gaps, c_gaps);
    assert_ne!(b_gaps, c_gaps);
    assert_eq!(a.scope_gaps(), a_gaps);
    assert_eq!(b.scope_gaps(), b_gaps);
    assert_eq!(c.scope_gaps(), c_gaps);
    assert_current(&sink, 6, &c)?;
    Ok(())
}
