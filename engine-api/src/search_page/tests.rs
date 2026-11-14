//! Search rows show complete Windows paths in their display form.
use du_analysis_index::{IngestSink, Observation};
use du_core_contracts::{
    Id, ItemRef, PathBytes, PathEncoding,
    browse_ipc::BrowseValue,
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot},
};

fn wide(text: &str) -> Vec<u8> {
    text.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

fn present(value: &BrowseValue<String>) -> Option<&str> {
    match value {
        BrowseValue::Present { value } => Some(value),
        _ => None,
    }
}

#[test]
fn rows_show_the_drive_form_and_keep_names_and_refs() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = sink
        .open_observed(
            Id::from_v7_bytes([1; 16]),
            ResultScopeRoot {
                path: ResultNativePath {
                    encoding: PathEncoding::WindowsUtf16le,
                    raw: PathBytes::from_bytes(&wide(r"\\?\C:\data")),
                },
                volume_id: vec![1],
                file_id: vec![1],
            },
        )
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_directory(Vec::new())
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_file(Observation {
            path: vec![wide("파일.txt")],
            size: None,
        })
        .map_err(|e| format!("{e:?}"))?;
    let version = Id::from_v7_bytes([2; 16]);
    let pinned = build
        .seal(version.clone(), ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    for (item_no, name, path) in [
        (0, r"C:\data", r"C:\data"),
        (1, "파일.txt", r"C:\data\파일.txt"),
    ] {
        let reference = ItemRef {
            result_version: version.clone(),
            item_no,
        };
        let row = super::row(&pinned, &reference).map_err(|e| format!("{e:?}"))?;
        assert_eq!(present(&row.display_name), Some(name));
        assert_eq!(present(&row.display_path), Some(path));
        assert_eq!(row.item, reference);
    }
    Ok(())
}
