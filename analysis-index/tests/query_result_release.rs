//! Query results of a result version that a publication replaced for good are released.
use du_analysis_index::query::page::{MultiFolderKey, PageError, ResultSets};
use du_analysis_index::query::{
    self, Basis, Condition, FolderLocation, QueryResult, Registry, Sort,
};
use du_analysis_index::{BuildHandle, IngestSink, Observation, ResultVersion};
use du_core_contracts::result::{
    ResultCompleteness, ResultNativePath, ResultScopeRoot, ResultVersionRead,
};
use du_core_contracts::{Id, PathBytes, PathEncoding};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};

fn id(n: u64) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}
fn fail(error: impl std::fmt::Debug) -> String {
    format!("{error:?}")
}
fn file(build: &mut BuildHandle, name: u64) -> Result<(), String> {
    build
        .add_file(Observation {
            path: vec![format!("{name:05}").into_bytes()],
            size: None,
        })
        .map_err(fail)
}
fn new_build(sink: &IngestSink, scope: u64) -> Result<BuildHandle, String> {
    let mut build = sink.open(id(scope)?).map_err(fail)?;
    build.add_directory(vec![]).map_err(fail)?;
    Ok(build)
}
fn sort() -> Sort<'static> {
    Sort {
        id: "sort.path",
        revision: 1,
        basis: Basis::Logical,
        descending: false,
    }
}
/// Registers a folder query of the scope root and returns its reference.
fn register_folder(
    sets: &ResultSets,
    version: &Arc<ResultVersion>,
    query: u64,
) -> Result<Id, String> {
    let result = query::run_folder(
        &Registry::baseline(),
        Arc::clone(version),
        None,
        &Condition::All(vec![]),
        sort(),
        || false,
    )
    .map_err(fail)?;
    let key = MultiFolderKey {
        version: version.version_id().clone(),
        folder: FolderLocation::Item(0),
        tree: false,
        condition: result.condition.clone(),
        sorts: result.sorts.clone(),
    };
    sets.register_multi_folder(key, id(query)?, result)
        .map_err(fail)
}
fn full_result(version: &Arc<ResultVersion>) -> Result<QueryResult, String> {
    query::run(
        &Registry::baseline(),
        Arc::clone(version),
        &Condition::All(vec![]),
        sort(),
        || false,
    )
    .map_err(fail)
}
fn is_stale(sets: &ResultSets, query: u64, version: &Id) -> Result<bool, String> {
    Ok(matches!(
        sets.open_page(&id(query)?, version, 0, None),
        Err(PageError::StaleVersion)
    ))
}
fn alive(version: &Weak<ResultVersion>) -> bool {
    version.upgrade().is_some()
}

#[test]
fn partial_versions_do_not_accumulate_in_one_build() -> Result<(), String> {
    let sink = IngestSink::default();
    let sets = ResultSets::new(sink.clone());
    let mut build = new_build(&sink, 1)?;
    let mut weak = Vec::new();
    for n in 0..20_u64 {
        file(&mut build, n)?;
        let version = build.publish_partial(id(100 + n)?).map_err(fail)?;
        register_folder(&sets, &version, 1_000 + n)?;
        weak.push(Arc::downgrade(&version));
    }
    let live: Vec<bool> = weak.iter().map(alive).collect();
    assert_eq!(live.iter().filter(|alive| **alive).count(), 1);
    assert!(live[19]);
    assert!(is_stale(&sets, 1_000, &id(100)?)?);
    let page = sets
        .open_page(&id(1_019)?, &id(119)?, 0, None)
        .map_err(fail)?;
    assert_eq!(page.items().len(), 20);
    Ok(())
}

#[test]
fn abort_keeps_baseline_results_and_releases_the_withdrawn_partial() -> Result<(), String> {
    let sink = IngestSink::default();
    let sets = ResultSets::new(sink.clone());
    let baseline = new_build(&sink, 1)?
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(fail)?;
    register_folder(&sets, &baseline, 10)?;
    let weak_baseline = Arc::downgrade(&baseline);
    drop(baseline);

    let mut build = new_build(&sink, 3)?;
    let partial = build.publish_partial(id(4)?).map_err(fail)?;
    register_folder(&sets, &partial, 11)?;
    let weak_partial = Arc::downgrade(&partial);
    drop(partial);
    assert!(alive(&weak_baseline), "baseline is restored by an abort");
    assert!(is_stale(&sets, 10, &id(2)?)?, "baseline is not current yet");

    build.abort();
    assert!(!alive(&weak_partial));
    assert!(is_stale(&sets, 11, &id(4)?)?);
    let page = sets.open_page(&id(10)?, &id(2)?, 0, None).map_err(fail)?;
    assert_eq!(page.items().len(), 0);
    Ok(())
}

