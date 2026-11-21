//! #298: the type classes of one folder apply to real results only.
mod folder_support;

use du_analysis_index::{
    IngestSink, SampleSessionKey,
    visualization::folder_types::{FolderTypes, folder_types},
};
use du_core_contracts::{SizeBasis, result::ResultCompleteness};
use folder_support::{dirs, err, forest, id, item};

#[test]
fn a_real_empty_folder_lists_no_class_and_a_sample_folder_has_none() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = forest(&sink, 1, 1)?;
    dirs(&mut build, 0, &[&["a"]])?;
    let real = build
        .seal(id(2), ResultCompleteness::Complete)
        .map_err(err)?;
    let types = |version: &du_analysis_index::ResultVersion| {
        folder_types(
            version,
            &item(version, 0, &["a"])?,
            &SizeBasis::Logical,
            &mut || false,
        )
        .map_err(err)
    };
    // Every file was counted and there is none: an empty list, not an absent one.
    assert_eq!(
        types(&real)?,
        Some(FolderTypes {
            classes: Vec::new(),
            below_unread: false,
        })
    );
    let mut sample = sink
        .open_sample_forest(
            id(3),
            vec![folder_support::root(b"/s")],
            &SampleSessionKey::new(id(9)),
        )
        .map_err(err)?;
    sample.add_directory_at(0, Vec::new()).map_err(err)?;
    dirs(&mut sample, 0, &[&["a"]])?;
    let sample = sample
        .seal(id(4), ResultCompleteness::Complete)
        .map_err(err)?;
    assert_eq!(types(&sample)?, None);
    // The scope root is a folder too, but a file reference is not.
    assert!(
        folder_types(
            &real,
            &item(&real, 0, &[])?,
            &SizeBasis::Logical,
            &mut || false
        )
        .map_err(err)?
        .is_some()
    );
    Ok(())
}

fn allocated_forest() -> Result<std::sync::Arc<du_analysis_index::ResultVersion>, String> {
    use du_analysis_index::{FileIdentity, Observation, SharingEvidence, SpaceObservation};
    use du_core_contracts::{SizeAmount, SizeEvidence, SizeValue};
    let sink = IngestSink::default();
    let mut build = forest(&sink, 5, 1)?;
    dirs(&mut build, 0, &[&["p"], &["q"]])?;
    let allocated = SizeValue {
        basis: SizeBasis::Allocated,
        evidence: SizeEvidence::OsReported,
        amount: SizeAmount::Exact {
            bytes: 4096_u64.into(),
        },
    };
    // p/a.pdf and q/b.pdf are one file; q/c.mp3 is another.
    for (folder, name, file) in [("p", "a.pdf", 7_u8), ("q", "b.pdf", 7), ("q", "c.mp3", 8)] {
        build
            .add_with_space_at(
                0,
                Observation {
                    path: vec![folder.as_bytes().to_vec(), name.as_bytes().to_vec()],
                    size: None,
                },
                SpaceObservation {
                    allocated: Some(allocated.clone()),
                    identity: Some(FileIdentity {
                        volume: vec![1],
                        file: vec![file],
                    }),
                    link_count: Some(if file == 7 { 2 } else { 1 }),
                    sharing: SharingEvidence::None,
                },
            )
            .map_err(err)?;
    }
    build.seal(id(6), ResultCompleteness::Complete).map_err(err)
}

#[test]
fn a_group_counts_a_file_shared_by_two_member_folders_once_as_the_legend_does() -> Result<(), String>
{
    use du_analysis_index::visualization::{folder_types::group_types, type_legend};
    let version = allocated_forest()?;
    let members = [item(&version, 0, &["p"])?, item(&version, 0, &["q"])?];
    let types = group_types(&version, &members, &SizeBasis::Allocated, &mut || false)
        .map_err(err)?
        .ok_or("no types")?;
    let bytes: Vec<_> = types
        .classes
        .iter()
        .map(|(key, total)| (*key, total.count, total.bytes))
        .collect();
    // Two paths are two items, but the shared allocation is added once.
    assert_eq!(bytes, [("type.document", 2, 4096), ("type.audio", 1, 4096)]);
    assert!(!types.below_unread);
    // The legend of the scope that holds both says the same.
    let legend = type_legend::scope_totals(
        &version,
        &item(&version, 0, &[])?,
        &SizeBasis::Allocated,
        &mut || false,
    )
    .map_err(err)?;
    assert_eq!(legend.get("type.document").map(|t| t.bytes), Some(4096));
    Ok(())
}

#[test]
fn an_unread_member_folder_makes_a_groups_counts_lower_bounds() -> Result<(), String> {
    use du_analysis_index::visualization::folder_types::group_types;
    use folder_support::gap;
    let sink = IngestSink::default();
    let mut build = forest(&sink, 7, 1)?;
    dirs(&mut build, 0, &[&["read"], &["locked"]])?;
    gap(&mut build, 0, &["locked"], "scan.path_denied")?;
    let version = build
        .seal(id(8), ResultCompleteness::Missing)
        .map_err(err)?;
    let read = [item(&version, 0, &["read"])?];
    let both = [
        item(&version, 0, &["read"])?,
        item(&version, 0, &["locked"])?,
    ];
    let types = |members: &[du_core_contracts::ItemRef]| {
        group_types(&version, members, &SizeBasis::Logical, &mut || false)
            .map_err(err)?
            .ok_or_else(|| "no types".to_owned())
    };
    assert!(!types(&read)?.below_unread);
    assert!(types(&both)?.below_unread);
    Ok(())
}
