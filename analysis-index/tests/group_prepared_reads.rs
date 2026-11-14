mod group_support;

use std::cmp::Ordering;

use du_analysis_index::group::{self, Category, DEFAULT_MEMBER_SORT, Error};
use group_support::{
    FileSpec, Recording, build, classify, dataset, dataset_rules, ends_with, err, file_key,
    item_key, parallel, registry, sort,
};

/// Orders by logical size, largest first, missing last, then by observed path.
fn largest_first(a: &&FileSpec, b: &&FileSpec) -> Ordering {
    let sizes = match (a.logical, b.logical) {
        (Some(x), Some(y)) => y.cmp(&x),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    };
    sizes.then_with(|| (a.dir, &a.name).cmp(&(b.dir, &b.name)))
}

#[test]
fn the_default_member_sort_selects_rows_from_the_order_built_with_the_classification()
-> Result<(), String> {
    let files = dataset(1);
    let version = build(&files, 1)?;
    let read = classify(&version, None, dataset_rules(), parallel(6, 2)?)?;
    let registry = registry()?;
    let summary = read.summary();
    let sorts = [DEFAULT_MEMBER_SORT];

    // The group of files that do not end in .bin, in the default order and in windows.
    let mut members: Vec<&FileSpec> = files.iter().filter(|f| !f.name.ends_with(".bin")).collect();
    members.sort_by(largest_first);
    let expected: Vec<_> = members.iter().map(|f| file_key(f)).collect();
    let group = Category::Group(1);
    let mut joined = Vec::new();
    for start in (0..members.len() + 5).step_by(5) {
        let page = read
            .page(group, &sorts, &registry, start, 5, || false)
            .map_err(err)?;
        assert_eq!(page.total, members.len(), "start {start}");
        assert!(page.rows.len() <= 5);
        joined.extend(
            page.rows
                .iter()
                .map(|row| item_key(&version, row.item.item_no)),
        );
    }
    assert_eq!(joined, expected);

    // Every category reports the count of the summary and the rows of its files.
    for (category, count) in [
        (Category::Group(0), summary.groups[0].members),
        (Category::Overlap, summary.overlap),
        (Category::Unclassified, summary.unclassified),
        (Category::Undecidable, summary.undecidable),
    ] {
        let page = read
            .page(category, &sorts, &registry, 0, usize::MAX, || false)
            .map_err(err)?;
        assert_eq!(page.total, count as usize, "{category:?}");
        assert_eq!(page.rows.len(), count as usize, "{category:?}");
        let again = read
            .page(category, &sorts, &registry, 0, usize::MAX, || false)
            .map_err(err)?;
        assert_eq!(page, again);
    }
    let beyond = read
        .page(group, &sorts, &registry, members.len() + 1, 5, || false)
        .map_err(err)?;
    assert_eq!((beyond.total, beyond.rows.len()), (members.len(), 0));
    let none = read
        .page(group, &sorts, &registry, 0, 0, || false)
        .map_err(err)?;
    assert_eq!((none.total, none.rows.len()), (members.len(), 0));
    assert_eq!(
        read.page(Category::Group(9), &sorts, &registry, 0, 1, || false)
            .map(|_| ()),
        Err(Error::InvalidSelection)
    );
    Ok(())
}

#[test]
fn only_the_default_sort_avoids_ordering_at_the_first_read() -> Result<(), String> {
    let version = build(&dataset(1), 1)?;
    let read = classify(&version, None, dataset_rules(), parallel(6, 2)?)?;
    let registry = registry()?;
    let group = Category::Group(1);
    // A reader that is cancelled at its first check still gets the prepared order.
    let prepared = read.page(group, &[DEFAULT_MEMBER_SORT], &registry, 0, 10, || true);
    assert!(prepared.is_ok_and(|page| !page.rows.is_empty()));
    // Another direction, basis, strategy or key list is ordered on request.
    for sorts in [
        vec![sort("sort.size", false)],
        vec![sort("sort.name", true)],
        vec![],
        vec![DEFAULT_MEMBER_SORT, sort("sort.name", false)],
    ] {
        let outcome = read.page(group, &sorts, &registry, 0, 10, || true);
        assert_eq!(outcome.map(|_| ()), Err(Error::Cancelled), "{sorts:?}");
    }
    Ok(())
}

#[test]
fn the_default_selection_totals_are_ready_and_other_selections_are_cached_in_turn()
-> Result<(), String> {
    let version = build(&dataset(1), 1)?;
    let rules = vec![ends_with(".txt"), ends_with(".log"), ends_with(".bin")];
    let read = classify(&version, None, rules, parallel(6, 2)?)?;
    let ready = |selected: &[usize]| read.totals(selected, || true).is_ok();
    // The default selection is the same as naming every group, and needs no work.
    assert!(ready(&[]) && ready(&[0, 1, 2]) && ready(&[2, 2, 1, 0]));
    let others: [&[usize]; 5] = [&[0], &[1], &[2], &[0, 1], &[0, 2]];
    assert!(!ready(&[0]) && !ready(&[1, 2]));
    for selected in others {
        read.totals(selected, || false).map_err(err)?;
    }
    // Four other selections are kept; the fifth pushed out the oldest, and not the default.
    assert!(!ready(&[0]), "the oldest selection was dropped");
    for selected in &others[1..] {
        assert!(ready(selected), "{selected:?} was dropped");
    }
    assert!(ready(&[]));
    // Cached totals are the calculated ones.
    let again = read.totals(&[0], || false).map_err(err)?;
    assert_eq!(again, read.totals(&[0], || true).map_err(err)?);
    let default = read.totals(&[], || true).map_err(err)?;
    assert_eq!(default.groups, again.groups);
    Ok(())
}

#[test]
fn the_decision_bound_is_checked_before_anything_is_evaluated() -> Result<(), String> {
    let version = build(&dataset(1), 1)?;
    let rules = || vec![ends_with(".txt"), ends_with(".log")];
    let files = dataset(1).len();
    let run = |limit: usize| {
        let control = Recording::default();
        let outcome = group::classify_scope_bounded(
            version.clone(),
            None,
            &registry()?,
            rules(),
            parallel(6, 2)?,
            &control,
            limit,
        );
        Ok::<_, String>((outcome, control.polls()))
    };
    let (outcome, polls) = run(files * 2 - 1)?;
    assert_eq!(outcome.map(|_| ()), Err(Error::ScopeTooLarge));
    assert!(polls <= 2, "the scope was evaluated: {polls} checks");
    assert!(run(files * 2)?.0.is_ok());
    Ok(())
}
