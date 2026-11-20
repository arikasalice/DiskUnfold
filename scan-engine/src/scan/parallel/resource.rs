//! Stage-one static worker policy for unknown storage devices (F02 FR7).
use du_core_contracts::analysis_ipc::{ScanLaneStrategy, ScanResourceMode};

pub(super) const UNKNOWN_DEVICE_START: u32 = 4;
pub(super) const UNKNOWN_DEVICE_PERMIT: u32 = 16;
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
    pub strategy: ScanLaneStrategy,
    pub effective_limit: u32,
}

/// Stage one accepts only a positive user limit within the unknown-device permit.
pub(crate) fn valid_user_limit(limit: Option<u32>) -> bool {
    limit.is_none_or(|value| (1..=UNKNOWN_DEVICE_PERMIT).contains(&value))
}

pub(super) fn resolve(condition: Condition, permit: u32) -> Option<Policy> {
    if permit == 0
        || condition
            .user_limit
            .is_some_and(|limit| limit == 0 || limit > permit)
    {
        return None;
    }
    let start = match condition.mode {
        ScanResourceMode::Auto => UNKNOWN_DEVICE_START,
        ScanResourceMode::Quiet => (UNKNOWN_DEVICE_START / 2).max(1),
        ScanResourceMode::Fast => permit,
    };
    let effective_limit = condition
        .user_limit
        .map_or(start, |limit| limit.min(start))
        .min(permit)
        .max(1);
    Some(Policy {
        mode: condition.mode,
        permit,
        strategy: ScanLaneStrategy {
            id: STRATEGY_ID.to_owned(),
            version: STRATEGY_VERSION,
        },
        effective_limit,
    })
}

pub(super) fn resolve_or_fallback(condition: Condition, permit: u32) -> Policy {
    resolve(condition, permit).unwrap_or_else(|| {
        eprintln!("warning: resource.static calculation failed; using one worker");
        Policy {
            mode: condition.mode,
            permit,
            strategy: ScanLaneStrategy {
                id: STRATEGY_ID.to_owned(),
                version: STRATEGY_VERSION,
            },
            effective_limit: 1,
        }
    })
}

#[cfg(test)]
mod tests;
