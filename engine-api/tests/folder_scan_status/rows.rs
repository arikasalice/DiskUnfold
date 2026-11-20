//! Row flags follow the per-folder scan state of the pinned version.
use super::support::{Check, absent, browse, err, id, parts, present, root, session, unknown};
use du_analysis_index::{
    BuildHandle, IngestSink, Observation,
    folder_collection::{
        BOUNDARY_ALREADY_COUNTED, BOUNDARY_OTHER_VOLUME, REASON_DATA_INSUFFICIENT,
        REASON_IN_PROGRESS, UnreadReason,
    },
};
use du_core_contracts::{
    browse_page_ipc::{FolderBoundary, FolderFlags},
    result::ResultCompleteness,
    scan_failures::{ScanFailureKind, ScanFailureStage, ScanIssue, unentered_reason},
};
use serde_json::{Value, json};

const IN_PROGRESS: &str = FolderFlags::REASON_SCAN_IN_PROGRESS;
const INSUFFICIENT: &str = FolderFlags::REASON_DATA_INSUFFICIENT;

fn folder(unenumerated: Value, boundary: Value) -> Value {
    json!({"unenumerated": unenumerated, "excluded": present(false), "boundary": boundary})
}

fn file() -> Value {
    json!({"unenumerated": absent(), "excluded": present(false)})
}

fn unentered(build: &mut BuildHandle, name: &str, reason: &str) -> Check {
    build
        .record_scan_issue(ScanIssue {
            root_index: 0,
            relative: parts(&[name]),
            kind: ScanFailureKind::UnreadRange,
            stage: ScanFailureStage::Traverse,
            reason_key: reason.into(),
            observed_at: None,
        })
        .map_err(err)
}

fn tree(sink: &IngestSink, names: &[&str]) -> Result<BuildHandle, String> {
    let mut build = sink.open_forest(id(1), vec![root(b"/r")]).map_err(err)?;
    build.add_directory_at(0, vec![]).map_err(err)?;
    for name in names {
        build.add_directory_at(0, parts(&[name])).map_err(err)?;
    }
    build
        .add_file(Observation {
            path: parts(&["f"]),
            size: None,
        })
        .map_err(err)?;
    Ok(build)
}

#[test]
fn the_reason_keys_and_boundary_names_are_the_published_ones() {
    assert_eq!(REASON_IN_PROGRESS, FolderFlags::REASON_SCAN_IN_PROGRESS);
    assert_eq!(
        REASON_DATA_INSUFFICIENT,
        FolderFlags::REASON_DATA_INSUFFICIENT
    );
    assert_eq!(BOUNDARY_OTHER_VOLUME, FolderBoundary::OTHER_VOLUME);
    assert_eq!(BOUNDARY_ALREADY_COUNTED, FolderBoundary::ALREADY_COUNTED);
}

#[test]
fn a_finished_scan_reports_read_unread_and_bounded_folders() -> Check {
    let sink = IngestSink::default();
    let mut build = tree(&sink, &["denied", "keep", "loop", "mnt"])?;
    build
        .record_gap_at(0, parts(&["denied"]), "scan.path_denied".into(), None)
        .map_err(err)?;
    unentered(&mut build, "loop", unentered_reason::REVISIT_BLOCKED)?;
    unentered(&mut build, "mnt", unentered_reason::BOUNDARY_OTHER_VOLUME)?;
    build
        .seal(id(2), ResultCompleteness::Missing)
        .map_err(err)?;
    // Two rows a page: the open, two continuation pages and the first position page.
    let wire = browse(&session(sink), None, None, 2)?;
    wire.check()?;
    assert_eq!(wire.continued.len(), 2);
    let flags = |name| wire.row(name).map(|row| row["flags"].clone());
    // A boundary is proven only by a mark; a folder read normally stays unproven.
    assert_eq!(
        flags("denied")?,
        folder(present(true), unknown(INSUFFICIENT))
    );
    assert_eq!(
        flags("keep")?,
        folder(unknown(INSUFFICIENT), unknown(INSUFFICIENT))
    );
    assert_eq!(
        flags("loop")?,
        folder(present(false), present("already_counted"))
    );
    assert_eq!(
        flags("mnt")?,
        folder(present(false), present("other_volume"))
    );
    assert_eq!(flags("f")?, file());
    assert!(!wire.text().contains("scan.path_denied"));
    Ok(())
}

#[test]
fn an_unfinished_scan_is_undecided_and_a_cancelled_one_reports_its_marks() -> Check {
    let sink = IngestSink::default();
    let mut build = tree(&sink, &["a", "c"])?;
    build.publish_partial(id(3)).map_err(err)?;
    let browser = session(sink);
    let running = browse(&browser, None, None, 1)?;
    running.check()?;
    let undecided = folder(unknown(IN_PROGRESS), unknown(IN_PROGRESS));
    assert_eq!(running.row("a")?["flags"], undecided);
    assert_eq!(running.row("c")?["flags"], undecided);
    assert_eq!(running.row("f")?["flags"], file());
    build
        .set_directory_unread(0, &parts(&["c"]), UnreadReason::Cancelled)
        .map_err(err)?;
    build
        .seal(id(4), ResultCompleteness::Cancelled)
        .map_err(err)?;
    let cancelled = browse(&browser, None, None, 1)?;
    cancelled.check()?;
    assert_eq!(
        cancelled.row("a")?["flags"],
        folder(unknown(INSUFFICIENT), unknown(INSUFFICIENT))
    );
    assert_eq!(
        cancelled.row("c")?["flags"],
        folder(present(true), unknown(INSUFFICIENT))
    );
    assert_eq!(cancelled.row("f")?["flags"], file());
    Ok(())
}

#[test]
fn a_disconnected_folder_is_unenumerated_without_its_reason() -> Check {
    let sink = IngestSink::default();
    let mut build = tree(&sink, &["gone"])?;
    build
        .set_directory_unread(0, &parts(&["gone"]), UnreadReason::Disconnected)
        .map_err(err)?;
    build
        .seal(id(2), ResultCompleteness::Missing)
        .map_err(err)?;
    let wire = browse(&session(sink), None, None, 5)?;
    wire.check()?;
    assert_eq!(
        wire.row("gone")?["flags"],
        folder(present(true), unknown(INSUFFICIENT))
    );
    Ok(())
}

#[test]
fn a_complete_scan_proves_a_folder_without_marks_has_no_boundary() -> Check {
    let sink = IngestSink::default();
    let build = tree(&sink, &["keep"])?;
    build
        .seal(id(2), ResultCompleteness::Complete)
        .map_err(err)?;
    let wire = browse(&session(sink), None, None, 5)?;
    wire.check()?;
    // F03 #219: a Complete seal proves every folder without a mark was read.
    assert_eq!(wire.row("keep")?["flags"], folder(present(false), absent()));
    assert_eq!(wire.row("f")?["flags"], file());
    Ok(())
}
