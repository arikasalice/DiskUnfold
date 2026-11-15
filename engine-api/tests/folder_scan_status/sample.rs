//! A source that is not scan lineage keeps the response it had before the scan state.
use super::support::{Check, browse, err, id, parts, root, session};
use du_analysis_index::{IngestSink, Observation, SampleSessionKey};
use du_core_contracts::result::ResultCompleteness;

// Captured from commit 99b8a3ef, before folder scan state was wired.
/// The open response of a sample version, as sent before folder scan state was wired.
const OPEN: &str = r#"{"breadcrumbs":[{"displayName":{"state":"present","value":"/s"},"item":{"itemNo":0,"resultVersion":"02020202-0202-7202-8202-020202020202"}}],"currentCorrespondence":false,"dataSource":"sample","kind":"opened","location":{"itemNo":0,"resultVersion":"02020202-0202-7202-8202-020202020202"},"origin":"scan","page":{"completeness":"complete","nextCursor":"s1.AQIDBAUGcGSAAAAAAAAAZAICAgICAnICggICAgICAgJ9Mn8vfSueFblPTjIEj20NAQAAAAAAAAA.hizPDwDvXy8cyi-yS7cf3fFttWp9prm31Uef7z2RJek","observedAt":null,"queryRef":"01020304-0506-7064-8000-000000000064","resultVersion":"02020202-0202-7202-8202-020202020202","rowKind":"folder","rows":[{"allocatedSize":{"state":"present","value":{"basis":"allocated","bytes":"0","evidence":"measured","state":"exact"}},"correspondence":{"reasonKey":"ref.unavailable","state":"unknown"},"displayName":{"state":"present","value":"d"},"flags":{"excluded":{"reasonKey":"ref.unavailable","state":"unknown"},"unenumerated":{"reasonKey":"ref.unavailable","state":"unknown"}},"hasChildFolder":{"reasonKey":"ref.unavailable","state":"unknown"},"item":{"itemNo":1,"resultVersion":"02020202-0202-7202-8202-020202020202"},"kind":"directory","logicalSize":{"state":"present","value":{"basis":"logical","bytes":"0","evidence":"measured","state":"exact"}},"modifiedAt":{"reasonKey":"ref.unavailable","state":"unknown"}}],"total":{"count":2,"state":"exact"},"unknownCount":0},"predicateStrategies":[],"requestSeq":7,"rootKind":"scope_root","sortStrategies":[{"id":"sort.path","revision":1}]}"#;
/// The first positional page of the same query, as sent before.
const FIRST: &str = r#"{"browseContext":{"breadcrumbs":[{"displayName":{"state":"present","value":"/s"},"item":{"itemNo":0,"resultVersion":"02020202-0202-7202-8202-020202020202"}}],"currentCorrespondence":false,"dataSource":"sample","location":{"itemNo":0,"resultVersion":"02020202-0202-7202-8202-020202020202"},"origin":"scan","predicateStrategies":[],"rootKind":"scope_root","sortStrategies":[{"id":"sort.path","revision":1}]},"completeness":"complete","nextCursor":"s1.AQIDBAUGcGSAAAAAAAAAZAICAgICAnICggICAgICAgJ9Mn8vfSueFblPTjIEj20NAQAAAAAAAAA.hizPDwDvXy8cyi-yS7cf3fFttWp9prm31Uef7z2RJek","observedAt":null,"queryRef":"01020304-0506-7064-8000-000000000064","resultVersion":"02020202-0202-7202-8202-020202020202","rowKind":"folder","rows":[{"allocatedSize":{"state":"present","value":{"basis":"allocated","bytes":"0","evidence":"measured","state":"exact"}},"correspondence":{"reasonKey":"ref.unavailable","state":"unknown"},"displayName":{"state":"present","value":"d"},"flags":{"excluded":{"reasonKey":"ref.unavailable","state":"unknown"},"unenumerated":{"reasonKey":"ref.unavailable","state":"unknown"}},"hasChildFolder":{"reasonKey":"ref.unavailable","state":"unknown"},"item":{"itemNo":1,"resultVersion":"02020202-0202-7202-8202-020202020202"},"kind":"directory","logicalSize":{"state":"present","value":{"basis":"logical","bytes":"0","evidence":"measured","state":"exact"}},"modifiedAt":{"reasonKey":"ref.unavailable","state":"unknown"}}],"total":{"count":2,"state":"exact"},"unknownCount":0}"#;
/// The continuation page of the same query, as sent before.
const NEXT: &str = r#"{"completeness":"complete","nextCursor":null,"observedAt":null,"queryRef":"01020304-0506-7064-8000-000000000064","resultVersion":"02020202-0202-7202-8202-020202020202","rowKind":"folder","rows":[{"allocatedSize":{"reasonKey":"space.data_insufficient","state":"unknown"},"correspondence":{"reasonKey":"ref.unavailable","state":"unknown"},"displayName":{"state":"present","value":"f"},"flags":{"excluded":{"reasonKey":"ref.unavailable","state":"unknown"},"unenumerated":{"reasonKey":"ref.unavailable","state":"unknown"}},"hasChildFolder":{"reasonKey":"ref.unavailable","state":"unknown"},"item":{"itemNo":2,"resultVersion":"02020202-0202-7202-8202-020202020202"},"kind":"file","logicalSize":{"reasonKey":"space.data_insufficient","state":"unknown"},"modifiedAt":{"reasonKey":"ref.unavailable","state":"unknown"}}],"total":{"count":2,"state":"exact"},"unknownCount":0}"#;

#[test]
fn a_sample_version_answers_the_bytes_it_answered_before() -> Check {
    let sink = IngestSink::default();
    let mut build = sink
        .open_sample_forest(id(1), vec![root(b"/s")], &SampleSessionKey::new(id(9)))
        .map_err(err)?;
    build.add_directory_at(0, vec![]).map_err(err)?;
    build.add_directory_at(0, parts(&["d"])).map_err(err)?;
    build.set_excluded_children(0, &[], 2, true).map_err(err)?;
    build
        .add_file(Observation {
            path: parts(&["f"]),
            size: None,
        })
        .map_err(err)?;
    build
        .seal(id(2), ResultCompleteness::Complete)
        .map_err(err)?;
    let wire = browse(&session(sink), Some(id(2)), None, 1)?;
    assert_eq!(wire.continued.len(), 1);
    let sent = [&wire.open, &wire.first, &wire.continued[0]].map(ToString::to_string);
    assert_eq!(sent, [OPEN, FIRST, NEXT]);
    Ok(())
}
