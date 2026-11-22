//! Child evidence collection for one expanded folder.
use super::ChildPolicyEvidence;
use crate::{current_analysis::CurrentAnalysis, viz_pages::render};
use du_analysis_index::visualization::projection;
use du_core_contracts::{ItemRef, result::ResultItem};

pub(super) fn collect_evidence(
    source: &CurrentAnalysis,
    parent: &ItemRef,
    mut evidence: impl FnMut(
        &CurrentAnalysis,
        &ResultItem,
        &mut dyn FnMut() -> bool,
    ) -> Result<ChildPolicyEvidence, render::Error>,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<Vec<render::ChildEvidence>, render::Error> {
    let folder = du_analysis_index::browse::open(source.version(), Some(parent))
        .map_err(|_| render::Error::Projection(projection::Error::InvalidSource))?;
    let mut children = Vec::with_capacity(folder.children.len());
    for item in folder.children {
        if cancelled() {
            return Err(render::Error::Cancelled);
        }
        let policy = evidence(source, item, cancelled)?;
        children.push(render::ChildEvidence {
            item: ItemRef {
                result_version: source.result_version().clone(),
                item_no: item.item_no,
            },
            path_compare_key: policy.path_compare_key,
            legend_class_key: policy.legend_class_key,
            display_flags: policy.display_flags,
        });
    }
    if cancelled() {
        return Err(render::Error::Cancelled);
    }
    Ok(children)
}
