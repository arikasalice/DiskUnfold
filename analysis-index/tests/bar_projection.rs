use du_analysis_index::{
    IngestSink, Observation, ResultVersion,
    visualization::{
        bar_projection::{
            self, AggregateEvidence, AggregateTarget, BarIdentity, ChildrenRequest, Error, Request,
            Row,
        },
        geometry::{Rect, Scale, Weight},
    },
};
use du_core_contracts::{
    Id, ItemRef, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    result::{ResultCompleteness, ResultVersionRead},
};
use std::sync::Arc;

fn id(n: u8) -> Id {
    Id::from_v7_bytes([n; 16])
}

#[test]
fn pinned_children_build_the_same_ranked_model_as_verified_f03_rows() -> Result<(), String> {
    let version = fixture(ResultCompleteness::Complete)?;
    let scope = reference(&version, 0);
    let all = rows(&version)?;
    let total = aggregate(&version)?;
    let expected =
        bar_projection::project(request(&version, &all, &scope, &total, 2, 3), &mut || false)
            .map_err(|e| format!("{e:?}"))?;
    let input = || ChildrenRequest {
        version: &version,
        scope: &scope,
        basis: SizeBasis::Logical,
        bounds: Rect {
            left: 0,
            top: 0,
            right: 1_000_000,
            bottom: 1_000_000,
        },
        scale: Scale::Fixed(50),
        top: 2,
        max_bars: 3,
    };
    let model =
        bar_projection::project_children(input(), &mut || false).map_err(|e| format!("{e:?}"))?;
    assert_eq!(model, expected);
    assert!(model.bars.iter().any(|bar| bar.geometry.is_some()));
    assert_eq!(
        bar_projection::project_children(input(), &mut || false),
        Ok(model)
    );
    assert_eq!(
        bar_projection::project_children(input(), &mut || true),
        Err(Error::Cancelled)
    );
    let mut limited = input();
    limited.max_bars = 2;
    assert_eq!(
        bar_projection::project_children(limited, &mut || false),
        Err(Error::LimitExceeded)
    );
    let stale = ItemRef {
        result_version: id(9),
        item_no: 0,
    };
    let mut wrong = input();
    wrong.scope = &stale;
    assert_eq!(
        bar_projection::project_children(wrong, &mut || false),
        Err(Error::InvalidEvidence)
    );
    Ok(())
}
fn fixture(completeness: ResultCompleteness) -> Result<Arc<ResultVersion>, String> {
    let sink = IngestSink::default();
    let mut build = sink.open(id(1)).map_err(|e| format!("{e:?}"))?;
    build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    for (name, bytes) in [
        (b'a', Some(25)),
        (b'b', Some(15)),
        (b'c', Some(10)),
        (b'z', Some(0)),
        (b'u', None),
    ] {
        build
            .add_file(Observation {
                path: vec![vec![name]],
                size: bytes.map(|n| SizeValue {
                    basis: SizeBasis::Logical,
                    evidence: SizeEvidence::Measured,
                    amount: SizeAmount::Exact { bytes: n.into() },
                }),
            })
            .map_err(|e| format!("{e:?}"))?;
    }
    build
        .seal(id(2), completeness)
        .map_err(|e| format!("{e:?}"))
}
fn reference(version: &ResultVersion, n: u32) -> ItemRef {
    ItemRef {
        result_version: version.version_id().clone(),
        item_no: n,
    }
}
fn rows(version: &ResultVersion) -> Result<Vec<Row>, String> {
    (1..version.items().len() as u32)
        .map(|n| {
            Ok(Row {
                item: reference(version, n),
                size: version.space(n).ok_or("missing aggregate")?.logical.clone(),
            })
        })
        .collect()
}
fn aggregate(version: &ResultVersion) -> Result<AggregateEvidence, String> {
    Ok(AggregateEvidence {
        result_version: version.version_id().clone(),
        target: AggregateTarget::Folder(reference(version, 0)),
        size: version.space(0).ok_or("missing root")?.logical.clone(),
    })
}
fn request<'a>(
    version: &'a ResultVersion,
    rows: &'a [Row],
    scope: &'a ItemRef,
    aggregate: &'a AggregateEvidence,
    top: usize,
    max_bars: usize,
) -> Request<'a> {
    Request {
        version,
        scope,
        basis: SizeBasis::Logical,
        rows,
        scope_total: aggregate,
        denominator: aggregate,
        bounds: Rect {
            left: 0,
            top: 0,
            right: 1_000_000,
            bottom: 1_000_000,
        },
        scale: Scale::Fixed(50),
        top,
        max_bars,
    }
}

