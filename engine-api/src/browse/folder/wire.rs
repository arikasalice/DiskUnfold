//! Projects admitted folder observations into the published GUI response bodies.

use du_analysis_index::query::page::Total;
use du_core_contracts::browse_page_ipc::{
    BrowseContext, BrowseOpenResponse, FolderFlags, FolderQueryPage, FolderQueryRow, FolderRowKind,
    QueryPage, QueryPageResponse, QueryTotal, ResultOrigin,
};

use super::{FolderPage, LiveOpen, Row, cursor::CursorPage};
use crate::{
    browse::{field, size, unknown},
    error::EngineError,
};

impl LiveOpen {
    /// Converts the first page using the origin of the source pinned at admission.
    pub fn into_response(self, origin: ResultOrigin) -> Result<BrowseOpenResponse, EngineError> {
        let opened = self.page.opened;
        let BrowseContext {
            origin,
            location,
            breadcrumbs,
            root_kind,
            current_correspondence,
            sort_strategies,
            predicate_strategies,
        } = super::cursor::context::project_context(
            origin,
            opened.location,
            opened.breadcrumbs,
            &opened.sort_strategies,
            &opened.predicate_strategies,
        )
        .map_err(|_| internal())?;
        let page = folder_page(opened.page, self.page.next_cursor)?;
        Ok(BrowseOpenResponse::Opened {
            request_seq: self.request_seq,
            origin,
            location,
            breadcrumbs,
            root_kind,
            current_correspondence,
            page: Box::new(page),
            sort_strategies,
            predicate_strategies,
        })
    }
}

impl CursorPage {
    /// Converts a subsequent page using the same folder-row projection as open.
    pub fn into_response(self) -> Result<QueryPageResponse, EngineError> {
        let mut page = folder_page(self.page, self.next_cursor)?;
        page.browse_context = self.browse_context;
        Ok(QueryPageResponse::Folder(page))
    }
}

fn folder_page(
    page: FolderPage,
    next_cursor: Option<String>,
) -> Result<FolderQueryPage, EngineError> {
    let total = match page.total {
        Total::Exact(value) => QueryTotal::Exact {
            count: count(value)?,
        },
        Total::LowerBound(value) => QueryTotal::LowerBound {
            count: count(value)?,
        },
    };
    let unknown_count = count(page.unknown_count)?;
    Ok(FolderQueryPage {
        row_kind: FolderRowKind::Folder,
        browse_context: None,
        page: QueryPage {
            query_ref: page.query_ref,
            result_version: page.result_version,
            rows: page.rows.into_iter().map(row).collect(),
            total,
            unknown_count,
            observed_at: page.observed_at,
            completeness: page.completeness,
            next_cursor,
        },
    })
}

fn row(value: Row) -> FolderQueryRow {
    FolderQueryRow {
        item: value.item,
        display_name: field(value.display_name, "ref.unavailable"),
        kind: value.observation.kind,
        logical_size: size(value.logical_size.as_ref()),
        allocated_size: size(value.allocated_size.as_ref()),
        modified_at: field(value.modified_at, "ref.unavailable"),
        has_child_folder: field(value.has_observed_child_folder, "ref.unavailable"),
        // Exact gap reports alone cannot establish either of these independent facts.
        flags: FolderFlags {
            unenumerated: unknown("ref.unavailable"),
            excluded: unknown("ref.unavailable"),
        },
        correspondence: unknown("ref.unavailable"),
    }
}

fn count(value: u64) -> Result<u32, EngineError> {
    u32::try_from(value).map_err(|_| internal())
}

fn internal() -> EngineError {
    EngineError::reason("support.internal_error")
}

#[cfg(test)]
mod tests;
