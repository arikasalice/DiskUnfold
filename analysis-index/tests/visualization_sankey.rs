#![allow(clippy::unwrap_used, clippy::panic)]

use du_analysis_index::visualization::{
    geometry::{Rect, Weight},
    sankey::{self, CategoryKey, FolderKey, Invalid, Outcome, Row, Unavailable},
};

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
