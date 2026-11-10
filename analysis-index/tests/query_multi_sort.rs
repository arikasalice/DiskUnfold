use du_analysis_index::{
    IngestSink, Observation, ResultVersion,
    query::{
        self, Basis, Condition, Error, Record, Registry, Sort, SortValue,
        page::{MultiFolderKey, PageError, ResultSets},
    },
};
use du_core_contracts::{
    ByteSize, Id, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    result::{ResultCompleteness, ResultVersionRead},
};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

static SORT_CALLS: AtomicUsize = AtomicUsize::new(0);
fn counted(_: &Record<'_>, _: Basis) -> SortValue {
    SORT_CALLS.fetch_add(1, Ordering::Relaxed);
    SortValue::Number(1)
}

fn id(n: u8) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}
fn fixture() -> Result<(IngestSink, Arc<ResultVersion>), String> {
    let sink = IngestSink::default();
    let mut build = sink.open(id(1)?).map_err(|e| format!("{e:?}"))?;
    build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    for (name, amount) in [
        ("a", Some(10)),
        ("b", Some(10)),
        ("c", Some(5)),
        ("d", None),
    ] {
        let size = amount.map(|value| SizeValue {
            basis: SizeBasis::Logical,
            amount: SizeAmount::Exact {
                bytes: ByteSize::from(value),
            },
            evidence: SizeEvidence::Measured,
        });
        build
            .add_file(Observation {
                path: vec![name.as_bytes().to_vec()],
                size,
            })
            .map_err(|e| format!("{e:?}"))?;
    }
    let version = build
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    Ok((sink, version))
}
fn size(descending: bool) -> Sort<'static> {
    Sort {
        id: "sort.size",
        revision: 1,
        basis: Basis::Logical,
        descending,
    }
}
fn name(descending: bool) -> Sort<'static> {
    Sort {
        id: "sort.name",
        revision: 1,
        basis: Basis::Logical,
        descending,
    }
}
fn names(result: &query::QueryResult) -> Vec<Vec<u8>> {
    result
        .items
        .iter()
        .map(|item| result.version.items()[item.item_no as usize].name.clone())
        .collect()
}

#[test]
fn multiple_directions_secondary_and_unknown_are_stable() -> Result<(), String> {
    let (_, version) = fixture()?;
    let registry = Registry::baseline();
    let condition = Condition::All(vec![]);
    let sorted = query::run_folder_with_sorts(
        &registry,
        version.clone(),
        None,
        &condition,
        size(true),
        &[name(false)],
        || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        names(&sorted),
        [b"a".to_vec(), b"b".to_vec(), b"c".to_vec(), b"d".to_vec()]
    );
    assert_eq!(
        sorted
            .sorts
            .iter()
            .map(|s| (s.id.as_str(), s.revision, s.basis, s.descending))
            .collect::<Vec<_>>(),
        [
            ("sort.size", 1, Basis::Logical, true),
            ("sort.name", 1, Basis::Logical, false)
        ]
    );
    let reversed = query::run_folder_with_sorts(
        &registry,
        version.clone(),
        None,
        &condition,
        size(false),
        &[name(true)],
        || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        names(&reversed),
        [b"c".to_vec(), b"b".to_vec(), b"a".to_vec(), b"d".to_vec()]
    );
    let legacy = query::run_folder(
        &registry,
        version.clone(),
        None,
        &condition,
        size(true),
        || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    let equivalent = query::run_folder_with_sorts(
        &registry,
        version,
        None,
        &condition,
        size(true),
        &[],
        || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(legacy.items, equivalent.items);
    assert_eq!(legacy.sorts, equivalent.sorts);
    Ok(())
}

#[test]
fn tie_uses_native_path_and_pages_retain_all_keys() -> Result<(), String> {
    let (sink, version) = fixture()?;
    let registry = Registry::baseline();
    let condition = Condition::All(vec![]);
    let result = query::run_folder_with_sorts(
        &registry,
        version.clone(),
        None,
        &condition,
        size(true),
        &[size(true)],
        || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        names(&result),
        [b"a".to_vec(), b"b".to_vec(), b"c".to_vec(), b"d".to_vec()]
    );
    let expected = result.items.clone();
    let sets = ResultSets::new(sink);
    let key = MultiFolderKey {
        version: id(2)?,
        folder: 0,
        tree: false,
        condition: condition.clone(),
        sorts: result.sorts.clone(),
    };
    sets.register_multi_folder(key.clone(), id(3)?, result)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        sets.find_multi_folder(&key).map_err(|e| format!("{e:?}"))?,
        Some(id(3)?)
    );
    let mut different = key;
    different.sorts.reverse();
    different.sorts[1].descending = false;
    assert_eq!(
        sets.find_multi_folder(&different)
            .map_err(|e| format!("{e:?}"))?,
        None
    );
    let first = sets
        .open_page(&id(3)?, &id(2)?, 0, Some(2))
        .map_err(|e| format!("{e:?}"))?;
    let second = sets
        .open_page(&id(3)?, &id(2)?, 2, Some(2))
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!([first.items(), second.items()].concat(), expected);
    assert_eq!(first.metadata().sorts, second.metadata().sorts);
    assert_eq!(first.metadata().sorts.len(), 2);
    let repeat = sets
        .open_page(&id(3)?, &id(2)?, 0, Some(2))
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(first.items(), repeat.items());
    assert!(matches!(
        sets.open_page(&id(3)?, &id(2)?, 5, Some(1)),
        Err(PageError::InvalidRange)
    ));
    Ok(())
}

#[test]
fn unregistered_revision_and_cancellation_leave_no_result() -> Result<(), String> {
    let (_, version) = fixture()?;
    let mut registry = Registry::baseline();
    registry
        .register_sort("sort.counted", 1, counted)
        .map_err(|e| format!("{e:?}"))?;
    let condition = Condition::All(vec![]);
    let unsupported = Sort {
        revision: 2,
        ..name(false)
    };
    assert!(matches!(
        query::run_with_sorts(
            &registry,
            version.clone(),
            &condition,
            size(true),
            &[unsupported],
            || false
        ),
        Err(Error::UnknownSort)
    ));
    SORT_CALLS.store(0, Ordering::Relaxed);
    let first = Sort {
        id: "sort.counted",
        revision: 1,
        basis: Basis::Logical,
        descending: false,
    };
    assert!(matches!(
        query::run_with_sorts(
            &registry,
            version.clone(),
            &condition,
            first,
            &[unsupported],
            || false
        ),
        Err(Error::UnknownSort)
    ));
    assert_eq!(SORT_CALLS.load(Ordering::Relaxed), 0);
    let mut calls = 0;
    assert!(matches!(
        query::run_folder_with_sorts(
            &registry,
            version,
            None,
            &condition,
            size(true),
            &[name(false)],
            || {
                calls += 1;
                calls > 1
            }
        ),
        Err(Error::Cancelled)
    ));
    Ok(())
}
