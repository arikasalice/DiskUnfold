use du_analysis_index::{
    BuildHandle, FileIdentity, IngestSink, Observation, ResultVersion, SharingEvidence,
    SpaceObservation,
    query::{
        self, Basis, Boundary, Condition, Kind, Predicate, Registry, Sort, SpaceTotalError,
        page::{PageError, ResultSets},
    },
};
use du_core_contracts::{
    Id, SizeAmount, SizeBasis, SizeEvidence, SizeValue, result::ResultCompleteness,
};
use std::sync::Arc;

fn id(n: u8) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}

fn exact(bytes: u64) -> SizeAmount {
    SizeAmount::Exact {
        bytes: bytes.into(),
    }
}

fn add(build: &mut BuildHandle, name: &[u8], identity: u8, bytes: u64) -> Result<(), String> {
    build
        .add_with_space(
            Observation {
                path: vec![name.to_vec()],
                size: Some(SizeValue {
                    basis: SizeBasis::Logical,
                    evidence: SizeEvidence::Measured,
                    amount: exact(bytes),
                }),
            },
            SpaceObservation {
                allocated: Some(SizeValue {
                    basis: SizeBasis::Allocated,
                    evidence: SizeEvidence::OsReported,
                    amount: exact(bytes),
                }),
                identity: Some(FileIdentity {
                    volume: vec![1],
                    file: vec![identity],
                }),
                link_count: Some(2),
                sharing: SharingEvidence::None,
            },
        )
        .map_err(|error| format!("{error:?}"))
}

fn files() -> Condition {
    Condition::Predicate(Predicate::Kind(Kind::File))
}

fn run(version: Arc<ResultVersion>, condition: &Condition) -> Result<query::QueryResult, String> {
    query::run(
        &Registry::baseline(),
        version,
        condition,
        Sort {
            id: "sort.path",
            revision: 1,
            basis: Basis::Logical,
            descending: false,
        },
        || false,
    )
    .map_err(|error| format!("{error:?}"))
}

#[test]
fn filtered_set_deduplicates_hardlinks_without_adding_excluded_files() -> Result<(), String> {
    let store = IngestSink::default();
    let mut build = store.open(id(1)?).map_err(|error| format!("{error:?}"))?;
    build
        .add_directory(vec![])
        .map_err(|error| format!("{error:?}"))?;
    add(&mut build, b"alias-a", 1, 8)?;
    add(&mut build, b"alias-b", 1, 8)?;
    add(&mut build, b"other", 2, 64)?;
    let version = build
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(|error| format!("{error:?}"))?;
    let condition = Condition::All(vec![
        files(),
        Condition::Predicate(Predicate::Size(Basis::Logical, Boundary::AtMost(8))),
    ]);
    let mut selected = run(version.clone(), &condition)?;
    selected.items.push(selected.items[0].clone());
    let total = selected
        .space_total(|| false)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(total.logical.amount, exact(16));
    assert_eq!(total.allocated.amount, exact(8));
    assert_eq!(total.unique_physical.amount, exact(8));
    assert_eq!(version.scope_total().allocated.amount, exact(72));
    let empty = run(version, &Condition::Any(vec![]))?
        .space_total(|| false)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(empty.allocated.amount, exact(0));
    Ok(())
}

#[test]
fn missing_attributes_and_indeterminate_membership_are_not_partial_totals() -> Result<(), String> {
    let store = IngestSink::default();
    let mut build = store.open(id(1)?).map_err(|error| format!("{error:?}"))?;
    build
        .add_directory(vec![])
        .map_err(|error| format!("{error:?}"))?;
    add(&mut build, b"known", 1, 8)?;
    build
        .add_file(Observation {
            path: vec![b"unknown".to_vec()],
            size: None,
        })
        .map_err(|error| format!("{error:?}"))?;
    let version = build
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(|error| format!("{error:?}"))?;
    for condition in [
        files(),
        Condition::All(vec![
            files(),
            Condition::Predicate(Predicate::Size(Basis::Logical, Boundary::AtLeast(1))),
        ]),
        Condition::All(vec![]),
    ] {
        let aggregate = run(version.clone(), &condition)?
            .space_total(|| false)
            .map_err(|error| format!("{error:?}"))?;
        assert_eq!(aggregate.logical.amount, SizeAmount::Unknown);
        assert_eq!(aggregate.allocated.amount, SizeAmount::Unknown);
        assert_eq!(aggregate.unique_physical.amount, SizeAmount::Unknown);
        assert!(aggregate.limitations.contains(&"space.data_insufficient"));
    }
    Ok(())
}

