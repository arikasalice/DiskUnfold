//! The one condition viz.render serves: a type legend fragment (F06 FR14).
//!
//! A legend row of the type criterion carries the fragment that keeps its files;
//! the screen sends it back unread. The fragment and the request filter are made
//! and read here only, so a row narrows to exactly the files counted in it.
use super::*;
use du_analysis_index::{
    query::file_type_predicate,
    visualization::{filter::ItemFilter, type_legend},
};
use du_core_contracts::{
    PathBytes,
    browse_open_ipc::{QueryCondition, QueryPredicate},
    visualization::request::VizCondition,
};

/// Formats whose shapes stand for folders and files, which a type filter narrows.
const FILTERED_FORMATS: [&str; 2] = ["treemap", "bar"];

/// The condition fragment of a legend class: files of its type. The class of no
/// type and the folder class have none.
pub(crate) fn fragment(class_key: &str) -> Option<QueryCondition> {
    type_legend::type_of_key(class_key.as_bytes())?;
    Some(QueryCondition::Predicate {
        predicate: QueryPredicate::Registered {
            id: file_type_predicate::ID.to_owned(),
            revision: file_type_predicate::REVISION,
            operand: PathBytes::from_bytes(class_key.as_bytes()),
        },
    })
}

/// The filter of a request's condition.
///
/// A format that does not narrow refuses any condition with predicate.not_allowed,
/// as does a condition other than one type fragment. A type fragment whose operand
/// is not a listed type key is query.syntax_invalid.
pub(super) fn filter_of(request: &VizRenderRequest) -> Result<Option<ItemFilter>, EngineError> {
    let Some(condition) = &request.condition else {
        return Ok(None);
    };
    if !FILTERED_FORMATS.contains(&request.format.as_str()) {
        return Err(not_allowed());
    }
    let VizCondition::Normalized {
        condition:
            QueryCondition::Predicate {
                predicate:
                    QueryPredicate::Registered {
                        id,
                        revision,
                        operand,
                    },
            },
    } = condition
    else {
        return Err(not_allowed());
    };
    if id != file_type_predicate::ID || *revision != file_type_predicate::REVISION {
        return Err(not_allowed());
    }
    let operand = operand
        .bytes()
        .map_err(|_| EngineError::reason("query.syntax_invalid"))?;
    type_legend::type_of_key(&operand)
        .map(|kind| Some(ItemFilter::FileType(kind)))
        .ok_or_else(|| EngineError::reason("query.syntax_invalid"))
}

pub(super) fn not_allowed() -> EngineError {
    EngineError::reason("predicate.not_allowed")
}

/// The type key a request narrows to, when its condition is a type fragment a
/// narrowing format accepts; none otherwise, also for a condition it refuses.
pub(crate) fn narrowed_type_key(request: &VizRenderRequest) -> Option<&'static str> {
    match filter_of(request) {
        Ok(Some(ItemFilter::FileType(kind))) => Some(type_legend::key_of(kind)),
        _ => None,
    }
}
