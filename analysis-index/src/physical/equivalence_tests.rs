//! The whole-build folder totals must equal one identity table per subtree.
//!
//! The reference is the single-table calculation behind `scope_total`, run on
//! the files below each item with the same repeated identities tabled.
use super::*;
use crate::space;
use du_core_contracts::result::ResultItemKind;
use std::sync::Arc;

const FORESTS: u64 = 300;
const COMPLETENESS: [ResultCompleteness; 4] = [
    ResultCompleteness::Complete,
    ResultCompleteness::Partial,
    ResultCompleteness::Missing,
    ResultCompleteness::Cancelled,
];
const LIMITS: [usize; 9] = [0, 1, 2, 3, 5, 9, 40, 1_000_000, usize::MAX];
const RESULT_KINDS: [ResultItemKind; 5] = [
    ResultItemKind::Directory,
    ResultItemKind::File,
    ResultItemKind::UntraversedLink,
    ResultItemKind::Unknown,
    ResultItemKind::UnknownStructure,
];

/// A fixed pseudo-random sequence, so every run checks the same forests.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, bound: usize) -> usize {
        usize::try_from(self.next() % bound as u64).unwrap_or_default()
    }

    fn chance(&mut self, percent: usize) -> bool {
        self.below(100) < percent
    }
}

struct Forest {
    items: Vec<ResultItem>,
    observations: Option<Vec<Option<Arc<SpaceObservation>>>>,
}

fn values_from_index(
    items: &[ResultItem],
    indexed: &space::SpaceAggregates,
    observations: Option<&PhysicalObservations>,
    completeness: ResultCompleteness,
    limit: usize,
) -> Result<Vec<SpaceAggregate>, String> {
    items
        .iter()
        .map(|item| {
            if expected_is_structure(item.kind) {
                return indexed
                    .get(item.item_no)
                    .cloned()
                    .ok_or_else(|| "missing structural item aggregate".to_owned());
            }
            let mut aggregate = space::aggregate_item(item, completeness);
            let observation = observations.and_then(|values| values.get(item.item_no));
            let reading = Observed::read(observation);
            let mut own = Physical::with_limit(limit);
            own.note_placeholder(&reading, false);
            let _ = own.record(&reading, false);
            own.finish(completeness, &mut aggregate);
            Ok(aggregate)
        })
        .collect()
}

/// Test oracle for the established result-kind boundary, independent of production code.
fn expected_is_structure(kind: ResultItemKind) -> bool {
    matches!(
        kind,
        ResultItemKind::Directory | ResultItemKind::UnknownStructure
    )
}

fn value(basis: SizeBasis, amount: SizeAmount, evidence: SizeEvidence) -> SizeValue {
    SizeValue {
        basis,
        amount,
        evidence,
    }
}

fn observation(rng: &mut Rng, pool: usize) -> Option<Arc<SpaceObservation>> {
    if rng.chance(10) {
        return None;
    }
    let file = rng.below(pool);
    let mut bytes = 1 + (file % 3) as u64;
    if rng.chance(12) {
        bytes = 1 + rng.below(4) as u64;
    }
    let exact = SizeAmount::Exact {
        bytes: bytes.into(),
    };
    let amount = if rng.chance(10) {
        SizeAmount::LowerBound {
            bytes: bytes.into(),
        }
    } else {
        exact.clone()
    };
    let allocated = match rng.below(100) {
        0..6 => None,
        6..10 => Some(value(
            SizeBasis::Allocated,
            SizeAmount::Unknown,
            SizeEvidence::OsReported,
        )),
        10..13 => Some(value(SizeBasis::Logical, exact, SizeEvidence::OsReported)),
        13..16 => Some(value(SizeBasis::Allocated, exact, SizeEvidence::Measured)),
        _ => Some(value(
            SizeBasis::Allocated,
            amount,
            SizeEvidence::OsReported,
        )),
    };
    let byte = u8::try_from(file).unwrap_or_default();
    let identity = match rng.below(100) {
        0..8 => None,
        8..10 => Some(FileIdentity {
            volume: Vec::new(),
            file: vec![byte],
        }),
        10..12 => Some(FileIdentity {
            volume: vec![1],
            file: Vec::new(),
        }),
        _ => Some(FileIdentity {
            volume: vec![1 + byte % 2],
            file: vec![byte],
        }),
    };
    let link_count = match rng.below(100) {
        0..5 => Some(0),
        5..35 => Some(1 + rng.below(3) as u64),
        _ => None,
    };
    let sharing = if rng.chance(50) {
        SharingEvidence::None
    } else {
        SharingEvidence::Unknown
    };
    Some(Arc::new(SpaceObservation {
        allocated,
        identity,
        link_count,
        sharing,
    }))
}