#[test]
fn physical_capacity_and_cancellation_do_not_change_the_logical_observations() -> Result<(), String>
{
    let store = IngestSink::default();
    let mut build = store.open(id(1)?).map_err(|error| format!("{error:?}"))?;
    build
        .set_shared_table_limit(1)
        .map_err(|error| format!("{error:?}"))?;
    build
        .add_directory(vec![])
        .map_err(|error| format!("{error:?}"))?;
    add(&mut build, b"one", 1, 8)?;
    let partial = build
        .publish_partial(id(2)?)
        .map_err(|error| format!("{error:?}"))?;
    add(&mut build, b"two", 2, 32)?;
    let version = build
        .seal(id(3)?, ResultCompleteness::Complete)
        .map_err(|error| format!("{error:?}"))?;
    let prior = run(partial, &files())?
        .space_total(|| false)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(prior.completeness, ResultCompleteness::Partial);
    assert_eq!(prior.logical.amount, SizeAmount::Unknown);
    let result = run(version, &files())?;
    let sets = ResultSets::new(store);
    let query_ref = id(4)?;
    let version_ref = id(3)?;
    sets.register(query_ref.clone(), result)
        .map_err(|error| format!("{error:?}"))?;
    let pinned = sets
        .open_parent(&query_ref, &version_ref)
        .map_err(|error| format!("{error:?}"))?;
    sets.release(&query_ref)
        .map_err(|error| format!("{error:?}"))?;
    assert!(matches!(
        sets.open_parent(&query_ref, &version_ref),
        Err(PageError::StaleVersion)
    ));
    let total = pinned
        .space_total(|| false)
        .map_err(|error| format!("{error:?}"))?;
    let pinned_range = pinned
        .space_total_with_observation_range(|| false)
        .map_err(|error| format!("{error:?}"))?
        .observation_range;
    assert!(!pinned_range.complete);
    assert_eq!((pinned_range.min, pinned_range.max), (None, None));
    assert_eq!(total.logical.amount, exact(40));
    assert_eq!(total.allocated.amount, SizeAmount::Unknown);
    assert_eq!(total.unique_physical.amount, SizeAmount::Unknown);
    assert!(total.limitations.contains(&"space.dedup_capacity"));
    let mut checks = 0;
    assert_eq!(
        pinned.space_total(|| {
            checks += 1;
            checks == 2
        }),
        Err(SpaceTotalError::Cancelled)
    );
    Ok(())
}

#[test]
fn query_total_ranges_follow_pinned_file_membership_and_cancellation() -> Result<(), String> {
    use du_core_contracts::{ItemRef, Timestamp};
    let store = IngestSink::default();
    let mut build = store.open(id(10)?).map_err(|e| format!("{e:?}"))?;
    build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    add(&mut build, b"a", 1, 8)?;
    add(&mut build, b"b", 1, 8)?;
    let early = Timestamp::from_unix_millis(10).ok_or("time")?;
    let late = Timestamp::from_unix_millis(30).ok_or("time")?;
    build
        .set_item_observed_at(0, &[b"a".to_vec()], late.clone())
        .map_err(|e| format!("{e:?}"))?;
    let partial = build
        .publish_partial(id(11)?)
        .map_err(|e| format!("{e:?}"))?;
    build
        .set_item_observed_at(0, &[b"b".to_vec()], early.clone())
        .map_err(|e| format!("{e:?}"))?;
    let version = build
        .seal_at(
            id(12)?,
            ResultCompleteness::Complete,
            Timestamp::from_unix_millis(99),
        )
        .map_err(|e| format!("{e:?}"))?;
    let old = run(partial, &files())?;
    let prior = old
        .space_total_with_observation_range(|| false)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        (
            prior.observation_range.min,
            prior.observation_range.max,
            prior.observation_range.complete
        ),
        (Some(&late), Some(&late), false)
    );
    let mut query = run(version.clone(), &files())?;
    query.items.push(query.items[0].clone());
    let result = query
        .space_total_with_observation_range(|| false)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        (
            result.observation_range.min,
            result.observation_range.max,
            result.observation_range.complete
        ),
        (Some(&early), Some(&late), true)
    );
    assert_eq!(result.aggregate.allocated.amount, exact(8));
    let empty = run(version.clone(), &Condition::Any(vec![]))?;
    let range = empty
        .space_total_with_observation_range(|| false)
        .map_err(|e| format!("{e:?}"))?
        .observation_range;
    assert_eq!((range.min, range.max, range.complete), (None, None, true));
    let all = run(version, &Condition::All(vec![]))?;
    let range = all
        .space_total_with_observation_range(|| false)
        .map_err(|e| format!("{e:?}"))?
        .observation_range;
    assert_eq!(
        (range.min, range.max, range.complete),
        (Some(&early), Some(&late), false)
    );
    let mut count = 0;
    assert!(matches!(
        query.space_total_with_observation_range(|| {
            count += 1;
            count == 2
        }),
        Err(SpaceTotalError::Cancelled)
    ));
    query.items.push(ItemRef {
        result_version: id(11)?,
        item_no: 0,
    });
    assert!(matches!(
        query.space_total_with_observation_range(|| false),
        Err(SpaceTotalError::Item(_))
    ));
    Ok(())
}

