//! Keeps new use cases from pinning result versions without the channel check.
//!
//! Production code in this crate pins through the wrappers of the parent module. The files
//! below still pin directly; each one is safe for the reason given, and a new entry needs
//! the same kind of reason.

use std::{fs, path::Path};

/// Calls that pin a version of the shared result store.
const PIN_CALLS: [&str; 4] = [
    ".pin_current(",
    ".pin_scan(",
    ".pin()",
    ".pin_completed_scan(",
];

/// Files with direct pins and why each is acceptable.
const DIRECT: [(&str, &str); 5] = [
    ("source_exposure.rs", "the wrappers themselves"),
    (
        "viz_pages.rs",
        "model pages re-pin the version of a model a render already admitted; viz.rows, \
         viz.group_rows and viz.locate call ModelPages::refuse_hidden first",
    ),
    ("viz_pages/deferred.rs", "model pages, as in viz_pages.rs"),
    ("viz_pages/locate.rs", "model pages, as in viz_pages.rs"),
    ("viz_pages/session.rs", "model pages, as in viz_pages.rs"),
];

/// Files that pin a completed scan for the app host's comparison and snapshot flow.
const COMPLETED_SCAN: (&str, &str) = (
    "desktop/scan_completion.rs",
    "only a real scan can be delivered as completed, so a sample version never reaches it",
);

fn rust_files(dir: &Path, found: &mut Vec<std::path::PathBuf>) -> std::io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            rust_files(&path, found)?;
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            found.push(path);
        }
    }
    Ok(())
}

fn is_test_file(relative: &str) -> bool {
    relative.split('/').any(|part| {
        matches!(part, "tests" | "tests.rs" | "pin_guard.rs")
            || part.ends_with("_tests.rs")
            || part.starts_with("test_")
    })
}

/// The part of a source file before its first test module.
fn production_part(source: &str) -> &str {
    const MARKER: &str = "#[cfg(test)]";
    let mut from = 0;
    while let Some(found) = source[from..].find(MARKER) {
        let start = from + found;
        let after = source[start + MARKER.len()..].trim_start();
        if after.starts_with("mod ") || after.starts_with("pub mod ") {
            return &source[..start];
        }
        from = start + MARKER.len();
    }
    source
}

#[test]
fn production_code_pins_result_versions_only_through_the_exposure_wrappers() -> Result<(), String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&root, &mut files).map_err(|error| error.to_string())?;
    assert!(files.len() > 100, "the source tree was not found");
    let allowed: Vec<&str> = DIRECT
        .iter()
        .map(|(file, _)| *file)
        .chain([COMPLETED_SCAN.0])
        .collect();
    let mut offenders = Vec::new();
    for file in files {
        let relative = file
            .strip_prefix(&root)
            .map_err(|error| error.to_string())?
            .to_string_lossy()
            .replace('\\', "/");
        if is_test_file(&relative) || allowed.contains(&relative.as_str()) {
            continue;
        }
        let source = fs::read_to_string(&file).map_err(|error| error.to_string())?;
        let production = production_part(&source);
        if let Some(call) = PIN_CALLS.iter().find(|call| production.contains(**call)) {
            offenders.push(format!("{relative}: {call}"));
        }
    }
    assert!(
        offenders.is_empty(),
        "pin through source_exposure::pin_*_for instead: {offenders:?}"
    );
    Ok(())
}

#[test]
fn the_allowed_direct_pins_still_exist() -> Result<(), String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for file in DIRECT
        .iter()
        .map(|(file, _)| *file)
        .chain([COMPLETED_SCAN.0])
    {
        let source =
            fs::read_to_string(root.join(file)).map_err(|error| format!("{file}: {error}"))?;
        assert!(
            PIN_CALLS
                .iter()
                .any(|call| production_part(&source).contains(call)),
            "{file} no longer pins directly; remove it from the list"
        );
    }
    Ok(())
}
