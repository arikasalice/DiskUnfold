//! Visualization models keep version-wide completeness and bounded folder values when coverage
//! is unproven (F03 FR5.HD1; the screen compares model and frame completeness).
use du_analysis_index::{
    BuildHandle, IngestSink, Observation, ResultVersion,
    visualization::{
        bar_projection::{self, ChildrenRequest},
        geometry::{Rect, Scale},
        hierarchy,
        projection::{self, Options, Source},
        registry,
        typed_layout::{self, Dispatcher, Strategy},
    },
};
use du_core_contracts::{
    ByteSize, Id, ItemRef, PathBytes, PathEncoding, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot, ResultVersionRead},
};
use std::sync::Arc;

type Test = Result<(), String>;

fn e<T: std::fmt::Debug>(error: T) -> String {
    format!("{error:?}")
}
fn id(n: u8) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}
fn path(parts: &[&str]) -> Vec<Vec<u8>> {
    parts.iter().map(|part| part.as_bytes().to_vec()).collect()
}
fn exact(bytes: u64) -> SizeAmount {
    SizeAmount::Exact {
        bytes: ByteSize::from(bytes),
    }
}
fn lower(bytes: u64) -> SizeAmount {
    SizeAmount::LowerBound {
        bytes: ByteSize::from(bytes),
    }
}
fn add(build: &mut BuildHandle, parts: &[&str], bytes: u64) -> Test {
    build
        .add_file(Observation {
            path: path(parts),
            size: Some(SizeValue {
                basis: SizeBasis::Logical,
                evidence: SizeEvidence::Measured,
                amount: exact(bytes),
            }),
        })
        .map_err(e)
}

/// A Missing version with a gap and sibling observations without separate coverage evidence.
fn missing_version() -> Result<Arc<ResultVersion>, String> {
    let root = ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(b"/r"),
        },
        volume_id: vec![1],
        file_id: vec![1],
    };
    let mut build = IngestSink::default()
        .open_forest(id(1)?, vec![root])
        .map_err(e)?;
    build.add_directory_at(0, vec![]).map_err(e)?;
    for parts in [&["a"][..], &["a", "bad"], &["s"]] {
        build.add_directory_at(0, path(parts)).map_err(e)?;
    }
    add(&mut build, &["a", "af"], 10)?;
    add(&mut build, &["s", "sf1"], 20)?;
    add(&mut build, &["s", "sf2"], 5)?;
    build
        .record_gap_at(0, path(&["a", "bad"]), "scan.path_denied".into(), None)
        .map_err(e)?;
    build.seal(id(2)?, ResultCompleteness::Missing).map_err(e)
}

fn reference(version: &ResultVersion, name: &str) -> Result<ItemRef, String> {
    let item = version
        .items()
        .iter()
        .find(|item| item.name == name.as_bytes())
        .ok_or_else(|| format!("no item {name}"))?;
    Ok(ItemRef {
        result_version: version.version_id().clone(),
        item_no: item.item_no,
    })
}
fn bounds() -> Rect {
    Rect {
        left: 0,
        top: 0,
        right: 1_000_000,
        bottom: 1_000_000,
    }
}

#[test]
fn treemap_missing_lower_bound_requires_attribution_proof() -> Test {
    let version = missing_version()?;
    let parent = reference(&version, "s")?;
    let descriptor = typed_layout::treemap_descriptor();
    let mut builder = registry::Builder::<Box<dyn Strategy>>::default();
    builder
        .register(descriptor.clone(), Box::new(typed_layout::Treemap))
        .map_err(e)?;
    let dispatcher = Dispatcher::new(builder.build(&[descriptor]).map_err(e)?);
    let result = projection::project(
        Source {
            version: &version,
            parent: &parent,
            basis: SizeBasis::Logical,
            bounds: bounds(),
        },
        &dispatcher,
        Options {
            format: "treemap",
            strategy: None,
            max_shapes: 10,
        },
        &mut || false,
    );
    assert_eq!(version.completeness(), ResultCompleteness::Missing);
    assert_eq!(
        result.err(),
        Some(projection::Error::UnavailableAttribution)
    );
    Ok(())
}

#[test]
fn ranked_bar_keeps_version_completeness_and_lower_bound_values() -> Test {
    let version = missing_version()?;
    let scope = reference(&version, "s")?;
    let ranking = bar_projection::project_children(
        ChildrenRequest {
            version: &version,
            scope: &scope,
            basis: SizeBasis::Logical,
            bounds: bounds(),
            scale: Scale::Fixed(50),
            top: 2,
            max_bars: 3,
        },
        &mut || false,
    )
    .map_err(e)?;
    assert_eq!(ranking.completeness, version.completeness());
    assert_eq!(ranking.completeness, ResultCompleteness::Missing);
    assert_eq!(ranking.scope_total.size.amount, lower(25));
    Ok(())
}

#[test]
fn hierarchy_keeps_version_completeness_and_lower_bound_values() -> Test {
    let version = missing_version()?;
    let folder = reference(&version, "s")?;
    let model = hierarchy(&version, &folder, &folder, SizeBasis::Logical, 1, &[]).map_err(e)?;
    assert_eq!(model.completeness, version.completeness());
    assert_eq!(model.completeness, ResultCompleteness::Missing);
    let root = model.rows.first().ok_or("root row")?;
    assert_eq!(root.value.amount, lower(25));
    Ok(())
}