#[test]
fn missing_timestamps_and_unknown_predicates_mark_range_incomplete() -> Result<(), String> {
    use du_core_contracts::Timestamp;
    let store = IngestSink::default();
    let mut build = store.open(id(20)?).map_err(|e| format!("{e:?}"))?;
    build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    add(&mut build, b"a", 1, 8)?;
    build
        .add_file(Observation {
            path: vec![b"b".to_vec()],
            size: None,
        })
        .map_err(|e| format!("{e:?}"))?;
    let time = Timestamp::from_unix_millis(10).ok_or("time")?;
    build
        .set_item_observed_at(0, &[b"a".to_vec()], time.clone())
        .map_err(|e| format!("{e:?}"))?;
    let version = build
        .seal(id(21)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let all = run(version.clone(), &files())?;
    let range = all
        .space_total_with_observation_range(|| false)
        .map_err(|e| format!("{e:?}"))?
        .observation_range;
    assert_eq!(
        (range.min, range.max, range.complete),
        (Some(&time), Some(&time), false)
    );
    let uncertain = run(
        version,
        &Condition::All(vec![
            files(),
            Condition::Predicate(Predicate::Size(Basis::Logical, Boundary::AtLeast(1))),
        ]),
    )?;
    assert!(uncertain.unknown_count > 0);
    let result = uncertain
        .space_total_with_observation_range(|| false)
        .map_err(|e| format!("{e:?}"))?;
    let refined = query::run_in_result(
        &Registry::baseline(),
        &uncertain,
        &files(),
        Sort {
            id: "sort.path",
            revision: 1,
            basis: Basis::Logical,
            descending: false,
        },
        || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(refined.unknown_count, 0);
    let inherited = refined
        .space_total_with_observation_range(|| false)
        .map_err(|e| format!("{e:?}"))?;
    assert!(!inherited.observation_range.complete);
    assert_eq!(result.aggregate.logical.amount, SizeAmount::Unknown);
    assert_eq!(
        (
            result.observation_range.min,
            result.observation_range.max,
            result.observation_range.complete
        ),
        (Some(&time), Some(&time), false)
    );
    Ok(())
}

#[test]
fn incomplete_total_still_validates_later_refs_and_honors_cancellation() -> Result<(), String> {
    use du_core_contracts::ItemRef;
    let store = IngestSink::default();
    let mut build = store.open(id(30)?).map_err(|e| format!("{e:?}"))?;
    build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    add(&mut build, b"file", 1, 8)?;
    let partial = build
        .publish_partial(id(31)?)
        .map_err(|e| format!("{e:?}"))?;
    let mut query = run(partial, &files())?;
    query.items.push(ItemRef {
        result_version: id(32)?,
        item_no: 0,
    });
    assert!(matches!(
        query.space_total(|| false),
        Err(SpaceTotalError::Item(_))
    ));
    let mut checks = 0;
    assert_eq!(
        query.space_total(|| {
            checks += 1;
            checks == 2
        }),
        Err(SpaceTotalError::Cancelled)
    );
    Ok(())
}
