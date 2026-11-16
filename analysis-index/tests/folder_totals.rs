mod folder_support;

use du_analysis_index::{
    FileIdentity, IngestSink, Observation, ResultVersion, SharingEvidence, SpaceObservation,
};
use du_core_contracts::{
    ByteSize, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    result::{ResultCompleteness, ResultVersionRead},
};
use folder_support::{OTHER_VOLUME, REVISIT, dirs, err, folder_path, forest, gap, id, unentered};

fn size(bytes: u64, basis: SizeBasis, evidence: SizeEvidence) -> SizeValue {
    SizeValue {
        basis,
        evidence,
        amount: SizeAmount::Exact {
            bytes: ByteSize::from(bytes),
        },
    }
}

/// Two roots; root 0 holds a file in `kept` and the folders the scenarios mark.
fn scenario(variant: Variant) -> Result<std::sync::Arc<ResultVersion>, String> {
    let sink = IngestSink::default();
    let mut build = forest(&sink, 1, 2)?;
    dirs(
        &mut build,
        0,
        &[&["kept"], &["mnt"], &["again"], &["excl"], &["denied"]],
    )?;
    for (root, number) in [(0, 1u8), (1, 2)] {
        let physical = SpaceObservation {
            allocated: Some(size(16, SizeBasis::Allocated, SizeEvidence::OsReported)),
            identity: Some(FileIdentity {
                volume: vec![number],
                file: vec![number],
            }),
            link_count: Some(1),
            sharing: SharingEvidence::None,
        };
        let observation = Observation {
            path: folder_path(&if root == 0 {
                vec!["kept", "f"]
            } else {
                vec!["f"]
            }),
            size: Some(size(10, SizeBasis::Logical, SizeEvidence::Measured)),
        };
        build
            .add_with_space_at(root, observation, physical)
            .map_err(err)?;
    }
    if variant != Variant::Plain {
        unentered(&mut build, 0, &["mnt"], OTHER_VOLUME)?;
        unentered(&mut build, 0, &["again"], REVISIT)?;
        build
            .set_excluded_children(0, &folder_path(&["excl"]), 3, true)
            .map_err(err)?;
        build.set_excluded_children(0, &[], 1, true).map_err(err)?;
    }
    if variant == Variant::Gap {
        gap(&mut build, 0, &["denied"], "scan.path_denied")?;
    }
    build.seal(id(9), ResultCompleteness::Complete).map_err(err)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Variant {
    Plain,
    Boundaries,
    Gap,
}

fn totals(version: &ResultVersion) -> Vec<du_core_contracts::result::SpaceAggregate> {
    let mut all: Vec<_> = (0..version.items().len() as u32)
        .filter_map(|item_no| version.space(item_no).cloned())
        .collect();
    all.push(version.scope_total().clone());
    all
}

#[test]
fn boundaries_and_exclusions_leave_every_total_unchanged() -> Result<(), String> {
    let plain = scenario(Variant::Plain)?;
    let marked = scenario(Variant::Boundaries)?;
    assert_eq!(totals(&plain), totals(&marked));
    for aggregate in totals(&marked) {
        assert_eq!(aggregate.inaccessible_ranges, None);
        assert_eq!(aggregate.completeness, ResultCompleteness::Complete);
    }
    let root = marked.scope_roots()[0].item_no;
    let aggregate = marked.space(root).ok_or("missing aggregate")?;
    assert_eq!(
        aggregate.logical.amount,
        SizeAmount::Exact {
            bytes: ByteSize::from(10)
        }
    );
    assert_eq!(
        aggregate.allocated.amount,
        SizeAmount::Exact {
            bytes: ByteSize::from(16)
        }
    );
    assert_eq!(
        marked.scope_total().logical.amount,
        SizeAmount::Exact {
            bytes: ByteSize::from(20)
        }
    );
    Ok(())
}

#[test]
fn a_gap_makes_ancestors_and_the_scope_lower_bounds() -> Result<(), String> {
    let marked = scenario(Variant::Boundaries)?;
    let gapped = scenario(Variant::Gap)?;
    let root = gapped.scope_roots()[0].item_no;
    let aggregate = gapped.space(root).ok_or("missing aggregate")?;
    assert_eq!(aggregate.inaccessible_ranges, Some(1));
    assert_eq!(
        aggregate.logical.amount,
        SizeAmount::LowerBound {
            bytes: ByteSize::from(10)
        }
    );
    assert_eq!(
        aggregate.allocated.amount,
        SizeAmount::LowerBound {
            bytes: ByteSize::from(16)
        }
    );
    assert_eq!(aggregate.completeness, ResultCompleteness::Missing);
    let scope = gapped.scope_total();
    assert_eq!(scope.inaccessible_ranges, Some(1));
    assert_eq!(
        scope.logical.amount,
        SizeAmount::LowerBound {
            bytes: ByteSize::from(20)
        }
    );
    // The second root and the other folders keep their totals.
    assert_eq!(
        gapped.space(gapped.scope_roots()[1].item_no),
        marked.space(marked.scope_roots()[1].item_no)
    );
    assert_eq!(
        gapped.space(item_no(&gapped, "kept")?),
        marked.space(item_no(&marked, "kept")?)
    );
    assert_eq!(
        gapped.space(item_no(&gapped, "mnt")?),
        marked.space(item_no(&marked, "mnt")?)
    );
    Ok(())
}

fn item_no(version: &ResultVersion, name: &str) -> Result<u32, String> {
    Ok(folder_support::item(version, 0, &[name])?.item_no)
}