fn forest(rng: &mut Rng) -> Forest {
    let count = 20 + rng.below(181);
    let pool = 2 + rng.below(count);
    let mut items = Vec::with_capacity(count);
    let mut observations = Vec::with_capacity(count);
    let mut directories: Vec<u32> = Vec::new();
    for number in 0..count {
        let item_no = u32::try_from(number).unwrap_or_default();
        let kind = RESULT_KINDS
            .get(number)
            .copied()
            .unwrap_or_else(|| RESULT_KINDS[rng.below(RESULT_KINDS.len())]);
        let parent =
            (number > 0 && !rng.chance(3)).then(|| directories[rng.below(directories.len())]);
        if expected_is_structure(kind) {
            directories.push(item_no);
        }
        items.push(ResultItem {
            item_no,
            parent,
            name: Vec::new(),
            size: None,
            kind,
        });
        observations.push(if expected_is_structure(kind) {
            None
        } else {
            observation(rng, pool)
        });
    }
    Forest {
        items,
        observations: (!rng.chance(5)).then_some(observations),
    }
}

fn is_below(items: &[ResultItem], ancestor: usize, mut item: usize) -> bool {
    loop {
        if item == ancestor {
            return true;
        }
        match items[item].parent {
            Some(parent) => item = parent as usize,
            None => return false,
        }
    }
}

/// Logical expectations for generated forests, whose items all have no size observation.
fn expected_logical(
    items: &[ResultItem],
    ancestor: usize,
    completeness: ResultCompleteness,
) -> SpaceAggregate {
    let mut missing_items = 0;
    let mut untraversed_links = 0;
    for (index, item) in items.iter().enumerate() {
        if !is_below(items, ancestor, index) {
            continue;
        }
        match item.kind {
            ResultItemKind::Directory | ResultItemKind::UnknownStructure => {}
            ResultItemKind::UntraversedLink => untraversed_links += 1,
            ResultItemKind::File | ResultItemKind::Unknown => missing_items += 1,
        }
    }
    let completeness = match completeness {
        ResultCompleteness::Complete if missing_items > 0 || untraversed_links > 0 => {
            ResultCompleteness::Missing
        }
        other => other,
    };
    let amount = if completeness == ResultCompleteness::Complete {
        SizeAmount::Exact {
            bytes: ByteSize::from(0),
        }
    } else {
        SizeAmount::Unknown
    };
    let evidence = if amount == SizeAmount::Unknown {
        SizeEvidence::Unknown
    } else {
        SizeEvidence::Measured
    };
    SpaceAggregate {
        logical: value(SizeBasis::Logical, amount, evidence),
        allocated: value(
            SizeBasis::Allocated,
            SizeAmount::Unknown,
            SizeEvidence::Unknown,
        ),
        unique_physical: value(
            SizeBasis::UniquePhysical,
            SizeAmount::Unknown,
            SizeEvidence::Unknown,
        ),
        completeness,
        allocated_completeness: ResultCompleteness::Missing,
        unique_physical_completeness: ResultCompleteness::Missing,
        missing_items,
        untraversed_links,
        inaccessible_ranges: None,
        limitations: std::borrow::Cow::Borrowed(&[
            "space.allocated_not_collected",
            "space.sharing_not_collected",
            "space.aux_not_included",
            "space.observation_detail_insufficient",
        ]),
    }
}

/// What the single-table calculation yields for every item.
fn reference(
    forest: &Forest,
    completeness: ResultCompleteness,
    limit: usize,
    reversed: bool,
) -> Vec<SpaceAggregate> {
    let mut expected: Vec<_> = (0..forest.items.len())
        .map(|index| expected_logical(&forest.items, index, completeness))
        .collect();
    let compact = observations::compact_test_observations(forest.observations.as_deref());
    let repeated = compact
        .as_ref()
        .map(|observations| subtree::repeated_identities(&forest.items, observations));
    for (index, output) in expected.iter_mut().enumerate() {
        let mut sum = Physical::with_limit(limit);
        let files: Vec<usize> = (0..forest.items.len())
            .filter(|file| {
                !expected_is_structure(forest.items[*file].kind)
                    && is_below(&forest.items, index, *file)
            })
            .collect();
        let order: Box<dyn Iterator<Item = &usize>> = if reversed {
            Box::new(files.iter().rev())
        } else {
            Box::new(files.iter())
        };
        for file in order {
            let input = compact
                .as_ref()
                .and_then(|values| values.get(u32::try_from(*file).unwrap_or_default()));
            // A file that is its own scope has no other file to share with.
            let tabled =
                *file != index && repeated.as_deref().is_none_or(|repeated| repeated[*file]);
            let _ = sum.record(&Observed::read(input), tabled);
        }
        sum.finish(completeness, output);
    }
    expected
}

