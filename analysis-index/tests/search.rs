use du_analysis_index::query::{
    self, Basis, Boundary, Condition, Error, Kind, NameEncoding, NameTest, Predicate, Record,
    Registry, Sort, SortValue, Truth,
};
use du_analysis_index::{
    BuildError, IngestSink, Observation, ResultVersion, SharingEvidence, SpaceObservation,
};
use du_core_contracts::result::{ResultCompleteness, ResultVersionRead};
use du_core_contracts::{ByteSize, Id, SizeAmount, SizeBasis, SizeEvidence, SizeValue};
use std::sync::Arc;

fn id(n: u8) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}
fn err(e: BuildError) -> String {
    format!("{e:?}")
}
fn size(n: u64, lower: bool, basis: SizeBasis) -> SizeValue {
    let bytes = ByteSize::from(n);
    SizeValue {
        basis,
        evidence: SizeEvidence::OsReported,
        amount: if lower {
            SizeAmount::LowerBound { bytes }
        } else {
            SizeAmount::Exact { bytes }
        },
    }
}
fn setup() -> Result<(IngestSink, Arc<ResultVersion>), String> {
    let sink = IngestSink::default();
    let mut b = sink.open(id(1)?).map_err(err)?;
    b.add_directory(vec![]).map_err(err)?;
    for (name, value) in [
        (b"a".as_slice(), Some(size(10, false, SizeBasis::Logical))),
        (b"b".as_slice(), Some(size(10, true, SizeBasis::Logical))),
        (b"c".as_slice(), None),
    ] {
        b.add(Observation {
            path: vec![name.to_vec()],
            size: value,
        })
        .map_err(err)?;
    }
    b.add_with_space(
        Observation {
            path: vec![b"d".to_vec()],
            size: Some(size(20, false, SizeBasis::Logical)),
        },
        SpaceObservation {
            allocated: Some(size(4, false, SizeBasis::Allocated)),
            identity: None,
            link_count: Some(1),
            sharing: SharingEvidence::Unknown,
        },
    )
    .map_err(err)?;
    let version = b.seal(id(2)?, ResultCompleteness::Complete).map_err(err)?;
    Ok((sink, version))
}
fn sort<'a>(id: &'a str, basis: Basis, descending: bool) -> Sort<'a> {
    Sort {
        id,
        revision: 1,
        basis,
        descending,
    }
}
fn names(result: &query::QueryResult) -> Vec<Vec<u8>> {
    result
        .items
        .iter()
        .map(|r| result.version.items()[r.item_no as usize].name.clone())
        .collect()
}
fn q(
    version: Arc<ResultVersion>,
    condition: &Condition,
    sort: Sort<'_>,
) -> Result<query::QueryResult, Error> {
    query::run(&Registry::baseline(), version, condition, sort, || false)
}

#[test]
fn size_boundaries_propagate_lower_and_unknown() -> Result<(), Box<dyn std::error::Error>> {
    let (_, version) = setup()?;
    let tests = [
        (
            Boundary::AtLeast(10),
            vec![vec![], b"a".to_vec(), b"b".to_vec(), b"d".to_vec()],
            1,
        ),
        (Boundary::GreaterThan(10), vec![vec![], b"d".to_vec()], 2),
        (Boundary::AtMost(10), vec![b"a".to_vec()], 2),
        (Boundary::LessThan(10), vec![], 1),
    ];
    for (boundary, expected, unknown) in tests {
        let cond = Condition::Predicate(Predicate::Size(Basis::Logical, boundary));
        let result = q(
            version.clone(),
            &cond,
            sort("sort.path", Basis::Logical, false),
        )
        .map_err(|e| format!("{e:?}"))?;
        assert_eq!(names(&result), expected);
        assert_eq!(result.unknown_count, unknown);
    }
    let cond = Condition::Predicate(Predicate::Size(Basis::Allocated, Boundary::AtLeast(4)));
    let result = q(version, &cond, sort("sort.path", Basis::Allocated, false))
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(names(&result), vec![b"d".to_vec()]);
    assert_eq!(result.unknown_count, 4);
    Ok(())
}

