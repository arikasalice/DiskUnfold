//! Projection of an already-admitted search page into the GUI row contract.

use du_analysis_index::{
    QueryError, ResultVersion,
    query::page::{Page, RowKind, Total},
};
use du_core_contracts::{
    ItemRef, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    browse_ipc::BrowseValue,
    browse_page_ipc::{QueryPage, QueryTotal, SearchQueryPage, SearchQueryRow, SearchRowKind},
};

use crate::browse::display;

/// The admitted page cannot be projected as a search page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectionError {
    /// The producer registered a folder page, not a search page.
    NotSearch,
    /// The pinned page refers to an unavailable or inconsistent item.
    Item(QueryError),
    /// A count does not fit the published page representation.
    CountOverflow,
}

/// Projects one pinned search page without reading a live store or filesystem.
///
/// The cursor must be produced for this exact admission by the session reader.
pub fn project(
    page: &Page,
    next_cursor: Option<String>,
) -> Result<SearchQueryPage, ProjectionError> {
    if page.row_kind() != RowKind::Search {
        return Err(ProjectionError::NotSearch);
    }
    let metadata = page.metadata();
    let total = match metadata.total {
        Total::Exact(count) => QueryTotal::Exact {
            count: to_count(count)?,
        },
        Total::LowerBound(count) => QueryTotal::LowerBound {
            count: to_count(count)?,
        },
    };
    let rows = page
        .items()
        .iter()
        .map(|item| row(page.version(), item))
        .collect::<Result<_, _>>()?;
    Ok(SearchQueryPage {
        row_kind: SearchRowKind::Search,
        page: QueryPage {
            query_ref: page.query_ref().clone(),
            result_version: metadata.version.clone(),
            rows,
            total,
            unknown_count: to_count(metadata.unknown_count)?,
            observed_at: metadata.observed_at.cloned(),
            completeness: metadata.completeness,
            next_cursor,
        },
    })
}

fn row(version: &ResultVersion, reference: &ItemRef) -> Result<SearchQueryRow, ProjectionError> {
    let item = version
        .detail(reference)
        .map_err(ProjectionError::Item)?
        .item;
    let path = version.native_path(reference).ok().and_then(|path| {
        path.raw
            .bytes()
            .ok()
            .and_then(|bytes| display(&path.encoding, &bytes))
    });
    let name = if item.parent.is_none() {
        path.clone()
    } else {
        version
            .observed_root_for(reference)
            .ok()
            .and_then(|(_, root)| display(&root.path.encoding, &item.name))
    };
    Ok(SearchQueryRow {
        item: reference.clone(),
        display_name: field(name, "ref.unavailable"),
        display_path: field(path, "ref.unavailable"),
        kind: item.kind,
        logical_size: size(version.item_size(item.item_no, SizeBasis::Logical)),
        allocated_size: size(version.item_size(item.item_no, SizeBasis::Allocated)),
        modified_at: field(
            version.item_modified_at(item.item_no).cloned(),
            "ref.unavailable",
        ),
        created_at: unknown("ref.unavailable"),
        // The current result does not retain per-row missing-field reports or their causes.
        missing_fields: Vec::new(),
        reason_keys: Vec::new(),
    })
}

fn to_count(value: u64) -> Result<u32, ProjectionError> {
    u32::try_from(value).map_err(|_| ProjectionError::CountOverflow)
}

fn field<T>(value: Option<T>, reason: &str) -> BrowseValue<T> {
    value.map_or_else(|| unknown(reason), |value| BrowseValue::Present { value })
}

fn unknown<T>(reason: &str) -> BrowseValue<T> {
    BrowseValue::Unknown {
        reason_key: reason.into(),
    }
}

fn size(value: Option<&SizeValue>) -> BrowseValue<SizeValue> {
    field(
        value
            .filter(|value| {
                value.amount != SizeAmount::Unknown && value.evidence != SizeEvidence::Unknown
            })
            .cloned(),
        "space.data_insufficient",
    )
}
