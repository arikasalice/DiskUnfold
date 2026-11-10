//! Project location from the already admitted immutable page.
use crate::browse::field;
use du_analysis_index::query::{FolderLocation, page::Page};
use du_core_contracts::{
    browse_page_ipc::{
        BrowseBreadcrumb, BrowseContext, BrowseRootKind, QueryStrategy, ResultOrigin,
    },
    result::ResultVersionRead,
};

pub(super) fn page_context(page: &Page) -> Result<Option<BrowseContext>, super::super::Error> {
    let Some(location) = page.folder_location() else {
        return Ok(None);
    };
    let version = page.version();
    let requested = match location {
        FolderLocation::ScopeRoots => None,
        FolderLocation::Item(item_no) => Some(du_core_contracts::ItemRef {
            result_version: version.version_id().clone(),
            item_no,
        }),
    };
    let (location, breadcrumbs, _) = if requested.is_some() {
        super::super::open_location(version, requested.as_ref())?
    } else {
        (None, Vec::new(), 0)
    };
    let root_kind = match (location.is_some(), breadcrumbs.len()) {
        (false, 0) => BrowseRootKind::VirtualRoot,
        (true, 1) => BrowseRootKind::ScopeRoot,
        (true, 2..) => BrowseRootKind::Folder,
        _ => return Err(super::super::Error::Internal),
    };
    let metadata = page.metadata();
    Ok(Some(BrowseContext {
        origin: ResultOrigin("scan".into()),
        location,
        breadcrumbs: breadcrumbs
            .into_iter()
            .map(|crumb| BrowseBreadcrumb {
                item: crumb.item,
                display_name: field(crumb.display_name, "ref.unavailable"),
            })
            .collect(),
        root_kind,
        current_correspondence: false,
        sort_strategies: metadata
            .sorts
            .iter()
            .map(|s| QueryStrategy {
                id: s.id.clone(),
                revision: s.revision,
            })
            .collect(),
        predicate_strategies: metadata
            .predicate_strategies
            .iter()
            .map(|(id, revision)| QueryStrategy {
                id: id.clone(),
                revision: *revision,
            })
            .collect(),
    }))
}
