use du_analysis_index::{IngestSink, QueryError};
use du_core_contracts::{
    Id, ItemRef, PathBytes, PathEncoding,
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot, ResultVersionRead},
};

fn id(value: u32) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{value:012x}"))
}

fn wide(units: impl IntoIterator<Item = u16>) -> Vec<u8> {
    units.into_iter().flat_map(u16::to_le_bytes).collect()
}

fn observed(raw: &[u8], encoding: PathEncoding) -> ResultScopeRoot {
    ResultScopeRoot {
        path: ResultNativePath {
            encoding,
            raw: PathBytes::from_bytes(raw),
        },
        volume_id: vec![1],
        file_id: vec![2],
    }
}

#[test]
fn partial_and_terminal_paths_preserve_native_code_units_and_old_pins() -> Result<(), String> {
    let source = IngestSink::default();
    let raw = wide(r"\\?\D:\Fixture".encode_utf16());
    let root = observed(&raw, PathEncoding::WindowsUtf16le);
    let mut build = source
        .open_observed(id(1)?, root.clone())
        .map_err(|e| format!("{e:?}"))?;
    let name = wide([0xd800, u16::from(b' ')]);
    build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    build
        .add_directory(vec![name.clone()])
        .map_err(|e| format!("{e:?}"))?;
    let partial = build
        .publish_partial(id(2)?)
        .map_err(|e| format!("{e:?}"))?;
    let final_version = build
        .seal(id(3)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let expected = [raw, wide([u16::from(b'\\')]), name].concat();
    for version in [&partial, &final_version] {
        assert_eq!(version.observed_root(), Some(&root));
        let item = ItemRef {
            result_version: version.version_id().clone(),
            item_no: 1,
        };
        let path = version.native_path(&item).map_err(|e| format!("{e:?}"))?;
        assert_eq!(path.raw.bytes().map_err(|e| e.to_string())?, expected);
    }
    assert_eq!(
        final_version.native_path(&ItemRef {
            result_version: id(2)?,
            item_no: 1
        }),
        Err(QueryError::StaleVersion)
    );
    Ok(())
}

#[test]
fn unix_names_keep_invalid_utf8_and_reject_path_injection() -> Result<(), String> {
    let source = IngestSink::default();
    let mut build = source
        .open_observed(id(1)?, observed(b"/", PathEncoding::Unix))
        .map_err(|e| format!("{e:?}"))?;
    for path in [vec![], vec![b"bad/name".to_vec()], vec![vec![0xff]]] {
        build.add_directory(path).map_err(|e| format!("{e:?}"))?;
    }
    let version = build
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let item = |item_no| ItemRef {
        result_version: version.version_id().clone(),
        item_no,
    };
    assert_eq!(version.native_path(&item(1)), Err(QueryError::InvalidPath));
    let path = version
        .native_path(&item(2))
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        path.raw.bytes().map_err(|e| e.to_string())?,
        vec![b'/', 0xff]
    );

    let mut legacy = source.open(id(3)?).map_err(|e| format!("{e:?}"))?;
    legacy.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    let legacy = legacy
        .seal(id(4)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        legacy.native_path(&ItemRef {
            result_version: id(4)?,
            item_no: 0
        }),
        Err(QueryError::PathUnavailable)
    );
    Ok(())
}