#[test]
fn three_value_logic_and_normalization_are_deterministic() -> Result<(), Box<dyn std::error::Error>>
{
    let (_, version) = setup()?;
    let size = Condition::Predicate(Predicate::Size(Basis::Logical, Boundary::GreaterThan(10)));
    let file = Condition::Predicate(Predicate::Kind(Kind::File));
    let conjunction =
        Condition::All(vec![size.clone(), Condition::All(vec![file.clone()])]).normalize();
    assert_eq!(
        conjunction,
        Condition::All(vec![file.clone(), size.clone()]).normalize()
    );
    let result = q(
        version.clone(),
        &conjunction,
        sort("sort.path", Basis::Logical, false),
    )
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(names(&result), vec![b"d".to_vec()]);
    assert_eq!(result.unknown_count, 2);
    let not = Condition::Not(Box::new(size));
    let result =
        q(version, &not, sort("sort.path", Basis::Logical, false)).map_err(|e| format!("{e:?}"))?;
    assert_eq!(names(&result), vec![b"a".to_vec()]);
    assert_eq!(result.unknown_count, 2);
    let either = Condition::Any(vec![
        Condition::Predicate(Predicate::Size(Basis::Logical, Boundary::GreaterThan(10))),
        Condition::Predicate(Predicate::Name(
            NameTest::Equal(b"b".to_vec()),
            NameEncoding::Bytes,
        )),
    ]);
    let (_, version) = setup()?;
    let result = q(version, &either, sort("sort.path", Basis::Logical, false))
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(names(&result), vec![vec![], b"b".to_vec(), b"d".to_vec()]);
    assert_eq!(result.unknown_count, 1);
    Ok(())
}

#[test]
fn literal_name_matching_respects_windows_code_unit_alignment()
-> Result<(), Box<dyn std::error::Error>> {
    let sink = IngestSink::default();
    let mut b = sink.open(id(1)?).map_err(err)?;
    b.add_directory(vec![]).map_err(err)?;
    b.add(Observation {
        path: vec![vec![0x41, 0x00, 0x42, 0x00]],
        size: None,
    })
    .map_err(err)?;
    let version = b.seal(id(2)?, ResultCompleteness::Complete).map_err(err)?;
    let condition = Condition::Predicate(Predicate::Name(
        NameTest::Contains(vec![0, 0x42]),
        NameEncoding::WindowsUtf16Le,
    ));
    let result = q(
        version,
        &condition,
        sort("sort.path", Basis::Logical, false),
    )
    .map_err(|e| format!("{e:?}"))?;
    assert!(result.items.is_empty());
    Ok(())
}

#[test]
fn name_operators_and_unknown_kinds_preserve_three_values() -> Result<(), Box<dyn std::error::Error>>
{
    let (_, version) = setup()?;
    for test in [
        NameTest::Equal(b"a".to_vec()),
        NameTest::StartsWith(b"a".to_vec()),
        NameTest::EndsWith(b"a".to_vec()),
        NameTest::Contains(b"a".to_vec()),
    ] {
        let condition = Condition::Predicate(Predicate::Name(test, NameEncoding::Bytes));
        let result = q(
            version.clone(),
            &condition,
            sort("sort.name", Basis::Logical, false),
        )
        .map_err(|e| format!("{e:?}"))?;
        assert_eq!(names(&result), vec![b"a".to_vec()]);
    }
    let condition = Condition::Predicate(Predicate::Kind(Kind::File));
    let result = q(
        version,
        &condition,
        sort("sort.path", Basis::Logical, false),
    )
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        names(&result),
        vec![b"a".to_vec(), b"b".to_vec(), b"d".to_vec()]
    );
    assert_eq!(result.unknown_count, 1);
    Ok(())
}

fn custom(_: &Record<'_>, _: &Predicate) -> Truth {
    Truth::Match
}
fn custom_sort(_: &Record<'_>, _: Basis) -> SortValue {
    SortValue::Number(0)
}

