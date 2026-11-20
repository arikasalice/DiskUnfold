//! Stage-one static worker policy for storage devices (F02 FR7, FR15).
use du_core_contracts::{
    analysis_ipc::{ScanLaneStrategy, ScanResourceMode},
    scan_observation::DeviceKind,
};
use du_job_runtime::device_permit;
use std::num::NonZeroU32;

const STRATEGY_ID: &str = "resource.static";
const STRATEGY_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Condition {
    pub mode: ScanResourceMode,
    pub user_limit: Option<u32>,
}

impl Default for Condition {
    fn default() -> Self {
        Self {
            mode: ScanResourceMode::Auto,
            user_limit: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Policy {
    pub mode: ScanResourceMode,
    pub permit: u32,
    pub share: Option<NonZeroU32>,
    pub strategy: ScanLaneStrategy,
    pub effective_limit: u32,
}

/// Accepts a positive limit no greater than the maximum permit of this job's lanes.
pub(crate) fn valid_user_limit(limit: Option<u32>, max_permit: u32) -> bool {
    limit.is_none_or(|value| (1..=max_permit).contains(&value))
}

pub(super) fn resolve(
    condition: Condition,
    kind: DeviceKind,
    share: Option<NonZeroU32>,
) -> Option<Policy> {
    if condition.user_limit == Some(0) {
        return None;
    }
    let permit = device_permit(kind);
    let start = match condition.mode {
        ScanResourceMode::Auto => start_limit(kind),
        ScanResourceMode::Quiet => (start_limit(kind) / 2).max(1),
        ScanResourceMode::Fast => permit,
    };
    let effective_limit = condition
        .user_limit
        .map_or(start, |limit| limit.min(start))
        .min(share.map_or(permit, NonZeroU32::get))
        .min(permit)
        .max(1);
    Some(Policy {
        mode: condition.mode,
        permit,
        share,
        strategy: ScanLaneStrategy {
            id: STRATEGY_ID.to_owned(),
            version: STRATEGY_VERSION,
        },
        effective_limit,
    })
}

pub(super) fn resolve_or_fallback(
    condition: Condition,
    kind: DeviceKind,
    share: Option<NonZeroU32>,
) -> Policy {
    resolve(condition, kind, share).unwrap_or_else(|| {
        eprintln!("warning: resource.static calculation failed; using one worker");
        Policy {
            mode: condition.mode,
            permit: device_permit(kind),
            share,
            strategy: ScanLaneStrategy {
                id: STRATEGY_ID.to_owned(),
                version: STRATEGY_VERSION,
            },
            effective_limit: 1,
        }
    })
}

pub(super) fn start_limit(kind: DeviceKind) -> u32 {
    match kind {
        DeviceKind::Ssd => 8,
        DeviceKind::Hdd => 2,
        DeviceKind::Unknown => 4,
    }
}

#[cfg(test)]
mod tests;