#[test]
fn seal_releases_baseline_and_previous_partial() -> Result<(), String> {
    let sink = IngestSink::default();
    let sets = ResultSets::new(sink.clone());
    let baseline = new_build(&sink, 1)?
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(fail)?;
    register_folder(&sets, &baseline, 10)?;
    let weak_baseline = Arc::downgrade(&baseline);
    drop(baseline);

    let mut build = new_build(&sink, 3)?;
    let first = build.publish_partial(id(4)?).map_err(fail)?;
    register_folder(&sets, &first, 11)?;
    let weak_first = Arc::downgrade(&first);
    drop(first);
    file(&mut build, 1)?;
    let second = build.publish_partial(id(5)?).map_err(fail)?;
    register_folder(&sets, &second, 12)?;
    let weak_second = Arc::downgrade(&second);
    drop(second);
    assert!(
        alive(&weak_baseline),
        "first publication keeps the baseline"
    );
    assert!(!alive(&weak_first));

    let sealed = build
        .seal(id(6)?, ResultCompleteness::Complete)
        .map_err(fail)?;
    assert!(!alive(&weak_baseline));
    assert!(!alive(&weak_second));
    assert!(is_stale(&sets, 10, &id(2)?)?);
    assert!(is_stale(&sets, 12, &id(5)?)?);
    register_folder(&sets, &sealed, 13)?;
    sets.open_page(&id(13)?, &id(6)?, 0, None).map_err(fail)?;
    Ok(())
}

#[test]
fn admitted_reads_finish_after_the_version_is_replaced() -> Result<(), String> {
    let sink = IngestSink::default();
    let sets = ResultSets::new(sink.clone());
    let mut build = new_build(&sink, 1)?;
    file(&mut build, 1)?;
    let first = build.publish_partial(id(2)?).map_err(fail)?;
    register_folder(&sets, &first, 10)?;
    let weak_first = Arc::downgrade(&first);
    drop(first);
    let held = sets.open_parent(&id(10)?, &id(2)?).map_err(fail)?;
    let page = sets.open_page(&id(10)?, &id(2)?, 0, None).map_err(fail)?;

    file(&mut build, 2)?;
    build.publish_partial(id(3)?).map_err(fail)?;
    assert!(is_stale(&sets, 10, &id(2)?)?);
    assert_eq!(held.items.len(), 1);
    assert_eq!(page.items().len(), 1);
    assert!(
        alive(&weak_first),
        "the admitted result still owns the version"
    );
    drop((held, page));
    assert!(!alive(&weak_first));
    Ok(())
}

#[test]
fn sealing_a_replacement_releases_the_base_results() -> Result<(), String> {
    let sink = IngestSink::default();
    let lineage = sink
        .bind_current_analysis("release-replacement", id(40)?)
        .map_err(fail)?;
    let root = ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::WindowsUtf16le,
            raw: PathBytes::from_bytes(&[67, 0]),
        },
        volume_id: vec![1],
        file_id: vec![1],
    };
    let mut build = sink
        .open_forest_in_lineage(id(41)?, vec![root], lineage)
        .map_err(fail)?;
    build.add_directory(vec![]).map_err(fail)?;
    let base = build
        .seal(id(42)?, ResultCompleteness::Complete)
        .map_err(fail)?;
    let sets = ResultSets::new(sink.clone());
    sets.register(id(10)?, full_result(&base)?).map_err(fail)?;
    let mut candidate = sink.open_replacement(&base).map_err(fail)?;
    candidate.add_directory(vec![]).map_err(fail)?;
    let weak_base = Arc::downgrade(&base);
    drop(base);
    assert!(alive(&weak_base), "an unsealed candidate changes nothing");

    candidate
        .seal(id(43)?, ResultCompleteness::Complete)
        .map_err(fail)?;
    assert!(!alive(&weak_base));
    assert!(is_stale(&sets, 10, &id(42)?)?);
    Ok(())
}

#[test]
fn publication_after_dropping_a_registry_still_releases_for_the_others() -> Result<(), String> {
    let sink = IngestSink::default();
    let dropped = ResultSets::new(sink.clone());
    let kept = ResultSets::new(sink.clone());
    let mut build = new_build(&sink, 1)?;
    let first = build.publish_partial(id(2)?).map_err(fail)?;
    register_folder(&dropped, &first, 10)?;
    register_folder(&kept, &first, 11)?;
    let weak_first = Arc::downgrade(&first);
    drop((first, dropped));

    file(&mut build, 1)?;
    build.publish_partial(id(3)?).map_err(fail)?;
    assert!(!alive(&weak_first));
    assert!(is_stale(&kept, 11, &id(2)?)?);
    drop(kept);
    build
        .seal(id(4)?, ResultCompleteness::Complete)
        .map_err(fail)?;
    Ok(())
}

