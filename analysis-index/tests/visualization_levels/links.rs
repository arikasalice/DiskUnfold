//! F06 #134: an untraversed link counts its own observed size like a file (F03 FR5.HD1,
//! F06 DLD BL-DLD-1.17). A link whose own size is unknown has no area and leaves the
//! folders above it lower bounds, placed by what was measured. The target was not
//! followed and lowers neither the attribution nor the completeness. On every case the
//! drawn bytes add up to the F03 value of the scope.
use super::lower_bound::{Entry, is_lower_bound, project_at, row_of, scan, value_of};
use super::*;

const BASES: [SizeBasis; 2] = [SizeBasis::Logical, SizeBasis::Allocated];

fn drawn_adds_up_to_the_scope(model: &Model, version: &ResultVersion, basis: &SizeBasis) {
    let scope = value_of(version, "", basis);
    assert_eq!(model.totals.drawn_bytes, bytes_of(&scope), "{basis:?}");
    assert_eq!(bytes_of(&model.totals.scope), bytes_of(&scope), "{basis:?}");
    assert_eq!(
        model.totals.drawn_lower_bound,
        is_lower_bound(&scope),
        "{basis:?}"
    );
}

#[test]
fn a_link_with_its_own_size_is_drawn_like_a_file() {
    let version = scan(
        &[
            Entry::Dir("a"),
            Entry::Dir("x"),
            Entry::File("a/fa", 100),
            Entry::File("x/fx", 20),
            Entry::SizedLink("x/lx", 30),
        ],
        ResultCompleteness::Complete,
        &[],
    );
    for basis in BASES {
        // The link counts in its folder and leaves it exact.
        let x = value_of(&version, "x", &basis);
        assert!(!is_lower_bound(&x), "{basis:?}");
        assert_eq!(bytes_of(&x), 50, "{basis:?}");
        let model = project_at(&version, "", basis.clone(), 16).unwrap();
        assert_eq!(model.shapes.len(), 2, "{basis:?}");
        assert_eq!(model.detail.unknown_items, 0, "{basis:?}");
        assert_eq!(row_of(&model, &version, "x").lod, Lod::Drawn, "{basis:?}");
        drawn_adds_up_to_the_scope(&model, &version, &basis);
        assert_eq!(model.totals.drawn_bytes, 150, "{basis:?}");
        // Inside the folder the link has an area of its own beside the file.
        let inner = project_at(&version, "x", basis.clone(), 16).unwrap();
        assert_eq!(inner.shapes.len(), 2, "{basis:?}");
        assert_eq!(inner.detail.unknown_items, 0, "{basis:?}");
        assert_eq!(row_of(&inner, &version, "lx").lod, Lod::Drawn, "{basis:?}");
        assert_eq!(inner.totals.drawn_bytes, 50, "{basis:?}");
    }
}

#[test]
fn a_link_of_unknown_size_leaves_its_folder_a_lower_bound_placed_by_what_was_measured() {
    let version = scan(
        &[
            Entry::Dir("a"),
            Entry::Dir("x"),
            Entry::File("a/fa", 100),
            Entry::File("x/fx", 20),
            Entry::Link("x/lx"),
        ],
        ResultCompleteness::Complete,
        &[],
    );
    for basis in BASES {
        let x = value_of(&version, "x", &basis);
        assert!(is_lower_bound(&x), "{basis:?}");
        assert_eq!(bytes_of(&x), 20, "{basis:?}");
        let model = project_at(&version, "", basis.clone(), 16).unwrap();
        // The folder is still drawn, by its lower bound.
        assert_eq!(model.shapes.len(), 2, "{basis:?}");
        assert_eq!(row_of(&model, &version, "x").lod, Lod::Drawn, "{basis:?}");
        drawn_adds_up_to_the_scope(&model, &version, &basis);
        // Inside it the link is counted without an area.
        let inner = project_at(&version, "x", basis.clone(), 16).unwrap();
        assert_eq!(inner.shapes.len(), 1, "{basis:?}");
        assert_eq!(inner.detail.unknown_items, 1, "{basis:?}");
        assert_eq!(inner.totals.drawn_bytes, 20, "{basis:?}");
    }
}

#[test]
fn folders_of_only_links_are_drawn_by_their_own_sizes() {
    let version = scan(
        &[
            Entry::Dir("a"),
            Entry::Dir("known"),
            Entry::Dir("mixed"),
            Entry::File("a/fa", 100),
            Entry::SizedLink("known/one", 10),
            Entry::SizedLink("known/two", 20),
            Entry::SizedLink("mixed/sized", 40),
            Entry::Link("mixed/unsized"),
        ],
        ResultCompleteness::Complete,
        &[],
    );
    for basis in BASES {
        let known = value_of(&version, "known", &basis);
        assert!(!is_lower_bound(&known), "{basis:?}");
        assert_eq!(bytes_of(&known), 30, "{basis:?}");
        let mixed = value_of(&version, "mixed", &basis);
        assert!(is_lower_bound(&mixed), "{basis:?}");
        assert_eq!(bytes_of(&mixed), 40, "{basis:?}");
        let model = project_at(&version, "", basis.clone(), 16).unwrap();
        assert_eq!(model.shapes.len(), 3, "{basis:?}");
        assert_eq!(model.detail.unknown_items, 0, "{basis:?}");
        for name in ["known", "mixed"] {
            assert_eq!(row_of(&model, &version, name).lod, Lod::Drawn, "{basis:?}");
        }
        drawn_adds_up_to_the_scope(&model, &version, &basis);
        assert_eq!(model.totals.drawn_bytes, 170, "{basis:?}");
    }
}
