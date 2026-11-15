//! The whole-build folder totals must equal one identity table per subtree.
//!
//! The reference is the single-table calculation behind `scope_total`, run on
//! the files below each item. Byte values stay small, so the one documented
//! difference (overflow limitation of a capacity-exceeded subtree) cannot arise.
use super::*;
use crate::space;
use du_core_contracts::result::ResultItemKind;

const FORESTS: u64 = 300;
const COMPLETENESS: [ResultCompleteness; 4] = [
    ResultCompleteness::Complete,
    ResultCompleteness::Partial,
    ResultCompleteness::Missing,
    ResultCompleteness::Cancelled,
];
const LIMITS: [usize; 9] = [0, 1, 2, 3, 5, 9, 40, 1_000_000, usize::MAX];

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
    structures: Vec<bool>,
    observations: Option<Vec<Option<Arc<SpaceObservation>>>>,
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
    let mut structures = Vec::with_capacity(count);
    let mut observations = Vec::with_capacity(count);
    let mut directories: Vec<u32> = Vec::new();
    for number in 0..count {
        let item_no = u32::try_from(number).unwrap_or_default();
        let structure = number == 0 || rng.chance(35);
        let parent =
            (number > 0 && !rng.chance(3)).then(|| directories[rng.below(directories.len())]);
        if structure {
            directories.push(item_no);
        }
        let kind = match (structure, rng.chance(10)) {
            (true, false) => ResultItemKind::Directory,
            (true, true) => ResultItemKind::UnknownStructure,
            (false, _) => ResultItemKind::File,
        };
        items.push(ResultItem {
            item_no,
            parent,
            name: Vec::new(),
            size: None,
            kind,
        });
        structures.push(structure);
        observations.push(if structure {
            None
        } else {
            observation(rng, pool)
        });
    }
    Forest {
        items,
        structures,
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

/// What the single-table calculation yields for every item.
fn reference(
    forest: &Forest,
    completeness: ResultCompleteness,
    limit: usize,
    reversed: bool,
) -> Vec<SpaceAggregate> {
    let mut expected = space::aggregate(&forest.items, &forest.structures, completeness);
    for (index, output) in expected.iter_mut().enumerate() {
        let mut sum = Physical::with_limit(limit);
        let files: Vec<usize> = (0..forest.items.len())
            .filter(|file| !forest.structures[*file] && is_below(&forest.items, index, *file))
            .collect();
        let order: Box<dyn Iterator<Item = &usize>> = if reversed {
            Box::new(files.iter().rev())
        } else {
            Box::new(files.iter())
        };
        for file in order {
            let input = forest
                .observations
                .as_ref()
                .and_then(|values| values[*file].as_deref());
            let _ = sum.observe(input);
        }
        sum.finish(completeness, output);
    }
    expected
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
            let mut actual = space::aggregate(&forest.items, &forest.structures, completeness);
            apply(
                &forest.items,
                &forest.structures,
                forest.observations.as_deref(),
                completeness,
                limit,
                &mut actual,
            );
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
