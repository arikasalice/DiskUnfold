//! The opened folder carries the excluded entries directly below it once the result
//! can tell them; until #217 it cannot, so the count is omitted everywhere.
use super::support::{Check, browse, err, id, item, parts, root, session};
use du_analysis_index::IngestSink;
use du_core_contracts::result::{ResultCompleteness, ResultVersionRead};
use du_engine_api::browse::folder::cursor::CursorError;

#[test]
fn a_finished_scan_omits_the_count_it_cannot_prove() -> Check {
    let sink = IngestSink::default();
    let mut build = sink.open_forest(id(1), vec![root(b"/r")]).map_err(err)?;
    build.add_directory_at(0, vec![]).map_err(err)?;
    for name in ["denied", "marked", "part", "plain"] {
        build.add_directory_at(0, parts(&[name])).map_err(err)?;
    }
    build.set_excluded_children(0, &[], 2, true).map_err(err)?;
    build
        .set_excluded_children(0, &parts(&["part"]), 3, false)
        .map_err(err)?;
    build
        .set_excluded_children(0, &parts(&["marked"]), 4, true)
        .map_err(err)?;
    for name in ["denied", "marked"] {
        build
            .record_gap_at(0, parts(&[name]), "scan.path_denied".into(), None)
            .map_err(err)?;
    }
    let version = build
        .seal(id(2), ResultCompleteness::Missing)
        .map_err(err)?;
    let browser = session(sink);
    let top = browse(&browser, None, None, 1)?;
    top.check()?;
    assert_eq!(top.open["rootKind"], "scope_root");
    assert_eq!(top.excluded(), None);
    for name in ["plain", "part", "denied", "marked"] {
        let opened = browse(&browser, None, Some(item(&version, &[name])?), 1)?;
        opened.check()?;
        assert_eq!(opened.excluded(), None, "{name}");
    }
    Ok(())
}

#[test]
fn an_unfinished_scan_omits_the_count_and_its_pinned_query_never_reads_the_sealed_one() -> Check {
    let sink = IngestSink::default();
    let mut build = sink.open_forest(id(1), vec![root(b"/r")]).map_err(err)?;
    build.add_directory_at(0, vec![]).map_err(err)?;
    build.add_directory_at(0, parts(&["a"])).map_err(err)?;
    let partial = build.publish_partial(id(3)).map_err(err)?;
    let browser = session(sink);
    let running = browse(&browser, None, None, 1)?;
    running.check()?;
    assert_eq!(running.excluded(), None);
    let below = browse(&browser, None, Some(item(&partial, &["a"])?), 1)?;
    below.check()?;
    assert_eq!(below.excluded(), None);
    let query = serde_json::from_value(running.open["page"]["queryRef"].clone()).map_err(err)?;
    build
        .seal(id(4), ResultCompleteness::Complete)
        .map_err(err)?;
    // The pinned query never mixes in the sealed version's answer.
    assert!(matches!(
        browser.page_at(&query, &id(3), 0, Some(1)),
        Err(CursorError::Stale)
    ));
    let sealed = browse(&browser, None, None, 1)?;
    sealed.check()?;
    assert_eq!(sealed.excluded(), None);
    Ok(())
}

#[test]
fn the_virtual_top_of_several_roots_has_no_count() -> Check {
    let sink = IngestSink::default();
    let mut build = sink
        .open_forest(id(1), vec![root(b"/a"), root(b"/b")])
        .map_err(err)?;
    for index in 0..2 {
        build.add_directory_at(index, vec![]).map_err(err)?;
        build
            .set_excluded_children(index, &[], 1, true)
            .map_err(err)?;
    }
    let version = build
        .seal(id(2), ResultCompleteness::Complete)
        .map_err(err)?;
    let browser = session(sink);
    let top = browse(&browser, None, None, 1)?;
    top.check()?;
    assert_eq!(top.open["rootKind"], "virtual_root");
    assert_eq!(top.first["browseContext"]["rootKind"], "virtual_root");
    assert_eq!(top.excluded(), None);
    let first_root =
        serde_json::from_value(top.open["page"]["rows"][0]["item"].clone()).map_err(err)?;
    let opened = browse(
        &browser,
        Some(version.version_id().clone()),
        Some(first_root),
        1,
    )?;
    opened.check()?;
    assert_eq!(opened.open["rootKind"], "scope_root");
    assert_eq!(opened.excluded(), None);
    Ok(())
}