#[test]
fn every_result_kind_keeps_its_subtree_propagation() -> Result<(), String> {
    let items: Vec<_> = RESULT_KINDS
        .into_iter()
        .enumerate()
        .map(|(index, kind)| ResultItem {
            item_no: u32::try_from(index).unwrap_or_default(),
            parent: (index > 0).then_some(0),
            name: Vec::new(),
            size: None,
            kind,
        })
        .collect();
    let observation = |file, bytes| {
        Some(Arc::new(SpaceObservation {
            allocated: Some(value(
                SizeBasis::Allocated,
                SizeAmount::Exact {
                    bytes: ByteSize::from(bytes),
                },
                SizeEvidence::OsReported,
            )),
            identity: Some(FileIdentity {
                volume: vec![1],
                file: vec![file],
            }),
            link_count: Some(1),
            sharing: SharingEvidence::None,
        }))
    };
    let forest = Forest {
        items,
        observations: Some(vec![
            None,
            observation(1, 11),
            observation(2, 17),
            observation(3, 23),
            None,
        ]),
    };
    let expected = reference(&forest, ResultCompleteness::Complete, 3, false);
    let mut actual = space::aggregate(&forest.items, &[], ResultCompleteness::Complete, None);
    let compact = observations::compact_test_observations(forest.observations.as_deref());
    apply(
        &forest.items,
        FileEvidence {
            observations: compact.as_ref(),
            placeholders: None,
        },
        &[],
        3,
        &mut actual,
    );

    let actual = values_from_index(
        &forest.items,
        &actual,
        compact.as_ref(),
        ResultCompleteness::Complete,
        3,
    )?;
    assert_eq!(actual, expected);
    assert_eq!(expected[0].missing_items, 2);
    assert_eq!(expected[0].untraversed_links, 1);
    assert_eq!(
        expected[0].allocated.amount,
        SizeAmount::Exact {
            bytes: ByteSize::from(51)
        }
    );
    for (index, (missing, links, bytes)) in
        [(2, 1, 51), (1, 0, 11), (0, 1, 17), (1, 0, 23), (0, 0, 0)]
            .into_iter()
            .enumerate()
    {
        assert_eq!(expected[index].missing_items, missing, "item {index}");
        assert_eq!(expected[index].untraversed_links, links, "item {index}");
        assert_eq!(
            expected[index].allocated.amount,
            SizeAmount::Exact {
                bytes: ByteSize::from(bytes)
            },
            "item {index}"
        );
    }
    Ok(())
}

#[derive(Default)]
struct Coverage {
    compared: usize,
    capacity: usize,
    identity_unknown: usize,
    sharing_unknown: usize,
    exact: usize,
    lower: usize,
    unknown: usize,
}

impl Coverage {
    fn note(&mut self, aggregate: &SpaceAggregate) {
        self.compared += 1;
        let has = |key: &'static str| aggregate.limitations.contains(&key);
        self.capacity += usize::from(has("space.dedup_capacity"));
        self.identity_unknown += usize::from(has("space.hardlink_identity_unknown"));
        self.sharing_unknown += usize::from(has("space.sharing_not_collected"));
        match aggregate.allocated.amount {
            SizeAmount::Exact { .. } => self.exact += 1,
            SizeAmount::LowerBound { .. } => self.lower += 1,
            SizeAmount::Unknown => self.unknown += 1,
        }
    }
}

#[test]
fn whole_build_totals_equal_one_table_per_subtree() -> Result<(), String> {
    let mut coverage = Coverage::default();
    for seed in 0..FORESTS {
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15_u64.wrapping_mul(seed + 1) | 1);
        for _ in 0..4 {
            rng.next();
        }
        let forest = forest(&mut rng);
        let limit = LIMITS[rng.below(LIMITS.len())];
        for completeness in COMPLETENESS {
            let expected = reference(&forest, completeness, limit, seed % 2 == 0);
            let mut actual = space::aggregate(&forest.items, &[], completeness, None);
            let compact = observations::compact_test_observations(forest.observations.as_deref());
            apply(
                &forest.items,
                FileEvidence {
                    observations: compact.as_ref(),
                    placeholders: None,
                },
                &[],
                limit,
                &mut actual,
            );
            let actual = values_from_index(
                &forest.items,
                &actual,
                compact.as_ref(),
                completeness,
                limit,
            )?;
            for (index, (want, got)) in expected.iter().zip(&actual).enumerate() {
                if want != got {
                    return Err(format!(
                        "forest {seed}, item {index}, limit {limit}, {completeness:?}: \
                         expected {want:?}, got {got:?}"
                    ));
                }
                coverage.note(got);
            }
        }
    }
    assert!(
        coverage.compared > 100_000,
        "compared {}",
        coverage.compared
    );
    for (name, count) in [
        ("capacity", coverage.capacity),
        ("identity unknown", coverage.identity_unknown),
        ("sharing unknown", coverage.sharing_unknown),
        ("exact", coverage.exact),
        ("lower bound", coverage.lower),
        ("unknown", coverage.unknown),
    ] {
        assert!(count > 1_000, "{name} covered only {count} times");
    }
    Ok(())
}
