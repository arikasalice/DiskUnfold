//! Deferred Sankey page registration for a caller-owned publication gate (#296).
//!
//! The node rows (with the links leaving each) are the model's table; the geometry
//! travels in the wire model, so the committed entry carries only a marker.
use super::{DeferredRows, Error, ModelInput, ModelPages, PreparedModelInput, session::Session};
use du_core_contracts::ItemRef;
use std::sync::{Arc, Weak};

/// The metadata of a committed Sankey model.
pub struct SankeyFrame;

/// One reserved registration, including cleanup retained outside the outer gate.
pub struct DeferredSankeyPages {
    owner: Weak<()>,
    input: PreparedModelInput,
    reference: String,
    cleanup: DeferredRows,
    used: bool,
}

impl ModelPages {
    /// Validates the node rows and reserves a reference before entering a job gate.
    pub(crate) fn defer_sankey_pages(
        &self,
        input: ModelInput,
        scope: &ItemRef,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<DeferredSankeyPages, Error> {
        let input = super::prepare_scoped_model_input(input, scope, cancelled)?;
        Ok(DeferredSankeyPages {
            owner: Arc::downgrade(&self.identity),
            input,
            reference: self.ids.next_id().as_str().to_owned(),
            cleanup: DeferredRows::default(),
            used: false,
        })
    }

    /// Commits one reserved candidate while retaining retired rows in its owner.
    pub(crate) fn commit_sankey_pages(
        &self,
        deferred: &mut DeferredSankeyPages,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<String, Error> {
        if deferred.used {
            return Err(Error::InvalidModel);
        }
        deferred.used = true;
        if !deferred
            .owner
            .upgrade()
            .is_some_and(|owner| Arc::ptr_eq(&owner, &self.identity))
        {
            return Err(Error::InvalidModel);
        }
        let frame: super::LayoutMetadata = Arc::new(SankeyFrame);
        let (reference, _) = self.register_deferred(
            &mut deferred.input,
            &(None, Some(frame)),
            &deferred.reference,
            cancelled,
            &mut deferred.cleanup,
        )?;
        Ok(reference)
    }
}

impl Session {
    /// Lays out a Sankey graph through the exact descriptor admitted by this session.
    pub(crate) fn compute_sankey_selected(
        &self,
        input: du_analysis_index::visualization::typed_layout::SankeyInput<'_>,
        descriptor: &du_core_contracts::visualization::LayoutDescriptor,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<
        du_analysis_index::visualization::typed_layout::SankeySelected,
        du_analysis_index::visualization::typed_layout::SankeyDispatchError,
    > {
        self.dispatcher
            .execute_sankey(Some(&descriptor.strategy), input, cancelled)
    }

    /// Reserves Sankey node rows for a caller-owned publication gate.
    pub(crate) fn defer_sankey(
        &self,
        input: ModelInput,
        scope: &ItemRef,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<DeferredSankeyPages, Error> {
        self.pages.defer_sankey_pages(input, scope, cancelled)
    }

    /// Commits the reserved Sankey rows while the caller owns its publication gate.
    pub(crate) fn commit_sankey(
        &self,
        candidate: &mut DeferredSankeyPages,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<String, Error> {
        self.pages.commit_sankey_pages(candidate, cancelled)
    }
}