#[test]
fn ranking_retains_pinned_values_other_members_and_independent_f03_denominator()
-> Result<(), String> {
    let version = fixture(ResultCompleteness::Complete)?;
    let all = rows(&version)?;
    let scope = reference(&version, 0);
    let aggregate = aggregate(&version)?;
    let result = bar_projection::project(
        request(&version, &all, &scope, &aggregate, 2, 3),
        &mut || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(result.rows, all);
    assert_eq!(result.scope_total, aggregate);
    assert_eq!(result.denominator, aggregate);
    assert_eq!(result.bars.len(), 3);
    assert_eq!(result.bars[0].value, Weight::Exact(25));
    assert!(
        matches!(&result.bars[2].identity, BarIdentity::Other { members, size } if members.len() == 2 && size.amount == SizeAmount::Exact { bytes: 10.into() })
    );
    let unknown = version
        .items()
        .iter()
        .find(|item| item.name == b"u")
        .ok_or("missing unknown")?;
    assert_eq!(
        result.unknown_items,
        vec![reference(&version, unknown.item_no)]
    );
    assert!(result.ranking_incomplete);
    Ok(())
}

#[test]
fn invalid_source_aggregate_and_capacity_fail_closed() -> Result<(), String> {
    let version = fixture(ResultCompleteness::Complete)?;
    let mut all = rows(&version)?;
    let scope = reference(&version, 0);
    let mut aggregate = aggregate(&version)?;
    assert_eq!(
        bar_projection::project(
            request(&version, &all, &scope, &aggregate, 3, 2),
            &mut || false
        ),
        Err(Error::LimitExceeded)
    );
    aggregate.size.amount = SizeAmount::Exact { bytes: 999.into() };
    assert_eq!(
        bar_projection::project(
            request(&version, &all, &scope, &aggregate, 3, 4),
            &mut || false
        ),
        Err(Error::InvalidEvidence)
    );
    aggregate = self::aggregate(&version)?;
    aggregate.result_version = id(9);
    assert_eq!(
        bar_projection::project(
            request(&version, &all, &scope, &aggregate, 3, 4),
            &mut || false
        ),
        Err(Error::InvalidEvidence)
    );
    aggregate = self::aggregate(&version)?;
    all[0].item.result_version = id(9);
    assert_eq!(
        bar_projection::project(
            request(&version, &all, &scope, &aggregate, 3, 4),
            &mut || false
        ),
        Err(Error::InvalidEvidence)
    );
    Ok(())
}

#[test]
fn zero_is_retained_and_ranked_geometry_is_input_order_independent() -> Result<(), String> {
    let version = fixture(ResultCompleteness::Complete)?;
    let all = rows(&version)?;
    let scope = reference(&version, 0);
    let aggregate = aggregate(&version)?;
    let first = bar_projection::project(
        request(&version, &all, &scope, &aggregate, 5, 5),
        &mut || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    let mut reversed = all.clone();
    reversed.reverse();
    let second = bar_projection::project(
        request(&version, &reversed, &scope, &aggregate, 5, 5),
        &mut || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(first.bars, second.bars);
    assert_eq!(first.unknown_items, second.unknown_items);
    assert!(first.bars.iter().any(|bar| bar.value == Weight::Exact(0)
        && bar.geometry.is_none()
        && bar.no_area == Some(du_analysis_index::visualization::geometry::NoArea::ExactZero)));
    Ok(())
}

#[test]
fn partial_values_and_cancellation_never_become_exact_or_zero() -> Result<(), String> {
    let version = fixture(ResultCompleteness::Partial)?;
    let all = rows(&version)?;
    let scope = reference(&version, 0);
    let aggregate = aggregate(&version)?;
    let result = bar_projection::project(
        request(&version, &all, &scope, &aggregate, 5, 5),
        &mut || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(result.completeness, ResultCompleteness::Partial);
    assert!(result.ranking_incomplete);
    assert!(
        result
            .bars
            .iter()
            .any(|bar| matches!(bar.value, Weight::LowerBound(_)))
    );
    assert!(!result.unknown_items.is_empty());
    assert_eq!(
        bar_projection::project(
            request(&version, &all, &scope, &aggregate, 2, 3),
            &mut || false
        ),
        Err(Error::GroupAttributionUnavailable)
    );
    assert_eq!(
        bar_projection::project(
            request(&version, &all, &scope, &aggregate, 5, 5),
            &mut || true
        ),
        Err(Error::Cancelled)
    );
    let mut calls = 0;
    assert_eq!(
        bar_projection::project(
            request(&version, &all, &scope, &aggregate, 5, 5),
            &mut || {
                calls += 1;
                calls > 3
            }
        ),
        Err(Error::Cancelled)
    );
    Ok(())
}

#[test]
fn other_with_unverifiable_folder_members_fails_closed() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = sink.open(id(1)).map_err(|e| format!("{e:?}"))?;
    build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    build
        .add_directory(vec![b"folder".to_vec()])
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_file(Observation {
            path: vec![b"folder".to_vec(), b"file".to_vec()],
            size: Some(SizeValue {
                basis: SizeBasis::Logical,
                evidence: SizeEvidence::Measured,
                amount: SizeAmount::Exact { bytes: 10.into() },
            }),
        })
        .map_err(|e| format!("{e:?}"))?;
    let version = build
        .seal(id(2), ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let folder = version
        .items()
        .iter()
        .find(|item| item.name == b"folder")
        .ok_or("missing folder")?;
    let selected = vec![Row {
        item: reference(&version, folder.item_no),
        size: version
            .space(folder.item_no)
            .ok_or("missing size")?
            .logical
            .clone(),
    }];
    let scope = reference(&version, 0);
    let aggregate = aggregate(&version)?;
    assert_eq!(
        bar_projection::project(
            request(&version, &selected, &scope, &aggregate, 0, 1),
            &mut || false
        ),
        Err(Error::GroupAttributionUnavailable)
    );
    Ok(())
}

#[test]
fn denominator_must_be_scope_or_whole_analysis_even_for_equal_sibling_values() -> Result<(), String>
{
    let sink = IngestSink::default();
    let mut build = sink.open(id(1)).map_err(|e| format!("{e:?}"))?;
    build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    for name in [b"a".as_slice(), b"b".as_slice()] {
        build
            .add_directory(vec![name.to_vec()])
            .map_err(|e| format!("{e:?}"))?;
        build
            .add_file(Observation {
                path: vec![name.to_vec(), b"file".to_vec()],
                size: Some(SizeValue {
                    basis: SizeBasis::Logical,
                    evidence: SizeEvidence::Measured,
                    amount: SizeAmount::Exact { bytes: 10.into() },
                }),
            })
            .map_err(|e| format!("{e:?}"))?;
    }
    let version = build
        .seal(id(2), ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let find = |name: &[u8]| {
        version
            .items()
            .iter()
            .find(|item| item.name == name)
            .map(|item| item.item_no)
            .ok_or("missing folder")
    };
    let a = find(b"a")?;
    let b = find(b"b")?;
    let scope = reference(&version, a);
    let scope_evidence = AggregateEvidence {
        result_version: version.version_id().clone(),
        target: AggregateTarget::Folder(scope.clone()),
        size: version.space(a).ok_or("missing scope")?.logical.clone(),
    };
    let sibling = AggregateEvidence {
        result_version: version.version_id().clone(),
        target: AggregateTarget::Folder(reference(&version, b)),
        size: version.space(b).ok_or("missing sibling")?.logical.clone(),
    };
    assert_eq!(scope_evidence.size, sibling.size);
    let rows: Vec<Row> = version
        .items()
        .iter()
        .filter(|item| item.parent == Some(a))
        .map(|item| {
            Ok(Row {
                item: reference(&version, item.item_no),
                size: version
                    .space(item.item_no)
                    .ok_or("missing row")?
                    .logical
                    .clone(),
            })
        })
        .collect::<Result<_, &str>>()?;
    let base = || request(&version, &rows, &scope, &scope_evidence, 2, 2);
    let valid = bar_projection::project(base(), &mut || false).map_err(|e| format!("{e:?}"))?;
    assert_eq!(valid.denominator, scope_evidence);
    let wrong = Request {
        denominator: &sibling,
        ..base()
    };
    assert_eq!(
        bar_projection::project(wrong, &mut || false),
        Err(Error::InvalidEvidence)
    );
    let whole = AggregateEvidence {
        result_version: version.version_id().clone(),
        target: AggregateTarget::Analysis,
        size: version.scope_total().logical.clone(),
    };
    let valid = bar_projection::project(
        Request {
            denominator: &whole,
            ..base()
        },
        &mut || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(valid.denominator, whole);
    Ok(())
}
