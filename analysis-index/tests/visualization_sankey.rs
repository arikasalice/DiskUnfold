#![allow(clippy::unwrap_used, clippy::panic)]

use du_analysis_index::visualization::{bundle, registry};
use du_analysis_index::visualization::{
    geometry::{Rect, Weight},
    sankey::{self, CategoryKey, FolderKey, Invalid, Outcome, Row, Unavailable},
    typed_layout::{Dispatcher, SankeyDispatchError, SankeyInput, Strategy},
};
use du_core_contracts::visualization::{LayoutDescriptor, LayoutStrategyKey};

fn columns() -> sankey::Columns {
    sankey::Columns {
        volume: Rect {
            left: 0,
            right: 10,
            top: 0,
            bottom: 100,
        },
        folders: Rect {
            left: 20,
            right: 30,
            top: 0,
            bottom: 100,
        },
        categories: Rect {
            left: 40,
            right: 50,
            top: 0,
            bottom: 100,
        },
    }
}
fn row(folder: u32, category: u32, bytes: i128) -> Row {
    Row {
        folder,
        folder_order: folder,
        category,
        bytes: Some(bytes),
    }
}

#[test]
fn conserves_every_node_and_groups_tail_with_explicit_remainder() {
    let rows = [row(1, 10, 50), row(2, 10, 30), row(3, 20, 10)];
    let Outcome::Graph(graph) = sankey::layout(columns(), Weight::Exact(100), &rows, 2, &[10, 20])
    else {
        panic!("expected graph")
    };
    assert_eq!(graph.unattributed, 10);
    assert_eq!(graph.volume.bytes, 100);
    assert_eq!(graph.folders.iter().map(|n| n.bytes).sum::<u64>(), 100);
    assert_eq!(graph.categories.iter().map(|n| n.bytes).sum::<u64>(), 100);
    let other = graph
        .folders
        .iter()
        .find(|n| n.key == FolderKey::Other)
        .unwrap();
    assert_eq!((other.bytes, &other.members), (40, &vec![2, 3]));
    for node in &graph.folders {
        assert_eq!(
            node.bytes,
            graph
                .links
                .iter()
                .filter(|l| l.folder == node.key)
                .map(|l| l.bytes)
                .sum::<u64>()
        );
        if let Some(rect) = node.rect {
            assert!(rect.top < rect.bottom && rect.bottom <= 100);
        }
    }
    for node in &graph.categories {
        assert_eq!(
            node.bytes,
            graph
                .links
                .iter()
                .filter(|l| l.category == node.key)
                .map(|l| l.bytes)
                .sum::<u64>()
        );
    }
    assert!(
        graph
            .links
            .iter()
            .any(|l| l.folder == FolderKey::Unattributed
                && l.category == CategoryKey::Unattributed
                && l.bytes == 10)
    );
    let mut reversed = rows;
    reversed.reverse();
    assert_eq!(
        sankey::layout(columns(), Weight::Exact(100), &reversed, 2, &[10, 20]),
        Outcome::Graph(graph)
    );
}

#[test]
fn rejects_unavailable_or_inconsistent_facts() {
    let base = [row(1, 10, 3)];
    assert_eq!(
        sankey::layout(columns(), Weight::Exact(3), &[], 1, &[10]),
        Outcome::Unavailable(Unavailable::NoAttribution)
    );
    assert_eq!(
        sankey::layout(columns(), Weight::LowerBound(3), &base, 1, &[10]),
        Outcome::Unavailable(Unavailable::UncertainScope)
    );
    assert_eq!(
        sankey::layout(columns(), Weight::Exact(3), &[row(1, 10, -1)], 1, &[10]),
        Outcome::Unavailable(Unavailable::NegativeFlow)
    );
    assert_eq!(
        sankey::layout(
            columns(),
            Weight::Exact(3),
            &[Row {
                bytes: None,
                ..base[0]
            }],
            1,
            &[10]
        ),
        Outcome::Unavailable(Unavailable::UnknownAttribution)
    );
    assert_eq!(
        sankey::layout(columns(), Weight::Exact(2), &base, 1, &[10]),
        Outcome::Invalid(Invalid::ExceedsScope)
    );
    assert_eq!(
        sankey::layout(columns(), Weight::Exact(3), &base, 0, &[10]),
        Outcome::Invalid(Invalid::FolderBudget)
    );
    assert_eq!(
        sankey::layout(columns(), Weight::Exact(3), &base, 1, &[20]),
        Outcome::Invalid(Invalid::CategoryOrder)
    );
    assert_eq!(
        sankey::layout(
            columns(),
            Weight::Exact(u64::MAX),
            &[row(1, 10, i128::from(u64::MAX)), row(2, 10, 1)],
            2,
            &[10]
        ),
        Outcome::Unavailable(Unavailable::Overflow)
    );
}

