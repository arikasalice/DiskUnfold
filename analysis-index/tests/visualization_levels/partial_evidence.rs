//! A folder whose content is only partly measured on a physical basis (#364, F06
//! README 51.1): it is drawn by the bytes F03 counts under it, as a lower bound, rather
//! than falling out of the chart.
use super::lower_bound::{Entry, is_lower_bound, project_at, row_of, scan, value_of};
use super::*;
use du_analysis_index::visualization::projection::UnknownShare;

fn folders(odd: Entry<'static>) -> Arc<ResultVersion> {
    scan(
        &[
            Entry::Dir("a"),
            Entry::Dir("b"),
            Entry::File("a/fa", 100),
            odd,
            Entry::File("b/fb", 50),
        ],
        ResultCompleteness::Complete,
        &[],
    )
}

#[test]
fn a_file_without_allocation_leaves_its_folder_drawn_as_a_lower_bound() {
    let version = folders(Entry::NoAllocation("a/fx", 30));
    let a = value_of(&version, "a", &SizeBasis::Allocated);
    // F03 adds nothing for the file and makes the folder a lower bound of the rest.
    assert!(is_lower_bound(&a));
    assert_eq!(bytes_of(&a), 100);
    let model = project_at(&version, "", SizeBasis::Allocated, 16).unwrap();
    assert_eq!(model.detail.unknown_items, 0);
    assert_eq!(model.detail.unknown_share, UnknownShare::None);
    assert_eq!(model.shapes.len(), 2);
    let row = row_of(&model, &version, "a");
    assert_eq!(row.omission, None);
    assert!(is_lower_bound(&row.size));
    assert_eq!(model.totals.drawn_bytes, 150);
}

#[test]
fn a_lower_bound_allocation_is_counted_and_leaves_a_lower_bound() {
    let version = folders(Entry::LowerAllocation("a/fx", 30));
    let model = project_at(&version, "", SizeBasis::Allocated, 16).unwrap();
    assert_eq!(model.detail.unknown_items, 0);
    let row = row_of(&model, &version, "a");
    assert!(is_lower_bound(&row.size));
    assert_eq!(bytes_of(&row.size), 130);
    assert_eq!(model.totals.drawn_bytes, 180);
}

#[test]
fn unique_physical_size_still_refuses_a_partly_measured_folder() {
    let version = folders(Entry::NoAllocation("a/fx", 30));
    let refused = project_at(&version, "", SizeBasis::UniquePhysical, 16);
    assert!(refused.map_or(true, |model| {
        model
            .rows
            .iter()
            .any(|row| row.omission == Some(NoArea::Unknown))
    }));
}
