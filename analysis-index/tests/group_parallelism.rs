mod group_support;

use std::sync::Arc;

use du_analysis_index::{
    ResultVersion,
    group::{self, Category, Classification, Decision, Error, Parallelism, Summary, Totals},
    query::Condition,
};
use du_core_contracts::ItemRef;
use group_support::{
    FileSpec, Recording, absent, build, classify, dataset, ends_with, err, parallel, registered,
    registry, starts_with,
};

fn rules() -> Vec<Condition> {
    vec![
        ends_with(".txt"),
        starts_with("alpha"),
        absent(),
        Condition::Not(Box::new(ends_with(".bin"))),
        registered("pred.unknown"),
    ]
}

/// Everything a reader can see of a classification, for comparison between runs.
#[derive(Debug, PartialEq, Eq)]
struct Observed {
    rows: Vec<(ItemRef, Vec<Decision>)>,
    summary: Summary,
    totals: Totals,
    subset: Totals,
    pages: Vec<(group::Page, group::Page)>,
}

fn observe(read: &Classification) -> Result<Observed, String> {
    let registry = registry()?;
    let mut categories = vec![
        Category::Overlap,
        Category::Unclassified,
        Category::Undecidable,
    ];
    categories.extend((0..read.rules().len()).map(Category::Group));
    let mut pages = Vec::new();
    for category in categories {
        let all = read
            .page(category, &[], &registry, 0, usize::MAX, || false)
            .map_err(err)?;
        let window = read
            .page(category, &[], &registry, 1, 3, || false)
            .map_err(err)?;
        pages.push((all, window));
    }
    Ok(Observed {
        rows: read
            .rows()
            .map(|(item, row)| (item, row.to_vec()))
            .collect(),
        summary: read.summary().clone(),
        totals: read.totals(&[], || false).map_err(err)?,
        subset: read.totals(&[1, 3], || false).map_err(err)?,
        pages,
    })
}

fn version(roots: u32) -> Result<(Vec<FileSpec>, Arc<ResultVersion>), String> {
    let files = dataset(roots);
    let version = build(&files, roots)?;
    Ok((files, version))
}

#[test]
fn chunk_size_and_worker_count_do_not_change_the_classification() -> Result<(), String> {
    for roots in [1, 3] {
        let (files, version) = version(roots)?;
        let baseline = observe(&classify(&version, None, rules(), parallel(1, 1)?)?)?;
        assert_eq!(baseline.summary.scope_files as usize, files.len());
        assert!(baseline.summary.overlap > 0 && baseline.summary.undecidable > 0);
        let default = Parallelism::default();
        let configurations = [
            (1, 3),
            (default.chunk_items.get(), default.workers.get()),
            (default.chunk_items.get(), 3),
            (files.len(), 1),
            (files.len(), 3),
            (7, 3),
            (3, 8),
            (usize::MAX, 3),
        ];
        for (chunk, workers) in configurations {
            let read = classify(&version, None, rules(), parallel(chunk, workers)?)?;
            assert_eq!(
                observe(&read)?,
                baseline,
                "roots {roots}, chunk {chunk}, workers {workers}"
            );
        }
    }
    Ok(())
}

#[test]
fn the_default_division_uses_large_chunks_and_a_few_workers() {
    let default = Parallelism::default();
    assert_eq!(default.chunk_items.get(), 4096);
    assert!((1..=4).contains(&default.workers.get()));
}

#[test]
fn progress_grows_in_order_and_ends_at_the_scope_file_count() -> Result<(), String> {
    let (files, version) = version(1)?;
    let total = group::scope_file_count(&version, None).map_err(err)?;
    assert_eq!(total, files.len() as u64);
    for (chunk, workers) in [(1, 1), (5, 3), (1, 3), (1000, 3)] {
        let control = Recording::default();
        group::classify_scope(
            Arc::clone(&version),
            None,
            &registry()?,
            rules(),
            parallel(chunk, workers)?,
            &control,
        )
        .map_err(err)?;
        let reports = control.reports();
        assert!(
            reports.windows(2).all(|pair| pair[0] < pair[1]),
            "{reports:?}"
        );
        assert_eq!(reports.last(), Some(&total));
        assert_eq!(reports.len(), files.len().div_ceil(chunk));
    }
    Ok(())
}

#[test]
fn cancelling_at_any_check_returns_no_classification() -> Result<(), String> {
    let (_, version) = version(1)?;
    for (chunk, workers) in [(1, 1), (9, 1), (9, 3), (1000, 3)] {
        let counting = Recording::default();
        group::classify_scope(
            Arc::clone(&version),
            None,
            &registry()?,
            rules(),
            parallel(chunk, workers)?,
            &counting,
        )
        .map_err(err)?;
        let polls = counting.polls();
        assert!(polls >= 3, "a start, a chunk and an end check");
        for from in 1..=polls {
            let outcome = group::classify_scope(
                Arc::clone(&version),
                None,
                &registry()?,
                rules(),
                parallel(chunk, workers)?,
                &Recording::cancel_from(from),
            );
            assert!(
                matches!(outcome, Err(Error::Cancelled)),
                "chunk {chunk}, workers {workers}, cancelled from poll {from}: {outcome:?}"
            );
        }
        let outcome = group::classify_scope(
            Arc::clone(&version),
            None,
            &registry()?,
            rules(),
            parallel(chunk, workers)?,
            &Recording::cancel_from(polls + 1),
        );
        assert!(outcome.is_ok());
    }
    Ok(())
}

#[test]
fn a_cancelled_read_of_totals_pages_and_reconciliation_returns_nothing() -> Result<(), String> {
    let (_, version) = version(1)?;
    let read = classify(&version, None, rules(), parallel(4, 2)?)?;
    assert_eq!(read.totals(&[], || true).map(|_| ()), Err(Error::Cancelled));
    assert_eq!(read.reconcile(|| true).map(|_| ()), Err(Error::Cancelled));
    let page = read.page(Category::Overlap, &[], &registry()?, 0, 10, || true);
    assert_eq!(page.map(|_| ()), Err(Error::Cancelled));
    let mut polls = 0;
    let midway = read.totals(&[], || {
        polls += 1;
        polls == 20
    });
    assert_eq!(midway.map(|_| ()), Err(Error::Cancelled));
    Ok(())
}
