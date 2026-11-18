//! Summary reason keys come from the collector's own gap reports.
use super::*;
use du_core_contracts::{
    PathBytes, PathEncoding,
    result::{ResultNativePath, ResultScopeRoot},
};

#[test]
fn summary_reasons_are_the_collectors_gap_keys_inside_the_scope_only() {
    let root = ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(b"/root"),
        },
        volume_id: vec![1],
        file_id: vec![1],
    };
    let sink = IngestSink::default();
    let mut b = sink.open_observed(id(1), root).unwrap();
    for dir in [vec![], vec![b"in".to_vec()], vec![b"out".to_vec()]] {
        b.add_directory(dir).unwrap();
    }
    for name in [b"in".to_vec(), b"out".to_vec()] {
        b.add_file(Observation {
            path: vec![name, b"file".to_vec()],
            size: Some(logical(10)),
        })
        .unwrap();
    }
    for (name, reason) in [
        (&b"in"[..], "scan.path_denied"),
        (&b"out"[..], "scan.other"),
    ] {
        b.record_gap_at(
            0,
            vec![name.to_vec(), b"unread".to_vec()],
            reason.into(),
            None,
        )
        .unwrap();
    }
    // A gap inside a folder makes that folder's total a lower bound, which a partial
    // result still partitions.
    let version = b.seal(id(2), ResultCompleteness::Partial).unwrap();
    let scope = |name: &str| {
        let parent = find(&version, name);
        projection::project(
            Source {
                version: &version,
                parent: &parent,
                basis: SizeBasis::Logical,
                bounds: WINDOW,
            },
            &dispatcher(),
            Options {
                format: "treemap",
                strategy: None,
                max_shapes: 16,
                group_tail: true,
            },
            &mut || false,
        )
        .unwrap()
        .detail
        .reason_keys
    };
    assert_eq!(scope("in"), ["scan.path_denied"]);
    assert_eq!(scope("out"), ["scan.other"]);
    let all = project(&version, SizeBasis::Logical, 16).unwrap();
    assert_eq!(all.detail.reason_keys, ["scan.other", "scan.path_denied"]);
    // A result with no reported gap gives no reason at all.
    let clean = build(&many(3, 10), ResultCompleteness::Complete);
    let model = project(&clean, SizeBasis::Logical, 16).unwrap();
    assert!(model.detail.reason_keys.is_empty());
}
