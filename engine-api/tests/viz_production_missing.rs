//! A folder without independent coverage evidence stays bounded in a Missing result; the model
//! carries the same version-wide completeness as the response frame and rows page (F03 FR5.HD1).
use du_analysis_index::{
    IngestSink, Observation, ResultVersion,
    visualization::{bundle, geometry::Rect},
};
use du_core_contracts::{
    Id, ItemRef, PathBytes, PathEncoding, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot, ResultVersionRead},
    visualization::rows::VizRowsRequest,
};
use du_engine_api::{
    api::Channel,
    viz_pages::{
        render::{Error, Request},
        session::{ChildPolicyEvidence, Session},
    },
};
use du_job_runtime::JobIdentity;
use std::{
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

struct Ids(AtomicU64);
impl JobIdentity for Ids {
    fn next_id(&self) -> Id {
        let mut bytes = [0; 16];
        bytes[8..].copy_from_slice(&self.0.fetch_add(1, Ordering::SeqCst).to_be_bytes());
        Id::from_v7_bytes(bytes)
    }
}
fn e<T: std::fmt::Debug>(error: T) -> String {
    format!("{error:?}")
}
fn id(n: u32) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}
fn path(parts: &[&str]) -> Vec<Vec<u8>> {
    parts.iter().map(|part| part.as_bytes().to_vec()).collect()
}
fn file(bytes: u64) -> Option<SizeValue> {
    Some(SizeValue {
        basis: SizeBasis::Logical,
        evidence: SizeEvidence::Measured,
        amount: SizeAmount::Exact {
            bytes: bytes.into(),
        },
    })
}

/// Folder "a" has a gap; folder "s" has observations but no independent coverage evidence.
fn missing_result(sink: &IngestSink) -> Result<Arc<ResultVersion>, String> {
    let root = ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(b"/r"),
        },
        volume_id: vec![1],
        file_id: vec![1],
    };
    let lineage = sink
        .bind_current_analysis("missing-visualization", id(1)?)
        .map_err(e)?;
    let mut b = sink
        .open_forest_in_lineage(id(2)?, vec![root], lineage)
        .map_err(e)?;
    b.add_directory_at(0, vec![]).map_err(e)?;
    for parts in [&["a"][..], &["a", "bad"], &["s"]] {
        b.add_directory_at(0, path(parts)).map_err(e)?;
    }
    for (parts, bytes) in [(&["a", "af"][..], 10), (&["s", "x"], 25), (&["s", "y"], 75)] {
        b.add_file(Observation {
            path: path(parts),
            size: file(bytes),
        })
        .map_err(e)?;
    }
    b.record_gap_at(0, path(&["a", "bad"]), "scan.path_denied".into(), None)
        .map_err(e)?;
    b.seal(id(3)?, ResultCompleteness::Missing).map_err(e)
}

#[test]
fn production_viz_uses_same_version_no_attribution_fallback() -> Result<(), String> {
    let sink = IngestSink::default();
    let version = missing_result(&sink)?;
    assert_eq!(version.completeness(), ResultCompleteness::Missing);
    let folder = version
        .items()
        .iter()
        .find(|item| item.name == b"s")
        .ok_or("folder s")?;
    let parent = ItemRef {
        result_version: version.version_id().clone(),
        item_no: folder.item_no,
    };
    let session = Session::new(
        sink,
        NonZeroUsize::new(3).ok_or("zero")?,
        Arc::new(Ids(AtomicU64::new(10))),
        bundle::implemented_dispatcher().map_err(e)?,
    );
    let fallback = match session.render_selected(
        Request {
            selected_revision: version.version_id(),
            requested_revision: None,
            channel: Channel::Gui,
            parent: &parent,
            basis: SizeBasis::Logical,
            bounds: Rect {
                left: 0,
                top: 0,
                right: 1_000_000,
                bottom: 1_000_000,
            },
            format: "treemap",
            strategy: None,
            max_shapes: 10,
        },
        |_, _, _| {
            Ok(ChildPolicyEvidence {
                path_compare_key: None,
                legend_class_key: "source.class".into(),
                display_flags: vec![],
            })
        },
        &mut || false,
    ) {
        Err(Error::NoAttribution(fallback)) => fallback,
        Err(error) => return Err(format!("expected no-attribution fallback, got {error:?}")),
        Ok(_) => return Err("unproven lower bounds unexpectedly received treemap areas".into()),
    };
    assert_eq!(fallback.result_revision, *version.version_id());
    let page = session
        .rows(&VizRowsRequest {
            model_ref: fallback.model_ref,
            result_revision: version.version_id().clone(),
            start: 0,
            count: 10,
        })
        .map_err(e)?;
    assert_eq!(page.rows.len(), 2);
    assert_eq!(page.completeness, version.completeness());
    assert_eq!(page.result_revision, *version.version_id());
    let mut values: Vec<_> = page
        .rows
        .iter()
        .map(|row| (row.display_name.clone(), row.size.amount.clone()))
        .collect();
    values.sort_by(|left, right| left.0.cmp(&right.0));
    assert_eq!(
        values,
        vec![
            ("x".into(), SizeAmount::LowerBound { bytes: 25.into() }),
            ("y".into(), SizeAmount::LowerBound { bytes: 75.into() }),
        ]
    );
    for row in &page.rows {
        assert_eq!(
            row.denominator.size.as_ref(),
            version
                .space(parent.item_no)
                .map(|aggregate| &aggregate.logical)
        );
        assert_eq!(row.ratio, None);
    }
    Ok(())
}
