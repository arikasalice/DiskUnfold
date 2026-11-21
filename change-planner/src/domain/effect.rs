use crate::ports::{
    EstimateQuality, PlanAction, PlanItem, PlanItemState, RestoreRestriction, VolumeEstimate,
};
use du_core_contracts::{SizeAmount, SizeBasis, SizeEvidence, volume::CapabilityState};
use std::collections::{BTreeMap, HashSet};

/// Computes the v1 estimate once for each included file identity.
pub(crate) fn expected_effect(items: &[PlanItem]) -> Vec<VolumeEstimate> {
    let mut seen = HashSet::new();
    let mut volumes = BTreeMap::<Vec<u8>, VolumeEstimate>::new();
    for item in items
        .iter()
        .filter(|item| item.state == PlanItemState::Included)
    {
        let estimate_size = match item.action {
            PlanAction::Trash | PlanAction::Delete => item.expected_size.as_ref(),
            PlanAction::Move if item.overwrite => item.displaced_size.as_ref(),
            PlanAction::Move => None,
        };
        if item.action == PlanAction::Move && !item.overwrite {
            continue;
        }
        let identity = item
            .expected_identity
            .as_ref()
            .map(|identity| (identity.volume_id.clone(), identity.file_id.clone()));
        if let Some(identity) = identity
            && !seen.insert(identity)
        {
            continue;
        }
        let estimate = volumes
            .entry(item.volume_id.clone())
            .or_insert_with(|| VolumeEstimate {
                volume_id: item.volume_id.clone(),
                known_bytes: 0,
                unconfirmed_items: 0,
                quality: EstimateQuality::Exact,
            });
        if item.expected_identity.is_none() {
            add_size(estimate, None);
        } else {
            add_size(estimate, estimate_size);
        }
    }
    volumes.into_values().collect()
}

fn add_size(estimate: &mut VolumeEstimate, size: Option<&du_core_contracts::SizeValue>) {
    let Some(size) = size else {
        estimate.unconfirmed_items = estimate.unconfirmed_items.saturating_add(1);
        estimate.quality = EstimateQuality::Unconfirmed;
        return;
    };
    if size.evidence == SizeEvidence::Unknown || size.basis != SizeBasis::Logical {
        estimate.unconfirmed_items = estimate.unconfirmed_items.saturating_add(1);
        estimate.quality = EstimateQuality::Unconfirmed;
        return;
    }
    match &size.amount {
        SizeAmount::Exact { bytes } => {
            if let Some(total) = estimate.known_bytes.checked_add(bytes.get()) {
                estimate.known_bytes = total;
            } else {
                estimate.unconfirmed_items = estimate.unconfirmed_items.saturating_add(1);
                estimate.quality = EstimateQuality::Unconfirmed;
            }
        }
        SizeAmount::LowerBound { bytes } => {
            if let Some(total) = estimate.known_bytes.checked_add(bytes.get()) {
                estimate.known_bytes = total;
                if estimate.quality == EstimateQuality::Exact {
                    estimate.quality = EstimateQuality::LowerBound;
                }
            } else {
                estimate.unconfirmed_items = estimate.unconfirmed_items.saturating_add(1);
                estimate.quality = EstimateQuality::Unconfirmed;
            }
        }
        SizeAmount::Unknown => {
            estimate.unconfirmed_items = estimate.unconfirmed_items.saturating_add(1);
            estimate.quality = EstimateQuality::Unconfirmed;
        }
    }
}

/// Maps action and current trash capability to the restore limitation table.
pub(crate) fn restore_restriction(
    action: PlanAction,
    trash_capability: Option<CapabilityState>,
) -> RestoreRestriction {
    match action {
        PlanAction::Move => RestoreRestriction::Supported,
        PlanAction::Delete => RestoreRestriction::Unsupported,
        PlanAction::Trash => match trash_capability {
            Some(CapabilityState::Supported | CapabilityState::Partial) => {
                RestoreRestriction::Supported
            }
            Some(CapabilityState::Unsupported) => RestoreRestriction::Unsupported,
            None => RestoreRestriction::Unconfirmed,
        },
    }
}
