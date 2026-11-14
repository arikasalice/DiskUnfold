mod group_support;

use std::{
    cmp::Ordering,
    sync::atomic::{AtomicUsize, Ordering as AtomicOrdering},
};

use du_analysis_index::{
    group::{self, Category, Classification, Error},
    query::{Basis, Record, Sort, SortValue},
};
use group_support::{
    FileSpec, alias_fixture, build, classify, dataset, dataset_rules, ends_with, err, file_key,
    item_key, parallel, registry, sort, starts_with,
};
fn keys(
    read: &Classification,
    category: Category,
    sorts: &[Sort<'_>],
) -> Result<Vec<String>, String> {
    let page = read
        .page(category, sorts, &registry()?, 0, usize::MAX, || false)
        .map_err(err)?;
    Ok(page
        .rows
        .iter()
        .map(|row| item_key(read.version(), row.item.item_no))
        .collect())
}

#[test]
fn a_file_in_two_groups_is_one_row_with_both_groups() -> Result<(), String> {
    let (version, scope) = alias_fixture()?;
    let rules = vec![ends_with("alias"), starts_with("a-")];
    let read = classify(&version, Some(&scope), rules, parallel(1, 1)?)?;
    let registry = registry()?;
    let page = |category| {
        read.page(category, &[], &registry, 0, 10, || false)
            .map_err(err)
    };
    let overlap = page(Category::Overlap)?;
    assert_eq!(overlap.total, 1);
    assert_eq!(
        item_key(&version, overlap.rows[0].item.item_no),
        "r0:scope/a-alias"
    );
    assert_eq!(overlap.rows[0].groups, [0, 1]);
    let first = page(Category::Group(0))?;
    assert_eq!(first.total, 2);
    assert_eq!(first.rows[0].groups, [0, 1]);
    assert_eq!(first.rows[1].groups, [0]);
    assert_eq!(
        item_key(&version, first.rows[1].item.item_no),
        "r0:scope/b-alias"
    );
    let second = page(Category::Group(1))?;
    assert_eq!((second.total, second.rows.len()), (1, 1));
    let rest = page(Category::Unclassified)?;
    assert_eq!(rest.total, 1);
    assert!(rest.rows[0].groups.is_empty() && rest.rows[0].undecidable_reasons.is_empty());
    assert_eq!(page(Category::Undecidable)?.total, 0);
    Ok(())
}

#[test]
fn category_totals_equal_the_summary_and_rows_carry_their_reasons() -> Result<(), String> {
    let files = dataset(1);
    let version = build(&files, 1)?;
    let read = classify(&version, None, dataset_rules(), parallel(6, 2)?)?;
    let registry = registry()?;
    let summary = read.summary();
    let total = |category| {
        read.page(category, &[], &registry, 0, 1, || false)
            .map(|page| page.total)
            .map_err(err)
    };
    assert_eq!(total(Category::Overlap)?, summary.overlap as usize);
    assert_eq!(
        total(Category::Unclassified)?,
        summary.unclassified as usize
    );
    assert_eq!(total(Category::Undecidable)?, summary.undecidable as usize);
    for (index, counts) in summary.groups.iter().enumerate() {
        assert_eq!(total(Category::Group(index))?, counts.members as usize);
    }
    let all = read
        .page(Category::Group(1), &[], &registry, 0, usize::MAX, || false)
        .map_err(err)?;
    assert!(!all.rows.is_empty());
    for row in &all.rows {
        assert_eq!(
            row.undecidable_reasons,
            [group::REASON_RULE_UNEVALUABLE, group::REASON_VALUE_UNKNOWN]
        );
    }
    Ok(())
}

/// Orders by logical size, missing last, then by observed path.
fn by_size(descending: bool) -> impl Fn(&&FileSpec, &&FileSpec) -> Ordering {
    move |a, b| {
        let sizes = match (a.logical, b.logical) {
            (Some(x), Some(y)) if descending => y.cmp(&x),
            (Some(x), Some(y)) => x.cmp(&y),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        };
        sizes.then_with(|| (a.dir, &a.name).cmp(&(b.dir, &b.name)))
    }
}

#[test]
fn pages_follow_the_sort_then_the_path_and_repeat_exactly() -> Result<(), String> {
    let files = dataset(1);
    let version = build(&files, 1)?;
    let read = classify(&version, None, dataset_rules(), parallel(6, 2)?)?;
    let registry = registry()?;
    let members: Vec<&FileSpec> = files.iter().filter(|f| !f.name.ends_with(".bin")).collect();
    let group = Category::Group(1);

    let mut by_path = members.clone();
    by_path.sort_by(|a, b| (a.dir, &a.name).cmp(&(b.dir, &b.name)));
    let expected: Vec<_> = by_path.iter().map(|f| file_key(f)).collect();
    assert_eq!(keys(&read, group, &[])?, expected);

    let mut largest_first = members.clone();
    largest_first.sort_by(by_size(true));
    let size_desc = [sort("sort.size", true)];
    let expected: Vec<_> = largest_first.iter().map(|f| file_key(f)).collect();
    assert_eq!(keys(&read, group, &size_desc)?, expected);
    assert!(
        members.iter().any(|f| f.logical.is_none()),
        "an unreadable size must sort last"
    );
    let mut smallest_first = members.clone();
    smallest_first.sort_by(by_size(false));
    let expected: Vec<_> = smallest_first.iter().map(|f| file_key(f)).collect();
    assert_eq!(keys(&read, group, &[sort("sort.size", false)])?, expected);

    let mut by_name = members.clone();
    by_name.sort_by(|a, b| (&a.name, a.dir).cmp(&(&b.name, b.dir)));
    let expected: Vec<_> = by_name.iter().map(|f| file_key(f)).collect();
    assert_eq!(keys(&read, group, &[sort("sort.name", false)])?, expected);

    // Position ranges of one order add up to the whole order, and repeating a request
    // (after other orders were requested) returns the same rows.
    let full = read
        .page(group, &size_desc, &registry, 0, usize::MAX, || false)
        .map_err(err)?;
    assert_eq!(full.total, members.len());
    let mut joined = Vec::new();
    for start in (0..full.total).step_by(7) {
        let window = read
            .page(group, &size_desc, &registry, start, 7, || false)
            .map_err(err)?;
        assert_eq!(window.total, full.total);
        assert_eq!(window.rows, full.rows[start..(start + 7).min(full.total)]);
        joined.extend(window.rows);
    }
    assert_eq!(joined, full.rows);
    let again = read
        .page(group, &size_desc, &registry, 0, usize::MAX, || false)
        .map_err(err)?;
    assert_eq!(again, full);
    let past = read
        .page(group, &size_desc, &registry, full.total + 5, 3, || false)
        .map_err(err)?;
    assert_eq!((past.total, past.rows.len()), (full.total, 0));
    let none = read
        .page(group, &size_desc, &registry, 2, 0, || false)
        .map_err(err)?;
    assert_eq!((none.total, none.rows.len()), (full.total, 0));
    Ok(())
}

#[test]
fn invalid_requests_fail_and_a_cancelled_page_leaves_no_partial_order() -> Result<(), String> {
    let files = dataset(1);
    let version = build(&files, 1)?;
    let read = classify(&version, None, dataset_rules(), parallel(6, 2)?)?;
    let registry = registry()?;
    let page = |category, sorts: &[Sort<'_>], cancelled: bool| {
        read.page(category, sorts, &registry, 0, 5, || cancelled)
    };
    assert_eq!(
        page(Category::Group(9), &[], false).map(|_| ()),
        Err(Error::InvalidSelection)
    );
    let unknown = [sort("sort.absent", false)];
    assert!(matches!(
        page(Category::Overlap, &unknown, false),
        Err(Error::InvalidSort(_))
    ));
    let by_size = [sort("sort.size", true)];
    assert_eq!(
        page(Category::Group(1), &by_size, true).map(|_| ()),
        Err(Error::Cancelled)
    );
    let first = page(Category::Group(1), &by_size, false).map_err(err)?;
    assert_eq!(first.rows.len(), 5);
    let mut polls = 0;
    let midway = read.page(
        Category::Group(1),
        &[sort("sort.name", true)],
        &registry,
        0,
        5,
        || {
            polls += 1;
            polls == 3
        },
    );
    assert_eq!(midway.map(|_| ()), Err(Error::Cancelled));
    Ok(())
}

static KEY_CALLS: AtomicUsize = AtomicUsize::new(0);

fn counted_name_key(record: &Record<'_>, _: Basis) -> SortValue {
    KEY_CALLS.fetch_add(1, AtomicOrdering::SeqCst);
    SortValue::Bytes(record.name().to_vec())
}

#[test]
fn an_order_is_computed_on_its_first_request_and_then_kept() -> Result<(), String> {
    let files = dataset(1);
    let version = build(&files, 1)?;
    let read = classify(&version, None, dataset_rules(), parallel(6, 2)?)?;
    let mut registry = registry()?;
    registry
        .register_sort("sort.counted", 1, counted_name_key)
        .map_err(err)?;
    let ascending = [sort("sort.counted", false)];
    let not_binary = files.iter().filter(|f| !f.name.ends_with(".bin")).count();
    let text = files.iter().filter(|f| f.name.ends_with(".txt")).count();
    let page = |category, sorts: &[Sort<'_>], start| {
        read.page(category, sorts, &registry, start, 5, || false)
            .map_err(err)
    };
    let first = page(Category::Group(1), &ascending, 0)?;
    assert_eq!(KEY_CALLS.load(AtomicOrdering::SeqCst), not_binary);
    let second = page(Category::Group(1), &ascending, 5)?;
    assert_eq!(page(Category::Group(1), &ascending, 0)?, first);
    assert_eq!(
        KEY_CALLS.load(AtomicOrdering::SeqCst),
        not_binary,
        "kept order is reused"
    );
    assert_ne!(first.rows, second.rows);
    page(Category::Group(0), &ascending, 0)?;
    assert_eq!(KEY_CALLS.load(AtomicOrdering::SeqCst), not_binary + text);
    page(Category::Group(0), &[sort("sort.counted", true)], 0)?;
    assert_eq!(
        KEY_CALLS.load(AtomicOrdering::SeqCst),
        not_binary + 2 * text
    );
    Ok(())
}

#[test]
fn readers_on_several_threads_see_the_same_pages() -> Result<(), String> {
    fn shareable<T: Send + Sync>() {}
    shareable::<Classification>();
    shareable::<group::ClassificationStore>();
    let files = dataset(1);
    let version = build(&files, 1)?;
    let read = classify(&version, None, dataset_rules(), parallel(6, 2)?)?;
    let registry = registry()?;
    let sorts = [sort("sort.size", true)];
    let pages: Vec<_> = std::thread::scope(|scope| {
        let readers: Vec<_> = (0..4)
            .map(|_| {
                scope.spawn(|| read.page(Category::Group(1), &sorts, &registry, 0, 20, || false))
            })
            .collect();
        readers.into_iter().map(|reader| reader.join()).collect()
    });
    let first = pages.first().ok_or("no readers")?;
    for page in &pages {
        let (page, first) = (
            page.as_ref().map_err(|_| "reader panicked")?,
            first.as_ref().map_err(|_| "reader panicked")?,
        );
        assert_eq!(page, first);
        assert!(page.is_ok());
    }
    Ok(())
}
