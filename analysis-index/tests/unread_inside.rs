//! #226: unread ranges recorded inside a folder that was itself read.
mod folder_support;

use du_analysis_index::{
    IngestSink,
    folder_collection::{UnreadInside, UnreadReason},
};
use du_core_contracts::result::ResultCompleteness;
use folder_support::{dirs, err, folder_path, forest, gap, id, item};

#[test]
fn a_read_folder_names_the_unread_ranges_below_it() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = forest(&sink, 1, 1)?;
    dirs(
        &mut build,
        0,
        &[
            &["a"],
            &["a", "denied"],
            &["a", "other"],
            &["c"],
            &["c", "d"],
            &["plain"],
        ],
    )?;
    gap(&mut build, 0, &["a", "denied"], "scan.path_denied")?;
    gap(&mut build, 0, &["a", "other"], "scan.path_denied")?;
    build
        .set_directory_unread(0, &folder_path(&["c", "d"]), UnreadReason::Cancelled)
        .map_err(err)?;
    let interim = build.publish_partial(id(2)).map_err(err)?;
    let version = build
        .seal(id(3), ResultCompleteness::Missing)
        .map_err(err)?;
    let inside = |parts: &[&str]| -> Result<Option<UnreadInside>, String> {
        version
            .unread_inside(&item(&version, 0, parts)?)
            .map_err(err)
    };
    // Two gaps below "a".
    assert_eq!(
        inside(&["a"])?,
        Some(UnreadInside::Recorded { count: Some(2) })
    );
    // A folder that was not read itself shows that instead.
    assert_eq!(inside(&["a", "denied"])?, None);
    assert_eq!(inside(&["c", "d"])?, None);
    // A cancelled folder below is not a counted range, so the number is not known.
    assert_eq!(
        inside(&["c"])?,
        Some(UnreadInside::Recorded { count: None })
    );
    assert_eq!(inside(&[])?, Some(UnreadInside::Recorded { count: None }));
    assert_eq!(inside(&["plain"])?, Some(UnreadInside::NoneRecorded));
    // While the scan runs the fact is not decided yet.
    assert!(matches!(
        interim
            .unread_inside(&item(&interim, 0, &["plain"])?)
            .map_err(err)?,
        Some(UnreadInside::Unknown { .. })
    ));
    Ok(())
}
