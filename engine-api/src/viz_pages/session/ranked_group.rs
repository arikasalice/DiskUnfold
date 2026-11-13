//! Session access to source-verified ranked group publication.
use super::*;

impl Session {
    /// Computes a registered ranked strategy and retains its exact source allocation.
    pub fn compute_pinned_ranked_bar(
        &self,
        source: &Arc<ResultVersion>,
        request: du_analysis_index::visualization::bar_projection::ChildrenRequest<'_>,
        descriptor: &du_core_contracts::visualization::LayoutDescriptor,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<
        crate::viz_pages::ranked_group::PinnedRankedBar,
        du_analysis_index::visualization::typed_layout::BarDispatchError,
    > {
        let selected = self.compute_ranked_bar(source, request, descriptor, cancelled)?;
        Ok(crate::viz_pages::ranked_group::PinnedRankedBar {
            source: source.clone(),
            selected,
            owner: Arc::downgrade(&self.pages.identity),
        })
    }

    /// Reserves verified ranked geometry, model rows and Other members together.
    /// The caller must supply the result returned by this session's strategy execution.
    pub fn defer_ranked_group(
        &self,
        request: crate::viz_pages::ranked_group::Request<'_>,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<crate::viz_pages::ranked_group::DeferredRankedGroup, Error> {
        let selected = &request.pinned.selected.descriptor;
        if !request
            .pinned
            .owner
            .upgrade()
            .is_some_and(|owner| Arc::ptr_eq(&owner, &self.pages.identity))
            || self
                .select_strategy(&selected.format, Some(&selected.strategy))
                .ok()
                .as_ref()
                != Some(selected)
        {
            return Err(Error::InvalidModel);
        }
        self.pages.defer_ranked_group(request, cancelled)
    }

    /// Commits a ranked group under the caller's publication gate.
    pub fn commit_ranked_group(
        &self,
        candidate: &mut crate::viz_pages::ranked_group::DeferredRankedGroup,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<String, Error> {
        self.pages.commit_ranked_group(candidate, cancelled)
    }

    /// Reads ranked geometry from a currently admitted model.
    pub fn ranked_frame(
        &self,
        reference: &str,
    ) -> Result<Arc<crate::viz_pages::ranked_group::RankedFrame>, Error> {
        self.pages.ranked_frame(reference)
    }

    /// Reads members of a fixed model-local group without registering a wire command.
    pub fn group_rows(
        &self,
        reference: &str,
        group: &str,
        start: u32,
        count: u32,
    ) -> Result<crate::viz_pages::GroupPage, Error> {
        self.pages.group_rows(reference, group, start, count)
    }
}
