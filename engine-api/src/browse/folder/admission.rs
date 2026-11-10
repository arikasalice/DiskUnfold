//! Adapts the published input to the current-analysis browse session.

use du_analysis_index::query::{
    Basis, Boundary, Condition, Kind, NameEncoding, NameTest, Predicate,
};
use du_core_contracts::{
    SizeBasis,
    browse_open_ipc::{
        BrowseOpenRequest, BrowseSortKey, BrowseViewKind, QueryCondition, QueryItemType,
        QueryNameEncoding, QueryNameOp, QueryPredicate, QuerySizeBasis, QuerySizeOp,
    },
};

use super::{
    OpenRequest, Sort, View,
    cursor::{CursorError, OpenedPage, Session},
};

/// Failure adapting a published request to the current-analysis source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdmissionError {
    SourceUnresolved,
    InvalidCondition,
    InvalidSizeBasis,
    Browse(CursorError),
}

/// A current-analysis open with the sequence supplied by the caller.
pub struct LiveOpen {
    pub request_seq: u32,
    pub page: OpenedPage,
}

impl Session {
    /// Opens the implicit current-analysis source and binds its continuation cursor.
    /// Explicit source references require the owning result-source resolver.
    pub fn open_live(
        &self,
        request: BrowseOpenRequest,
        cancelled: impl FnMut() -> bool,
    ) -> Result<LiveOpen, AdmissionError> {
        if request.source_ref.is_some() {
            return Err(AdmissionError::SourceUnresolved);
        }
        let condition = request
            .condition
            .map(|node| lower(node, 0))
            .transpose()?
            .unwrap_or(Condition::All(vec![]));
        let mut sorts = request.sort_keys.unwrap_or_default().into_iter().map(sort);
        let primary = sorts.next().transpose()?.unwrap_or_else(|| Sort {
            id: "sort.path".into(),
            revision: 1,
            basis: Basis::Logical,
            descending: false,
        });
        let additional = sorts.collect::<Result<Vec<_>, _>>()?;
        let page = self
            .open(
                OpenRequest {
                    result_version: request.result_version,
                    location: request.location,
                    view: match request.view_kind {
                        BrowseViewKind::List => View::List,
                        BrowseViewKind::TreeBranch => View::Tree,
                    },
                    condition,
                    sort: primary,
                    page_size: request.page_size,
                },
                &additional,
                cancelled,
            )
            .map_err(AdmissionError::Browse)?;
        Ok(LiveOpen {
            request_seq: request.request_seq,
            page,
        })
    }
}

fn sort(key: BrowseSortKey) -> Result<Sort, AdmissionError> {
    let basis = match key.size_basis {
        None | Some(SizeBasis::Logical) => Basis::Logical,
        Some(SizeBasis::Allocated) => Basis::Allocated,
        Some(SizeBasis::UniquePhysical) => return Err(AdmissionError::InvalidSizeBasis),
    };
    Ok(Sort {
        id: key.id,
        revision: key.revision,
        basis,
        descending: key.descending,
    })
}

fn lower(node: QueryCondition, depth: usize) -> Result<Condition, AdmissionError> {
    if depth > 64 {
        return Err(AdmissionError::InvalidCondition);
    }
    let children = |nodes: Vec<_>| {
        nodes
            .into_iter()
            .map(|node| lower(node, depth + 1))
            .collect::<Result<Vec<_>, _>>()
    };
    Ok(match node {
        QueryCondition::All { children: nodes } => Condition::All(children(nodes)?),
        QueryCondition::Any { children: nodes } => Condition::Any(children(nodes)?),
        QueryCondition::Not { child } => Condition::Not(Box::new(lower(*child, depth + 1)?)),
        QueryCondition::Predicate { predicate } => {
            Condition::Predicate(lower_predicate(predicate)?)
        }
    })
}

fn lower_predicate(predicate: QueryPredicate) -> Result<Predicate, AdmissionError> {
    Ok(match predicate {
        QueryPredicate::Name {
            op,
            encoding,
            value,
        } => {
            let bytes = value
                .bytes()
                .map_err(|_| AdmissionError::InvalidCondition)?;
            let test = match op {
                QueryNameOp::Equal => NameTest::Equal(bytes),
                QueryNameOp::Contains => NameTest::Contains(bytes),
                QueryNameOp::StartsWith => NameTest::StartsWith(bytes),
                QueryNameOp::EndsWith => NameTest::EndsWith(bytes),
            };
            let encoding = match encoding {
                QueryNameEncoding::Bytes => NameEncoding::Bytes,
                QueryNameEncoding::WindowsUtf16le => NameEncoding::WindowsUtf16Le,
            };
            Predicate::Name(test, encoding)
        }
        QueryPredicate::Type { value } => Predicate::Kind(match value {
            QueryItemType::File => Kind::File,
            QueryItemType::Directory => Kind::Directory,
            QueryItemType::Link => Kind::Link,
        }),
        QueryPredicate::Size { basis, op, bytes } => {
            let bytes = bytes
                .as_str()
                .parse()
                .map_err(|_| AdmissionError::InvalidCondition)?;
            let bound = match op {
                QuerySizeOp::AtLeast => Boundary::AtLeast(bytes),
                QuerySizeOp::GreaterThan => Boundary::GreaterThan(bytes),
                QuerySizeOp::AtMost => Boundary::AtMost(bytes),
                QuerySizeOp::LessThan => Boundary::LessThan(bytes),
            };
            Predicate::Size(
                match basis {
                    QuerySizeBasis::Logical => Basis::Logical,
                    QuerySizeBasis::Allocated => Basis::Allocated,
                },
                bound,
            )
        }
        QueryPredicate::Registered {
            id,
            revision,
            operand,
        } => Predicate::Registered {
            id,
            revision,
            operand: operand
                .bytes()
                .map_err(|_| AdmissionError::InvalidCondition)?,
        },
    })
}