#[test]
fn zero_and_ties_are_deterministic() {
    let rows = [row(2, 10, 1), row(1, 10, 1)];
    let Outcome::Graph(graph) = sankey::layout(columns(), Weight::Exact(2), &rows, 1, &[10]) else {
        panic!("expected graph")
    };
    assert_eq!(graph.folders[0].members, vec![1, 2]);
    let Outcome::Graph(zero) =
        sankey::layout(columns(), Weight::Exact(0), &[row(1, 10, 0)], 1, &[10])
    else {
        panic!("expected zero graph")
    };
    assert!(zero.volume.rect.is_none());
    assert!(zero.folders.iter().all(|node| node.rect.is_none()));
}

fn selected_sankey() -> Dispatcher {
    let mut builder = registry::Builder::<Box<dyn Strategy>>::default();
    bundle::register_sankey_candidate(&mut builder).unwrap();
    let required = LayoutDescriptor {
        format: "sankey".into(),
        strategy: LayoutStrategyKey {
            id: "viz.sankey".into(),
            revision: 1,
        },
    };
    Dispatcher::new(builder.build(&[required]).unwrap())
}

#[test]
fn registered_selection_executes_conserved_pinned_provider_facts() {
    // This fixture exercises the intended trusted provider boundary only;
    // no production provider is wired into analysis-index yet.
    let rows = [row(1, 10, 5), row(2, 20, 7), row(3, 10, 2)];
    let selected = selected_sankey()
        .execute_sankey(
            Some(&LayoutStrategyKey {
                id: "viz.sankey".into(),
                revision: 1,
            }),
            SankeyInput {
                columns: columns(),
                scope: Weight::Exact(15),
                rows: &rows,
                folder_budget: 2,
                category_order: &[10, 20],
            },
            &mut || false,
        )
        .unwrap();
    assert_eq!(selected.descriptor.strategy.id, "viz.sankey");
    assert_eq!(selected.descriptor.strategy.revision, 1);
    assert_eq!(selected.output.graph.total, 15);
    assert_eq!(selected.output.graph.unattributed, 1);
    assert_eq!(
        selected
            .output
            .graph
            .folders
            .iter()
            .map(|node| node.bytes)
            .sum::<u64>(),
        15
    );
    assert_eq!(
        selected
            .output
            .graph
            .categories
            .iter()
            .map(|node| node.bytes)
            .sum::<u64>(),
        15
    );
    assert_eq!(
        selected.output.relations.volume_to_folder.len(),
        selected.output.graph.folders.len()
    );
    assert_eq!(
        selected.output.relations.folder_to_category.len(),
        selected.output.graph.links.len()
    );
    let other = selected
        .output
        .graph
        .folders
        .iter()
        .find(|node| node.key == FolderKey::Other)
        .unwrap();
    assert_eq!(other.members, vec![1, 3]);
    assert_eq!(
        selected
            .output
            .relations
            .folder_to_category
            .iter()
            .map(|relation| relation.bytes)
            .collect::<Vec<_>>(),
        selected
            .output
            .graph
            .links
            .iter()
            .map(|link| link.bytes)
            .collect::<Vec<_>>()
    );
}

