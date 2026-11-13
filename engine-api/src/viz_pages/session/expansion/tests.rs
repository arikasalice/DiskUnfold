use super::*;
use du_analysis_index::{
    IngestSink, Observation,
    visualization::{
        bundle,
        geometry::Rect,
        projection::{Options, Source},
    },
};
use du_core_contracts::{
    Id, PathBytes, PathEncoding, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot, ResultVersionRead},
};
fn id(v: u8) -> Id {
    Id::from_v7_bytes([v; 16])
}
fn fixture() -> Result<(CurrentAnalysis, projection::Model, Dispatcher), String> {
    let sink = IngestSink::default();
    let lineage = sink
        .bind_current_analysis("scope", id(1))
        .map_err(|e| format!("{e:?}"))?;
    let root = ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(b"/fixture"),
        },
        volume_id: vec![1],
        file_id: vec![2],
    };
    let mut scan = sink
        .open_forest_in_lineage(id(2), vec![root], lineage)
        .map_err(|e| format!("{e:?}"))?;
    for path in [
        vec![],
        vec![b"a".to_vec()],
        vec![b"a".to_vec(), b"b".to_vec()],
        vec![b"a".to_vec(), b"b".to_vec(), b"c".to_vec()],
        vec![b"z".to_vec()],
    ] {
        scan.add_directory(path).map_err(|e| format!("{e:?}"))?;
    }
    for n in 0..64 {
        scan.add_directory(vec![format!("dir{n:02}").into_bytes()])
            .map_err(|e| format!("{e:?}"))?;
    }
    for (path, size) in [(vec!["a", "b", "c", "leaf"], 30), (vec!["z", "other"], 70)] {
        scan.add_file(Observation {
            path: path.into_iter().map(|x| x.as_bytes().to_vec()).collect(),
            size: Some(SizeValue {
                basis: SizeBasis::Logical,
                evidence: SizeEvidence::Measured,
                amount: SizeAmount::Exact { bytes: size.into() },
            }),
        })
        .map_err(|e| format!("{e:?}"))?;
    }
    scan.seal(id(3), ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let source = crate::current_analysis::admit_selected(&sink, &id(3), None)
        .map_err(|e| format!("{e:?}"))?;
    let parent = ItemRef {
        result_version: id(3),
        item_no: 0,
    };
    let dispatcher = bundle::implemented_dispatcher().map_err(|e| format!("{e:?}"))?;
    let model = projection::project_deferred(
        Source {
            version: source.version(),
            parent: &parent,
            basis: SizeBasis::Logical,
            bounds: Rect {
                left: 0,
                top: 0,
                right: 1000000,
                bottom: 1000000,
            },
            keys: &[],
        },
        &dispatcher,
        Options {
            format: "treemap",
            strategy: None,
            max_shapes: 100,
        },
        &mut || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    Ok((source, model, dispatcher))
}
fn policy(
    _: &CurrentAnalysis,
    item: &ResultItem,
    _: &mut dyn FnMut() -> bool,
) -> Result<ChildPolicyEvidence, render::Error> {
    Ok(ChildPolicyEvidence {
        path_compare_key: Some(item.name.clone()),
        legend_class_key: "test".into(),
        display_flags: vec![],
    })
}
fn reference(source: &CurrentAnalysis, name: &[u8]) -> Result<ItemRef, String> {
    let i = source
        .version()
        .items()
        .iter()
        .find(|i| i.name == name)
        .ok_or("missing item")?;
    Ok(ItemRef {
        result_version: source.result_version().clone(),
        item_no: i.item_no,
    })
}
#[test]
fn local_expansion_keeps_other_branch_and_root_rows() -> Result<(), String> {
    let (source, mut baseline, dispatcher) = fixture()?;
    let mut expanded = baseline.clone();
    extend(
        &mut baseline,
        &source,
        &dispatcher,
        Policy {
            base_depth: 2,
            entries: &[],
            max_shapes: 100,
        },
        &mut policy,
        &mut || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    let entry = VizExpansion {
        item_ref: reference(&source, b"a")?,
        extra_depth: 2,
    };
    extend(
        &mut expanded,
        &source,
        &dispatcher,
        Policy {
            base_depth: 2,
            entries: &[entry],
            max_shapes: 100,
        },
        &mut policy,
        &mut || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(baseline.shapes, expanded.shapes);
    assert_eq!(baseline.totals, expanded.totals);
    assert_eq!(
        baseline
            .rows
            .iter()
            .map(|r| (&r.item, &r.size))
            .collect::<Vec<_>>(),
        expanded
            .rows
            .iter()
            .map(|r| (&r.item, &r.size))
            .collect::<Vec<_>>()
    );
    assert!(
        expanded
            .descendants
            .iter()
            .any(|x| x.meta.visible_depth == 4)
    );
    let other = reference(&source, b"z")?;
    assert_eq!(
        baseline
            .descendants
            .iter()
            .filter(|s| s.meta.root_branch_ref.as_ref() == Some(&other))
            .collect::<Vec<_>>(),
        expanded
            .descendants
            .iter()
            .filter(|s| s.meta.root_branch_ref.as_ref() == Some(&other))
            .collect::<Vec<_>>()
    );
    Ok(())
}
#[test]
fn rejects_invalid_entry_and_honours_cancel() -> Result<(), String> {
    let (source, model, _) = fixture()?;
    let a = reference(&source, b"a")?;
    for entries in [
        vec![VizExpansion {
            item_ref: a.clone(),
            extra_depth: 9,
        }],
        vec![
            VizExpansion {
                item_ref: a.clone(),
                extra_depth: 1
            };
            65
        ],
        vec![VizExpansion {
            item_ref: reference(&source, b"leaf")?,
            extra_depth: 1,
        }],
        vec![VizExpansion {
            item_ref: ItemRef {
                result_version: id(9),
                item_no: a.item_no,
            },
            extra_depth: 1,
        }],
        vec![
            VizExpansion {
                item_ref: a.clone(),
                extra_depth: 1
            };
            2
        ],
    ] {
        assert!(normalize_expansions(&source, &model.parent, 2, &entries, &mut || false).is_err());
    }
    assert!(matches!(
        normalize_expansions(&source, &model.parent, 2, &[], &mut || true),
        Err(ExpansionError::Cancelled)
    ));
    assert!(matches!(
        normalize_expansions(
            &source,
            &model.parent,
            2,
            &[VizExpansion {
                item_ref: a,
                extra_depth: 1
            }],
            &mut || true
        ),
        Err(ExpansionError::Cancelled)
    ));
    Ok(())
}

#[test]
fn boundary_and_hidden_ancestor_requests_are_validated() -> Result<(), String> {
    let (source, model, dispatcher) = fixture()?;
    let entries: Vec<_> = source
        .version()
        .items()
        .iter()
        .filter(|i| i.name.starts_with(b"dir"))
        .map(|i| VizExpansion {
            item_ref: ItemRef {
                result_version: id(3),
                item_no: i.item_no,
            },
            extra_depth: 8,
        })
        .collect();
    assert_eq!(entries.len(), 64);
    assert_eq!(
        normalize_expansions(&source, &model.parent, 2, &entries, &mut || false)
            .map_err(|e| format!("{e:?}"))?
            .len(),
        64
    );
    let mut too_many = entries.clone();
    too_many.push(entries[0].clone());
    assert!(normalize_expansions(&source, &model.parent, 2, &too_many, &mut || false).is_err());
    let outside = VizExpansion {
        item_ref: reference(&source, b"z")?,
        extra_depth: 1,
    };
    assert!(
        normalize_expansions(
            &source,
            &reference(&source, b"a")?,
            2,
            &[outside],
            &mut || { false }
        )
        .is_err()
    );
    let mut normal = model.clone();
    let mut hidden = model;
    extend(
        &mut normal,
        &source,
        &dispatcher,
        Policy {
            base_depth: 2,
            entries: &[],
            max_shapes: 100,
        },
        &mut policy,
        &mut || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    let hidden_entry = VizExpansion {
        item_ref: reference(&source, b"c")?,
        extra_depth: 8,
    };
    extend(
        &mut hidden,
        &source,
        &dispatcher,
        Policy {
            base_depth: 2,
            entries: &[hidden_entry],
            max_shapes: 100,
        },
        &mut policy,
        &mut || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(normal, hidden);
    Ok(())
}

#[test]
fn normalizes_zero_and_closed_descendants_using_topology() -> Result<(), String> {
    let (source, model, _) = fixture()?;
    let a = reference(&source, b"a")?;
    let c = reference(&source, b"c")?;
    let hidden = VizExpansion {
        item_ref: c.clone(),
        extra_depth: 8,
    };
    assert!(
        normalize_expansions(
            &source,
            &model.parent,
            2,
            std::slice::from_ref(&hidden),
            &mut || false
        )
        .map_err(|e| format!("{e:?}"))?
        .is_empty()
    );
    let open = VizExpansion {
        item_ref: a.clone(),
        extra_depth: 1,
    };
    let normalized =
        normalize_expansions(&source, &model.parent, 2, &[hidden, open], &mut || false)
            .map_err(|e| format!("{e:?}"))?;
    assert_eq!(normalized.len(), 2);
    assert!(
        normalized
            .windows(2)
            .all(|pair| pair[0].item_ref.item_no < pair[1].item_ref.item_no)
    );
    let zero = VizExpansion {
        item_ref: a,
        extra_depth: 0,
    };
    assert!(
        normalize_expansions(&source, &model.parent, 2, &[zero], &mut || false)
            .map_err(|e| format!("{e:?}"))?
            .is_empty()
    );
    Ok(())
}