#[test]
fn abort_without_a_baseline_releases_the_withdrawn_partial() -> Result<(), String> {
    let sink = IngestSink::default();
    let sets = ResultSets::new(sink.clone());
    let mut build = new_build(&sink, 1)?;
    file(&mut build, 1)?;
    let partial = build.publish_partial(id(2)?).map_err(fail)?;
    register_folder(&sets, &partial, 10)?;
    let weak_partial = Arc::downgrade(&partial);
    drop(partial);
    assert!(sink.pin().map_err(fail)?.is_some());

    build.abort();
    assert!(!alive(&weak_partial));
    assert!(is_stale(&sets, 10, &id(2)?)?);
    assert!(sink.pin().map_err(fail)?.is_none());
    Ok(())
}

#[test]
fn plain_seal_without_partials_leaves_results_registrable() -> Result<(), String> {
    let sink = IngestSink::default();
    let sets = ResultSets::new(sink.clone());
    let mut build = new_build(&sink, 1)?;
    file(&mut build, 1)?;
    let sealed = build
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(fail)?;
    register_folder(&sets, &sealed, 10)?;
    let page = sets.open_page(&id(10)?, &id(2)?, 0, None).map_err(fail)?;
    assert_eq!(page.items().len(), 1);
    assert!(sink.pin().map_err(fail)?.is_some());
    Ok(())
}

#[test]
fn a_failed_publication_withdraws_the_build_and_releases_the_previous_partial() -> Result<(), String>
{
    let sink = IngestSink::default();
    let sets = ResultSets::new(sink.clone());
    let mut build = new_build(&sink, 1)?;
    file(&mut build, 1)?;
    let first = build.publish_partial(id(2)?).map_err(fail)?;
    register_folder(&sets, &first, 10)?;
    let weak_first = Arc::downgrade(&first);
    drop(first);

    build
        .add_file(Observation {
            path: vec![b"missing".to_vec(), b"child".to_vec()],
            size: None,
        })
        .map_err(fail)?;
    let failed = build.publish_partial(id(3)?);
    assert!(
        matches!(failed, Err(du_analysis_index::BuildError::InvalidTree)),
        "publication must fail with InvalidTree"
    );
    assert!(!alive(&weak_first));
    assert!(is_stale(&sets, 10, &id(2)?)?);
    assert!(sink.pin().map_err(fail)?.is_none());
    Ok(())
}

#[test]
fn concurrent_publication_and_registration_release_every_replaced_version() -> Result<(), String> {
    const PARTIALS: u64 = 200;
    let sink = IngestSink::default();
    let sets = Arc::new(ResultSets::new(sink.clone()));
    let done = Arc::new(AtomicBool::new(false));

    let reader = {
        let (sink, sets, done) = (sink.clone(), Arc::clone(&sets), Arc::clone(&done));
        std::thread::spawn(move || -> Result<(), String> {
            let mut query = 10_000_u64;
            while !done.load(Ordering::Acquire) {
                let Some(version) = sink.pin().map_err(fail)? else {
                    continue;
                };
                query += 1;
                match register_folder(&sets, &version, query) {
                    Ok(_) => {}
                    Err(error) if error == fail(PageError::StaleVersion) => continue,
                    Err(error) => return Err(error),
                }
                match sets.open_page(&id(query)?, version.version_id(), 0, None) {
                    Ok(_) | Err(PageError::StaleVersion) => {}
                    Err(error) => return Err(fail(error)),
                }
            }
            Ok(())
        })
    };
    let writer = {
        let (sink, done) = (sink.clone(), Arc::clone(&done));
        std::thread::spawn(
            move || -> Result<(Vec<Weak<ResultVersion>>, Arc<ResultVersion>), String> {
                let outcome = (|| {
                    let mut build = new_build(&sink, 1)?;
                    let mut weak = Vec::new();
                    for n in 0..PARTIALS {
                        file(&mut build, n)?;
                        let version = build.publish_partial(id(1_000 + n)?).map_err(fail)?;
                        weak.push(Arc::downgrade(&version));
                    }
                    let sealed = build
                        .seal(id(5_000)?, ResultCompleteness::Complete)
                        .map_err(fail)?;
                    Ok((weak, sealed))
                })();
                done.store(true, Ordering::Release);
                outcome
            },
        )
    };
    let (weak, sealed) = writer
        .join()
        .map_err(|_| "writer thread panicked".to_owned())??;
    reader
        .join()
        .map_err(|_| "reader thread panicked".to_owned())??;

    assert!(weak.iter().all(|version| !alive(version)));
    assert_eq!(
        sink.pin().map_err(fail)?.map(|v| v.version_id().clone()),
        Some(sealed.version_id().clone())
    );
    Ok(())
}