#[test]
fn registry_rejects_duplicates_and_unregistered_revisions() -> Result<(), Box<dyn std::error::Error>>
{
    let (_, version) = setup()?;
    let mut registry = Registry::baseline();
    assert_eq!(
        registry.register_predicate("pred.name", 1, custom),
        Err(Error::DuplicateStrategy)
    );
    registry
        .register_predicate("pred.custom", 1, custom)
        .map_err(|e| format!("{e:?}"))?;
    registry
        .register_sort("sort.custom", 1, custom_sort)
        .map_err(|e| format!("{e:?}"))?;
    let custom = Condition::Predicate(Predicate::Registered {
        id: "pred.custom".into(),
        revision: 1,
        operand: vec![],
    });
    let result = query::run(
        &registry,
        version.clone(),
        &custom,
        sort("sort.custom", Basis::Logical, false),
        || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(result.items.len(), 5);
    assert_eq!(result.predicate_strategies, vec![("pred.custom".into(), 1)]);
    assert_eq!(result.sort_strategy, ("sort.custom".into(), 1));
    assert!(matches!(
        query::run(
            &registry,
            version,
            &custom,
            Sort {
                revision: 2,
                ..sort("sort.custom", Basis::Logical, false)
            },
            || false
        ),
        Err(Error::UnknownSort)
    ));
    Ok(())
}

#[test]
fn sorting_is_stable_cancellation_is_atomic_and_snapshot_is_pinned()
-> Result<(), Box<dyn std::error::Error>> {
    let (sink, version) = setup()?;
    let all = Condition::All(vec![]);
    let result = q(
        version.clone(),
        &all,
        sort("sort.size", Basis::Logical, true),
    )
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        &names(&result)[..4],
        &[vec![], b"d".to_vec(), b"a".to_vec(), b"b".to_vec()]
    );
    assert_eq!(names(&result).last(), Some(&b"c".to_vec()));
    let mut polls = 0;
    let outcome = query::run(
        &Registry::baseline(),
        version,
        &all,
        sort("sort.path", Basis::Logical, false),
        || {
            polls += 1;
            polls == 3
        },
    );
    assert!(matches!(outcome, Err(Error::Cancelled)));
    let mut next = sink.open(id(1)?).map_err(err)?;
    next.add_directory(vec![]).map_err(err)?;
    next.seal(id(3)?, ResultCompleteness::Complete)
        .map_err(err)?;
    assert_eq!(result.version.version_id(), &id(2)?);
    assert_eq!(result.items.len(), 5);
    Ok(())
}

#[test]
fn flattened_groups_obey_limits_and_normalized_results_are_reusable()
-> Result<(), Box<dyn std::error::Error>> {
    let (_, version) = setup()?;
    let leaf = Condition::Predicate(Predicate::Kind(Kind::File));
    for all in [true, false] {
        let group = |children| {
            if all {
                Condition::All(children)
            } else {
                Condition::Any(children)
            }
        };
        let accepted = group(vec![
            group(vec![leaf.clone(); 128]),
            group(vec![leaf.clone(); 128]),
        ]);
        let first = q(
            version.clone(),
            &accepted,
            sort("sort.path", Basis::Logical, false),
        )
        .map_err(|e| format!("{e:?}"))?;
        let again = q(
            version.clone(),
            &first.condition,
            sort("sort.path", Basis::Logical, false),
        )
        .map_err(|e| format!("{e:?}"))?;
        assert_eq!(first.condition, again.condition);
        assert_eq!(first.items, again.items);
        assert_eq!(first.unknown_count, again.unknown_count);
        let too_many = group(vec![
            group(vec![leaf.clone(); 128]),
            group(vec![leaf.clone(); 129]),
        ]);
        assert!(matches!(
            q(
                version.clone(),
                &too_many,
                sort("sort.path", Basis::Logical, false)
            ),
            Err(Error::InvalidCondition)
        ));
    }
    Ok(())
}

#[test]
fn utf16le_raw_sort_is_not_utf16_code_unit_sort() -> Result<(), Box<dyn std::error::Error>> {
    let sink = IngestSink::default();
    let mut b = sink.open(id(1)?).map_err(err)?;
    b.add_directory(vec![]).map_err(err)?;
    for unit in [0x00ff_u16, 0x0100_u16] {
        b.add(Observation {
            path: vec![unit.to_le_bytes().to_vec()],
            size: Some(size(1, false, SizeBasis::Logical)),
        })
        .map_err(err)?;
    }
    let version = b.seal(id(2)?, ResultCompleteness::Complete).map_err(err)?;
    let condition = Condition::Predicate(Predicate::Kind(Kind::File));
    let result = q(
        version,
        &condition,
        sort("sort.name", Basis::Logical, false),
    )
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        names(&result),
        vec![
            0x0100_u16.to_le_bytes().to_vec(),
            0x00ff_u16.to_le_bytes().to_vec()
        ]
    );
    assert!(result.limitations.contains(&"query.raw_path_order"));
    Ok(())
}
