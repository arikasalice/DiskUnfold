//! F36 member paths from a Windows result whose root keeps the extended prefix.
use du_analysis_index::{IngestSink, Observation};
use du_core_contracts::{
    PathBytes, PathEncoding,
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot},
};
use serde_json::json;

use super::{MemoryRules, TestResult, at_least, classify, harness_from, id, logical};

fn wide(text: &str) -> Vec<u8> {
    text.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

fn publish(sink: &IngestSink, root: &str) -> TestResult {
    let root = ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::WindowsUtf16le,
            raw: PathBytes::from_bytes(&wide(root)),
        },
        volume_id: vec![1],
        file_id: vec![1],
    };
    let mut build = sink
        .open_observed(id(1), root)
        .map_err(|error| format!("{error:?}"))?;
    build
        .add_directory(Vec::new())
        .map_err(|error| format!("{error:?}"))?;
    for (name, bytes) in [("big.bin", 20), ("small.txt", 5)] {
        build
            .add_file(Observation {
                path: vec![wide(name)],
                size: Some(logical(bytes)),
            })
            .map_err(|error| format!("{error:?}"))?;
    }
    build
        .seal(id(2), ResultCompleteness::Complete)
        .map_err(|error| format!("{error:?}"))?;
    Ok(())
}

/// Checks member and explanation paths of a result rooted at `root`; each
/// member is shown as `{shown}\{name}`.
fn check(root: &str, shown: &str) -> TestResult {
    let sink = IngestSink::default();
    publish(&sink, root)?;
    let harness = harness_from(sink, std::sync::Arc::new(MemoryRules::default()), false, 64)?;
    harness.ok(
        "group.rules.save",
        json!({ "expectedRevision": 0, "rules": [
            { "displayName": "Big", "condition": at_least(10) },
        ] }),
    )?;
    let summary = classify(&harness, 1, json!({ "kind": "saved", "rulesetVersion": 1 }))?;
    let summary = &summary["summary"];
    let reference = &summary["classificationRef"];
    let members = |category| {
        harness.ok(
            "group.members",
            json!({ "classificationRef": reference, "resultVersion": id(2),
                    "category": category, "start": 0, "count": 10 }),
        )
    };
    let group_id = summary["groups"][0]["groupId"].clone();
    for (category, name) in [
        (json!({ "kind": "group", "groupId": group_id }), "big.bin"),
        (json!({ "kind": "unclassified" }), "small.txt"),
    ] {
        let page = members(category)?;
        let row = &page["rows"][0];
        assert_eq!(row["displayPath"]["value"], format!(r"{shown}\{name}"));
        assert_eq!(row["displayName"]["value"], name);
        let item = row["item"].clone();
        let pinned = harness
            .sink
            .pin_current(&id(2))
            .map_err(|error| format!("{error:?}"))?;
        let item_ref: du_core_contracts::ItemRef = serde_json::from_value(item.clone())?;
        let native = pinned
            .native_path(&item_ref)
            .map_err(|error| format!("{error:?}"))?;
        assert_eq!(
            native.raw,
            PathBytes::from_bytes(&wide(&format!(r"{root}\{name}")))
        );
        let explained = harness.ok(
            "group.explain",
            json!({ "classificationRef": reference, "item": item }),
        )?;
        assert_eq!(explained["item"], row["item"]);
        assert_eq!(explained["displayPath"], row["displayPath"]);
    }
    Ok(())
}

#[test]
fn member_and_explanation_paths_drop_the_prefix_and_keep_item_refs() -> TestResult {
    check(r"\\?\C:\data", r"C:\data")
}

#[test]
fn member_paths_keep_the_prefix_for_trailing_dots_and_long_paths() -> TestResult {
    check(r"\\?\C:\trailing.", r"\\?\C:\trailing.")?;
    // 255 units without the prefix, so every member path reaches MAX_PATH.
    let long = format!(r"\\?\C:\{}", "g".repeat(252));
    check(&long, &long)
}
