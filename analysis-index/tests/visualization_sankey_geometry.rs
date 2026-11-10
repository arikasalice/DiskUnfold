use du_analysis_index::visualization::{
    geometry::{Rect, Weight},
    sankey::{
        self, CategoryKey, FolderKey, Outcome, Row, Unavailable,
        geometry_v1::{Endpoint, Error},
    },
};
use du_core_contracts::visualization::geometry::{COORDINATE_MAX, Point};

fn columns(height: u32) -> sankey::Columns {
    sankey::Columns {
        volume: Rect {
            left: 0,
            right: 100,
            top: 0,
            bottom: height,
        },
        folders: Rect {
            left: 200,
            right: 300,
            top: 0,
            bottom: height,
        },
        categories: Rect {
            left: 400,
            right: 500,
            top: 0,
            bottom: height,
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
fn graph(rows: &[Row], total: u64, height: u32) -> Result<sankey::Graph, String> {
    match sankey::layout(columns(height), Weight::Exact(total), rows, 4, &[10, 20]) {
        Outcome::Graph(graph) => Ok(graph),
        other => Err(format!("{other:?}")),
    }
}
fn p(x: u32, y: u32) -> Point {
    Point { x, y }
}

#[test]
fn both_stages_conserve_and_allocate_ordered_closed_slots() -> Result<(), String> {
    let rows = [
        row(1, 10, 40),
        row(1, 20, 20),
        row(2, 10, 30),
        row(2, 20, 10),
    ];
    let source = graph(&rows, 100, 1000)?;
    let before = source.clone();
    let result = source.geometry_v1().map_err(|e| format!("{e:?}"))?;
    assert_eq!(source, before);
    assert_eq!(
        result
            .volume_to_folder
            .iter()
            .map(|r| r.bytes)
            .collect::<Vec<_>>(),
        vec![60, 40]
    );
    assert_eq!(
        result
            .folder_to_category
            .iter()
            .map(|r| r.bytes)
            .collect::<Vec<_>>(),
        vec![40, 20, 30, 10]
    );
    assert_eq!(
        (
            result.volume_to_folder[0].from,
            result.volume_to_folder[0].to
        ),
        (Endpoint::Volume, Endpoint::Folder(FolderKey::Folder(1)))
    );
    assert_eq!(
        result.volume_to_folder[0]
            .geometry
            .as_ref()
            .ok_or("polygon")?
            .points(),
        &[p(100, 0), p(200, 0), p(200, 600), p(100, 600)]
    );
    let connections = &result.folder_to_category;
    assert_eq!(
        (connections[1].from, connections[1].to),
        (
            Endpoint::Folder(FolderKey::Folder(1)),
            Endpoint::Category(CategoryKey::Category(20))
        )
    );
    assert_eq!(
        connections[1].geometry.as_ref().ok_or("polygon")?.points(),
        &[p(300, 400), p(400, 700), p(400, 900), p(300, 600)]
    );
    assert_eq!(
        connections[2].geometry.as_ref().ok_or("polygon")?.points(),
        &[p(300, 600), p(400, 400), p(400, 700), p(300, 900)]
    );
    assert_eq!(
        connections[3].geometry.as_ref().ok_or("polygon")?.points(),
        &[p(300, 900), p(400, 900), p(400, 1000), p(300, 1000)]
    );
    assert!(connections.iter().all(|r| {
        r.geometry
            .as_ref()
            .is_some_and(|g| g.primitive() == "polygon")
    }));
    let mut reversed = rows;
    reversed.reverse();
    assert_eq!(
        graph(&reversed, 100, 1000)?
            .geometry_v1()
            .map_err(|e| format!("{e:?}"))?,
        result
    );
    Ok(())
}

#[test]
fn grouping_remainder_ties_and_u64_max_are_deterministic() -> Result<(), String> {
    let rows = [row(2, 10, 1), row(1, 10, 1)];
    let source = match sankey::layout(columns(1000), Weight::Exact(3), &rows, 1, &[10, 20]) {
        Outcome::Graph(g) => g,
        other => return Err(format!("{other:?}")),
    };
    let result = source.geometry_v1().map_err(|e| format!("{e:?}"))?;
    assert_eq!(source.folders[0].members, vec![1, 2]);
    assert_eq!(
        result.volume_to_folder.iter().map(|r| r.bytes).sum::<u64>(),
        3
    );
    assert_eq!(
        result
            .folder_to_category
            .iter()
            .map(|r| r.bytes)
            .sum::<u64>(),
        3
    );
    assert_eq!(
        result.folder_to_category[1].to,
        Endpoint::Category(CategoryKey::Unattributed)
    );
    let max = graph(
        &[row(1, 10, i128::from(u64::MAX))],
        u64::MAX,
        COORDINATE_MAX,
    )?;
    assert!(max.geometry_v1().is_ok());
    Ok(())
}

#[test]
fn zero_missing_and_unrepresentable_leave_source_unchanged() -> Result<(), String> {
    let zero = graph(&[row(1, 10, 0)], 0, 1000)?;
    let geometry = zero.geometry_v1().map_err(|e| format!("{e:?}"))?;
    assert!(
        geometry
            .volume_to_folder
            .iter()
            .all(|r| r.geometry.is_none())
    );
    assert!(
        geometry
            .folder_to_category
            .iter()
            .all(|r| r.geometry.is_none())
    );
    let base = [row(1, 10, 1)];
    assert_eq!(
        sankey::layout(columns(1000), Weight::Unknown, &base, 1, &[10]),
        Outcome::Unavailable(Unavailable::UncertainScope)
    );
    assert_eq!(
        sankey::layout(
            columns(1000),
            Weight::Exact(1),
            &[Row {
                bytes: None,
                ..base[0]
            }],
            1,
            &[10]
        ),
        Outcome::Unavailable(Unavailable::UnknownAttribution)
    );
    let tiny = graph(&[row(1, 10, 1), row(1, 20, 99)], 100, 1)?;
    let before = tiny.clone();
    assert_eq!(tiny.geometry_v1().err(), Some(Error::Unrepresentable));
    assert_eq!(tiny, before);
    let mut touching = columns(1000);
    touching.folders.left = touching.volume.right;
    let source = match sankey::layout(touching, Weight::Exact(1), &base, 1, &[10]) {
        Outcome::Graph(g) => g,
        other => return Err(format!("{other:?}")),
    };
    assert_eq!(source.geometry_v1().err(), Some(Error::Unrepresentable));
    assert_eq!(source.links[0].bytes, 1);
    Ok(())
}

#[test]
fn forged_endpoints_and_nonconserved_links_fail_closed() -> Result<(), String> {
    let source = graph(&[row(1, 10, 10)], 10, 1000)?;
    let mut foreign = source.clone();
    foreign.links[0].folder = FolderKey::Folder(999);
    assert_eq!(foreign.geometry_v1().err(), Some(Error::InvalidGraph));
    let mut mismatch = source.clone();
    mismatch.links[0].bytes = 9;
    assert_eq!(mismatch.geometry_v1().err(), Some(Error::InvalidGraph));
    let mut duplicate = source.clone();
    duplicate.links.push(source.links[0]);
    assert_eq!(duplicate.geometry_v1().err(), Some(Error::InvalidGraph));
    Ok(())
}

#[test]
fn category_endpoint_collapse_and_second_gap_fail_whole_conversion() -> Result<(), String> {
    let rows = [row(1, 10, 1), row(1, 20, 99), row(2, 10, 99)];
    let mut source = graph(&rows, 199, 1000)?;
    let category = source
        .categories
        .iter_mut()
        .find(|n| n.key == CategoryKey::Category(10))
        .ok_or("category")?;
    let rect = category.rect.as_mut().ok_or("rectangle")?;
    rect.bottom = rect.top + 1;
    let original = source.clone();
    assert_eq!(source.geometry_v1().err(), Some(Error::Unrepresentable));
    assert_eq!(source, original);
    let mut touching = columns(1000);
    touching.categories.left = touching.folders.right;
    let source = match sankey::layout(touching, Weight::Exact(1), &[row(1, 10, 1)], 1, &[10]) {
        Outcome::Graph(g) => g,
        other => return Err(format!("{other:?}")),
    };
    assert_eq!(source.geometry_v1().err(), Some(Error::Unrepresentable));
    Ok(())
}
