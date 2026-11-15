//! The four totals on the wire, for a treemap served through the product registration.
//!
//! The scope is the F03 value. The drawn total adds the item shapes and the grouped
//! total adds the group, and the two never overlap; whatever neither holds is a sibling
//! the model could not give an area.
use serde_json::Value;

use super::support::{Harness, Publication, render_body};
use super::{TestResult, row_bytes, rows_of, two_files};

fn total(model: &Value, name: &str) -> Result<u64, String> {
    model["totals"][name]["quantity"]["value"]
        .as_str()
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| format!("no {name} total in {}", model["totals"]))
}

/// The drawn and the grouped total never exceed the scope.
pub(super) fn assert_within_scope(model: &Value) -> TestResult {
    let (drawn, grouped) = (total(model, "drawn")?, total(model, "grouped")?);
    assert!(
        drawn + grouped <= total(model, "scope")?,
        "{}",
        model["totals"]
    );
    Ok(())
}

fn count(model: &Value, name: &str) -> Result<u64, String> {
    model["summary"][name]["value"]
        .as_str()
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| format!("no {name} count in {}", model["summary"]))
}

// A large sibling that always keeps a shape, then small ones of the given size.
fn files_of(large: &[u64], small: u64, small_count: usize) -> Vec<(u8, Option<u64>)> {
    let names = (b'a'..=b'z').chain(b'A'..=b'Z');
    let sizes = large
        .iter()
        .copied()
        .chain(std::iter::repeat_n(small, small_count));
    names
        .zip(sizes)
        .map(|(name, bytes)| (name, Some(bytes)))
        .collect()
}

fn render(harness: &Harness, files: &[(u8, Option<u64>)]) -> Result<Value, String> {
    harness.publish(&Publication {
        files,
        ..two_files(3)
    })?;
    let view = harness.open_view()?;
    let answer = harness.ok("viz.render", render_body(&view, 1, "treemap", 3))?;
    assert_eq!(answer["kind"], "model", "{answer}");
    Ok(answer)
}

#[test]
fn the_scope_is_the_drawn_item_shapes_plus_the_group() -> TestResult {
    let harness = Harness::product(4)?;
    // Two large files keep their shapes; forty small ones merge into the group.
    let files = files_of(&[1_200, 1_200], 40, 40);
    let answer = render(&harness, &files)?;
    let model = &answer["model"];
    assert_eq!(total(model, "scope")?, 4_000);
    assert_eq!(total(model, "drawn")?, 2_400);
    assert_eq!(total(model, "grouped")?, 1_600);
    assert_eq!(total(model, "filteredOut")?, 0);
    assert_within_scope(model)?;
    assert_eq!(count(model, "omittedItems")?, 0);
    assert_eq!(count(model, "unknownItems")?, 0);
    // The table has the two item rows and the group row, and no row is counted twice.
    let rows = rows_of(&harness, &answer)?;
    let sizes = row_bytes(&rows)?;
    let kinds: Vec<bool> = rows["rows"]
        .as_array()
        .ok_or("no rows")?
        .iter()
        .map(|row| row["groupRef"].is_null())
        .collect();
    let (items, group): (Vec<_>, Vec<_>) = sizes.iter().zip(&kinds).partition(|(_, item)| **item);
    assert_eq!(items.iter().map(|(bytes, _)| **bytes).sum::<u64>(), 2_400);
    assert_eq!(group.iter().map(|(bytes, _)| **bytes).sum::<u64>(), 1_600);
    assert_eq!(sizes.iter().sum::<u64>(), total(model, "scope")?);
    Ok(())
}

#[test]
fn siblings_below_the_minimum_area_are_the_difference_to_the_scope() -> TestResult {
    let harness = Harness::product(4)?;
    // Thirty small files total 300 bytes, below a quarter of the scope, so even a group
    // of them is too small to draw: they are counted, and their bytes are left over.
    let files = files_of(&[3_000], 10, 30);
    let omitted: u64 = files.iter().skip(1).filter_map(|(_, bytes)| *bytes).sum();
    let answer = render(&harness, &files)?;
    let model = &answer["model"];
    assert_eq!(total(model, "scope")?, 3_300);
    assert_eq!(total(model, "drawn")?, 3_000);
    assert_eq!(total(model, "grouped")?, 0);
    assert_eq!(total(model, "filteredOut")?, 0);
    assert_eq!(
        total(model, "scope")?
            - total(model, "drawn")?
            - total(model, "grouped")?
            - total(model, "filteredOut")?,
        omitted
    );
    assert_within_scope(model)?;
    assert_eq!(count(model, "omittedItems")?, 30);
    assert_eq!(count(model, "groupedItems")?, 0);
    assert_eq!(count(model, "unknownItems")?, 0);
    // The table still lists every sibling with its own value, so it adds up to the scope.
    let rows = rows_of(&harness, &answer)?;
    assert_eq!(row_bytes(&rows)?.len(), 31);
    assert_eq!(
        row_bytes(&rows)?.iter().sum::<u64>(),
        total(model, "scope")?
    );
    Ok(())
}

#[test]
fn with_nothing_omitted_or_unknown_the_drawn_total_is_the_scope() -> TestResult {
    let harness = Harness::product(2000)?;
    let answer = render(&harness, &files_of(&[75, 25], 0, 0))?;
    let model = &answer["model"];
    assert_within_scope(model)?;
    assert_eq!(count(model, "omittedItems")?, 0);
    assert_eq!(count(model, "unknownItems")?, 0);
    assert_eq!(total(model, "scope")?, 100);
    assert_eq!(
        total(model, "drawn")? + total(model, "grouped")? + total(model, "filteredOut")?,
        total(model, "scope")?
    );
    Ok(())
}
