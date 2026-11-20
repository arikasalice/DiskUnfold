//! #226: a folder whose own attempts ended lists its direct children with an exact count,
//! even in a version that is not complete.
use super::*;
use crate::query::{self, page::ResultSets, page::Total};
use du_core_contracts::ItemRef;
use std::sync::Arc;

/// The header total of the listing of folder `name` (empty for the scope root).
fn listing_total(
    sink: &IngestSink,
    version: &Arc<ResultVersion>,
    name: &str,
) -> Result<Total, String> {
    let item_no = if name.is_empty() {
        0
    } else {
        version
            .items
            .iter()
            .find(|item| item.name == name.as_bytes())
            .ok_or("missing folder")?
            .item_no
    };
    let location = ItemRef {
        result_version: version.version_id.clone(),
        item_no,
    };
    let listing = query::run_folder(
        &query::Registry::baseline(),
        Arc::clone(version),
        Some(&location),
        &query::Condition::All(vec![]),
        query::Sort {
            id: "sort.size",
            revision: 1,
            basis: query::Basis::Logical,
            descending: true,
        },
        || false,
    )
    .map_err(e)?;
    let store = ResultSets::new(sink.clone());
    store.register(id(9)?, listing).map_err(e)?;
    let page = store
        .open_page(&id(9)?, &version.version_id, 0, None)
        .map_err(e)?;
    Ok(page.metadata().total)
}

#[test]
fn a_folder_whose_attempts_ended_has_an_exact_item_count() -> Test {
    // The root lists "a" and "sub"; "sub" lists "b".
    for (case, root, sub) in [
        (Case::AllEnded, Total::Exact(2), Total::Exact(1)),
        // The subfolder's attempt did not end: its listing stays a lower bound, while
        // the root, whose own attempt ended, is exact.
        (Case::SubPending, Total::Exact(2), Total::LowerBound(1)),
        // The root's end was rejected: its listing stays a lower bound.
        (
            Case::ParentSkipsSubTerminal,
            Total::LowerBound(2),
            Total::Exact(1),
        ),
    ] {
        let sink = IngestSink::default();
        let build = tree(&sink, case)?;
        let version = build.seal(id(2)?, ResultCompleteness::Missing).map_err(e)?;
        assert_eq!(listing_total(&sink, &version, "")?, root);
        assert_eq!(listing_total(&sink, &version, "sub")?, sub);
    }
    Ok(())
}
