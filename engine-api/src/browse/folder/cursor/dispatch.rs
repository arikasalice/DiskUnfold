//! Shared page dispatch after authenticating and pinning the registered query.
use super::{CursorError, CursorPage, Error, Page, RowKind, Session, validate_count};
use du_core_contracts::{browse_page_ipc::SearchQueryPage, query_page_ipc::QueryPageRequest};

pub(in crate::browse::folder) enum AdmittedQueryPage {
    Folder(CursorPage),
    Search(SearchQueryPage),
}

impl Session {
    /// Binds the cursor and row projection to the same admitted query, not the requested kind.
    pub(in crate::browse::folder) fn read_query_request(
        &self,
        request: QueryPageRequest,
    ) -> Result<AdmittedQueryPage, CursorError> {
        match request {
            QueryPageRequest::Position(input) => {
                validate_count(Some(input.count)).map_err(|_| CursorError::InvalidPage)?;
                let page = self.admit(
                    &input.query_ref,
                    &input.result_version,
                    u64::from(input.start),
                    Some(input.count),
                )?;
                let condition = self.condition(&page)?;
                self.project_query(page, condition, input.start == 0)
            }
            QueryPageRequest::Continuation(input) => {
                validate_count(input.count).map_err(|_| CursorError::InvalidPage)?;
                let fields = self.decode(&input.next_cursor)?;
                let page = self.admit_cursor(&fields, input.count)?;
                self.project_query(page, fields.condition, false)
            }
        }
    }

    fn project_query(
        &self,
        page: Page,
        condition: [u8; 16],
        first_position: bool,
    ) -> Result<AdmittedQueryPage, CursorError> {
        match page.row_kind() {
            RowKind::Folder => self
                .finish_page(page, condition, first_position)
                .map(AdmittedQueryPage::Folder),
            RowKind::Search => {
                let next = u64::try_from(page.next_position())
                    .map_err(|_| CursorError::Folder(Error::Internal))?;
                let next_cursor = page
                    .has_next()
                    .then(|| {
                        self.encode(page.query_ref(), page.metadata().version, condition, next)
                    })
                    .transpose()?;
                crate::search_page::project(&page, next_cursor)
                    .map(AdmittedQueryPage::Search)
                    .map_err(|_| CursorError::Folder(Error::Internal))
            }
        }
    }
}
