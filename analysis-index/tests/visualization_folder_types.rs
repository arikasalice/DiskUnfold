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
