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

/// Publishes `root` with one file and checks the root row, then the file row.
fn check(root: &str, file: &str, expected: [(&str, &str); 2]) -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = sink
        .open_observed(
            Id::from_v7_bytes([1; 16]),
            ResultScopeRoot {
                path: ResultNativePath {
                    encoding: PathEncoding::WindowsUtf16le,
                    raw: PathBytes::from_bytes(&wide(root)),
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
            path: vec![wide(file)],
            size: None,
        })
        .map_err(|e| format!("{e:?}"))?;
    let version = Id::from_v7_bytes([2; 16]);
    let pinned = build
        .seal(version.clone(), ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    for (item_no, (name, path)) in (0..).zip(expected) {
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

#[test]
fn rows_show_the_drive_form_and_keep_names_and_refs() -> Result<(), String> {
    check(
        r"\\?\C:\data",
        "파일.txt",
        [(r"C:\data", r"C:\data"), ("파일.txt", r"C:\data\파일.txt")],
    )
}

#[test]
fn rows_keep_the_prefix_for_trailing_dots_and_long_paths() -> Result<(), String> {
    let trailing = r"\\?\C:\trailing.";
    let file = r"\\?\C:\trailing.\a.txt";
    check(trailing, "a.txt", [(trailing, trailing), ("a.txt", file)])?;
    // The root is 255 units without the prefix; its file reaches 261.
    let long = format!(r"\\?\C:\{}", "g".repeat(252));
    let long_file = format!(r"{long}\a.txt");
    check(
        &long,
        "a.txt",
        [(&long[4..], &long[4..]), ("a.txt", &long_file)],
    )
}
