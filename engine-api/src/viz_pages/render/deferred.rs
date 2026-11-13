//! Deferred ownership for F24 model publication.
use super::*;

pub(super) enum PrepareMode {
    Cached,
    Prepared,
    Hierarchical,
}

/// Caller-owned transaction and deferred destruction storage.
///
/// Keep this value alive until after the F24 publication gate is released,
/// regardless of success, error, or unwinding. It is single-use.
pub struct DeferredRender {
    reserved: ReservedRender,
    cleanup: super::super::DeferredRows,
    used: bool,
}

impl ModelPages {
    /// Prepares caller-owned storage for a single gate-protected commit.
    /// Construct and retain it outside the F24 publication gate.
    pub fn defer_reserved(&self, reserved: ReservedRender) -> DeferredRender {
        DeferredRender {
            reserved,
            cleanup: super::super::DeferredRows::default(),
            used: false,
        }
    }

    /// Attempts registration while preserving owned large values in `deferred`.
    ///
    /// The owner must retain `deferred` outside the F24 gate on every exit,
    /// including panic. No injected identity or runtime port is called here.
    pub fn commit_deferred(
        &self,
        deferred: &mut DeferredRender,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<CommittedRender, Error> {
        if deferred.used {
            return Err(Error::Cache(super::super::Error::InvalidModel));
        }
        deferred.used = true;
        check(cancelled)?;
        let candidate = &mut deferred.reserved.candidate;
        if !candidate
            .owner
            .upgrade()
            .is_some_and(|owner| Arc::ptr_eq(&owner, &self.identity))
        {
            return Err(Error::Cache(super::super::Error::InvalidModel));
        }
        use du_core_contracts::result::ResultVersionRead;
        let revision = candidate.input.version.version_id().clone();
        let (model_ref, model) = self
            .register_deferred(
                &mut candidate.input,
                &(candidate.layout.clone(), None),
                &deferred.reserved.reference,
                cancelled,
                &mut deferred.cleanup,
            )
            .map_err(registration_error)?;
        if let Some(reason) = candidate.fallback_reason {
            return Ok(CommittedRender::Fallback {
                reason: if reason == projection::Error::OrderingUnavailable {
                    FallbackReason::OrderingUnavailable
                } else {
                    FallbackReason::NoAttribution
                },
                fallback: VizFallback {
                    model_ref,
                    result_revision: revision,
                },
            });
        }
        let model = model.ok_or(Error::Cache(super::super::Error::InvalidModel))?;
        Ok(CommittedRender::Model(Rendered { model_ref, model }))
    }
}

impl ModelPages {
    pub(in crate::viz_pages) fn prepare_pinned(
        &self,
        dispatcher: &Dispatcher,
        request: Request<'_>,
        source: &current_analysis::CurrentAnalysis,
        defer_links: bool,
        evidence: impl FnOnce(
            &current_analysis::CurrentAnalysis,
            &mut dyn FnMut() -> bool,
        ) -> Result<Vec<ChildEvidence>, Error>,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<PreparedRender, Error> {
        match self.prepare_from(
            dispatcher,
            request,
            source,
            evidence,
            cancelled,
            if defer_links {
                PrepareMode::Hierarchical
            } else {
                PrepareMode::Prepared
            },
        )? {
            Preparation::Candidate(candidate) => Ok(candidate),
            Preparation::Cached(_) => Err(Error::Cache(super::super::Error::InvalidModel)),
        }
    }
}

impl ReservedRender {
    pub(crate) fn preview(&self) -> Option<Rendered> {
        self.candidate.layout.as_ref().map(|layout| Rendered {
            model_ref: self.reference.clone(),
            model: layout.clone(),
        })
    }
}
