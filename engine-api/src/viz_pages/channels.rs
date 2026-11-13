//! View-channel identities for one engine owned by one desktop window.
//!
//! This registers lifecycle commands only. A render use case must perform all
//! semantic admission before promotion and must coordinate shared F24 work
//! separately; the query-region job cancellation path is not a shared-job gate.
use std::sync::Arc;

use crate::{
    api::{CallerCtx, Channel, EngineBuilder, UseCase},
    error::EngineError,
    query_region::{Owner, Permit, Promotion, RegionError, RegionRegistry},
};
use du_core_contracts::{
    Id,
    ports::Random,
    visualization::request::{VizViewChannel, VizViewOpenRequest},
};
use du_job_runtime::JobIdentity;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Visualization,
}

/// View channels belonging to exactly one desktop engine/window session.
pub struct ViewChannels {
    registry: RegionRegistry<Kind>,
    owner: Owner,
}

/// A promoted render's cancellation and currentness marker.
pub struct ViewPermit(Permit<Kind>);
impl ViewPermit {
    /// Whether replacement, channel close, or session retirement stopped it.
    pub fn is_stopped(&self) -> bool {
        self.0.is_stopped()
    }
}

impl ViewChannels {
    /// Registers GUI lifecycle use cases for this engine's unique window session.
    ///
    /// The caller supplies an OS-backed random source and an explicit capacity.
    /// The returned owner must be shared with the eventual render use case.
    pub fn register(
        builder: &mut EngineBuilder,
        ids: Arc<dyn JobIdentity>,
        random: &dyn Random,
        capacity: usize,
    ) -> Result<Arc<Self>, &'static str> {
        let owner = Owner {
            session: ids.next_id(),
            window: ids.next_id(),
        };
        let registry = RegionRegistry::new(owner.clone(), capacity, ids, random)
            .map_err(|_| "view channel initialization failed")?;
        let channels = Arc::new(Self { registry, owner });
        let retiring = channels.clone();
        builder.on_session_retire(move || {
            let _ = retiring.registry.close_session(&retiring.owner);
        });
        builder.register(Open(channels.clone()));
        builder.register(Close(channels.clone()));
        Ok(channels)
    }

    /// Checks channel ownership before evaluating potentially costly input.
    pub fn validate(&self, channel: &Id) -> Result<(), &'static str> {
        self.registry
            .validate_region(&self.owner, channel, Kind::Visualization)
            .map_err(map_error)
    }

    /// Promotes only a semantically validated request and preserves errors unchanged.
    ///
    /// This marker does not attach or cancel a physical job. Shared computations
    /// require a separate F24-gated subscription owner before render activation.
    pub fn promote_validated<T, E>(
        &self,
        channel: &Id,
        sequence: u32,
        validated: Result<T, E>,
    ) -> Result<Result<(ViewPermit, T), E>, &'static str> {
        let promoted = self
            .registry
            .promote_validated(
                &self.owner,
                channel,
                Kind::Visualization,
                sequence,
                validated,
            )
            .map_err(map_error)?;
        Ok(promoted.map(
            |Promotion {
                 permit, prepared, ..
             }| (ViewPermit(permit), prepared),
        ))
    }
}

fn map_error(error: RegionError) -> &'static str {
    match error {
        RegionError::Unavailable => "ref.unavailable",
        RegionError::WrongKind | RegionError::InvalidSequence => "command.body_invalid",
        _ => "support.internal_error",
    }
}

struct Open(Arc<ViewChannels>);
impl UseCase for Open {
    const NAME: &'static str = "viz.view.open";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = VizViewOpenRequest;
    type Response = VizViewChannel;
    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }
    fn execute(&self, _: Self::Request, _: &CallerCtx) -> Result<Self::Response, EngineError> {
        let view_channel = self
            .0
            .registry
            .open(&self.0.owner, Kind::Visualization)
            .map_err(|e| EngineError::reason(map_error(e)))?;
        Ok(VizViewChannel { view_channel })
    }
}
struct Close(Arc<ViewChannels>);
impl UseCase for Close {
    const NAME: &'static str = "viz.view.close";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = VizViewChannel;
    type Response = VizViewOpenRequest;
    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }
    fn execute(
        &self,
        request: Self::Request,
        _: &CallerCtx,
    ) -> Result<Self::Response, EngineError> {
        self.0
            .registry
            .close(&self.0.owner, &request.view_channel)
            .map_err(|e| EngineError::reason(map_error(e)))?;
        Ok(VizViewOpenRequest {})
    }
}

#[cfg(test)]
mod tests;
