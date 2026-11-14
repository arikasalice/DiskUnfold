//! Start-of-scan observations made under one flow before any worker starts.
use super::{
    admission::{self, RootStart, Visited},
    disconnect::Disconnects,
    volume_reports::Reports,
    *,
};
use du_core_contracts::scan_observation::HandleIdentity;

/// Root handles with their start decisions and the identities reserved for them.
pub(super) struct Prepared {
    pub roots: Vec<Box<dyn AnchoredDirectory>>,
    pub starts: Vec<RootStart>,
    pub visited: Visited,
}

/// Probes start volumes, then reserves every root identity in root order.
///
/// Both run under one registered flow, so no worker can admit a directory
/// before all roots are reserved (FR1.HD1.app-host.DD1). A root closed as
/// disconnected is not queried. Cancellation before every root was read
/// returns `Cancelled`, because a partial reservation is not deterministic.
pub(super) fn prepare(
    reports: &mut Reports,
    roots: Vec<Box<dyn AnchoredDirectory>>,
    context: &dyn JobContext,
    disconnects: &Disconnects,
) -> Result<Prepared, JobFlowError> {
    let mut flows = acquire_flows(context, 1)?;
    let flow = flows.pop().ok_or(JobFlowError::Closed)?;
    reports.start(&roots, context, &*flow, disconnects);
    let identities = identities(&roots, context, &*flow, disconnects);
    flow.finish(JobCounts::default());
    let (visited, starts) = Visited::reserve_roots(identities.ok_or(JobFlowError::Cancelled)?);
    Ok(Prepared {
        roots,
        starts,
        visited,
    })
}

fn identities(
    roots: &[Box<dyn AnchoredDirectory>],
    context: &dyn JobContext,
    flow: &dyn JobFlow,
    disconnects: &Disconnects,
) -> Option<Vec<Option<HandleIdentity>>> {
    let mut identities = Vec::with_capacity(roots.len());
    for (index, root) in roots.iter().enumerate() {
        if context.checkpoint() == Checkpoint::Cancel || flow.safe_point() == Checkpoint::Cancel {
            return None;
        }
        let closed = u32::try_from(index).map_or(true, |index| disconnects.closed(index));
        identities.push(if closed {
            None
        } else {
            root.observed_identity()
        });
    }
    Some(identities)
}

/// Records unverified and repeated roots before their first enumeration.
///
/// A disconnected root is skipped; its closure records its own unread range.
pub(super) fn record_starts(
    build: &mut BuildHandle,
    starts: &[RootStart],
    disconnects: &Disconnects,
    counts: &mut JobCounts,
    context: &dyn JobContext,
) -> Result<(), BuildError> {
    for (index, &start) in starts.iter().enumerate() {
        let index = u32::try_from(index).map_err(|_| BuildError::Unavailable)?;
        let verified = matches!(start, RootStart::Collect(Boundary::Volume(_)));
        if !verified && !disconnects.closed(index) {
            let observed_at = context.utc_now();
            admission::record_root_start(build, index, start, counts, observed_at.as_ref())?;
        }
    }
    Ok(())
}