#[test]
fn selected_strategy_fails_closed_for_unknown_negative_invalid_and_overflow_facts() {
    let dispatcher = selected_sankey();
    let run = |rows: &[Row], categories: &[u32], cancelled: &mut dyn FnMut() -> bool| {
        dispatcher.execute_sankey(
            None,
            SankeyInput {
                columns: columns(),
                scope: Weight::Exact(10),
                rows,
                folder_budget: 2,
                category_order: categories,
            },
            cancelled,
        )
    };
    assert_eq!(
        run(&[row(1, 10, -1)], &[10], &mut || false).unwrap_err(),
        SankeyDispatchError::Unavailable(Unavailable::NegativeFlow)
    );
    assert_eq!(
        run(
            &[Row {
                bytes: None,
                ..row(1, 10, 1)
            }],
            &[10],
            &mut || false
        )
        .unwrap_err(),
        SankeyDispatchError::Unavailable(Unavailable::UnknownAttribution)
    );
    assert_eq!(
        run(&[row(1, 99, 1)], &[10], &mut || false).unwrap_err(),
        SankeyDispatchError::Invalid(Invalid::CategoryOrder)
    );
    assert_eq!(
        run(
            &[row(1, 10, i128::from(u64::MAX)), row(2, 10, 1)],
            &[10],
            &mut || false
        )
        .unwrap_err(),
        SankeyDispatchError::Unavailable(Unavailable::Overflow)
    );
    assert_eq!(
        run(&[row(1, 10, 1)], &[10], &mut || true).unwrap_err(),
        SankeyDispatchError::Cancelled
    );
    let mut checks = 0;
    let mut cancel_during_rows = || {
        checks += 1;
        checks >= 2
    };
    assert_eq!(
        run(&[row(1, 10, 1)], &[10], &mut cancel_during_rows).unwrap_err(),
        SankeyDispatchError::Cancelled
    );
    let mut checks = 0;
    let mut cancel_during_aggregation = || {
        checks += 1;
        checks >= 11
    };
    let aggregation_cancel = dispatcher.execute_sankey(
        None,
        SankeyInput {
            columns: columns(),
            scope: Weight::Exact(15),
            rows: &[row(1, 10, 5), row(2, 20, 7), row(3, 10, 2)],
            folder_budget: 2,
            category_order: &[10, 20],
        },
        &mut cancel_during_aggregation,
    );
    assert_eq!(
        aggregation_cancel.unwrap_err(),
        SankeyDispatchError::Cancelled
    );
    let mut checks = 0;
    let mut cancel_during_geometry = || {
        checks += 1;
        checks >= 15
    };
    let geometry_cancel = dispatcher.execute_sankey(
        None,
        SankeyInput {
            columns: columns(),
            scope: Weight::Exact(15),
            rows: &[row(1, 10, 5), row(2, 20, 7), row(3, 10, 2)],
            folder_budget: 2,
            category_order: &[10, 20],
        },
        &mut cancel_during_geometry,
    );
    assert_eq!(geometry_cancel.unwrap_err(), SankeyDispatchError::Cancelled);
}

#[test]
fn selected_geometry_failure_keeps_the_validated_source_graph() {
    let tiny = sankey::Columns {
        volume: Rect {
            bottom: 1,
            ..columns().volume
        },
        folders: Rect {
            bottom: 1,
            ..columns().folders
        },
        categories: Rect {
            bottom: 1,
            ..columns().categories
        },
    };
    let error = selected_sankey()
        .execute_sankey(
            None,
            SankeyInput {
                columns: tiny,
                scope: Weight::Exact(2),
                rows: &[row(1, 10, 1), row(2, 10, 1)],
                folder_budget: 2,
                category_order: &[10],
            },
            &mut || false,
        )
        .unwrap_err();
    let SankeyDispatchError::Geometry { graph, .. } = error else {
        panic!("expected geometry failure")
    };
    assert_eq!(graph.total, 2);
    assert_eq!(graph.folders.iter().map(|node| node.bytes).sum::<u64>(), 2);
}
