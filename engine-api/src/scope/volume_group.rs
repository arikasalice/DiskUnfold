//! Root-volume-group review evidence and GUI projection.
use du_core_contracts::{
    OsPath, PathEncoding,
    analysis_ipc::{ScopeDraft, ScopeVolumeGroup},
    volume::{RootVolumeGroup, RootVolumeGroupObservation, VolumePort},
};

pub(super) fn observe_group(
    draft: &ScopeDraft,
    volumes: Option<&dyn VolumePort>,
) -> RootVolumeGroupObservation {
    let includes_unix_root = draft.includes.iter().any(|path| {
        path.encoding == PathEncoding::Unix && path.raw.bytes().is_ok_and(|raw| raw == b"/")
    });
    if includes_unix_root {
        volumes.map_or(
            RootVolumeGroupObservation::NotApplicable,
            VolumePort::root_volume_group,
        )
    } else {
        RootVolumeGroupObservation::NotApplicable
    }
}

pub(crate) fn group_value(group: &RootVolumeGroupObservation) -> Option<RootVolumeGroup> {
    match group {
        RootVolumeGroupObservation::Confirmed(group) => Some(group.clone()),
        RootVolumeGroupObservation::NotApplicable | RootVolumeGroupObservation::Unconfirmed => None,
    }
}

pub(crate) fn group_wire(group: &RootVolumeGroupObservation) -> Option<ScopeVolumeGroup> {
    let data_mount = match group {
        RootVolumeGroupObservation::Confirmed(group) => group.data_mount.clone(),
        RootVolumeGroupObservation::NotApplicable | RootVolumeGroupObservation::Unconfirmed => {
            return None;
        }
    };
    let display = String::from_utf8_lossy(&data_mount).into_owned();
    Some(ScopeVolumeGroup {
        data_mount: OsPath {
            encoding: PathEncoding::Unix,
            raw: du_core_contracts::PathBytes::from_bytes(&data_mount),
            lossy: std::str::from_utf8(&data_mount).is_err(),
            display,
        },
    })
}

pub(crate) fn group_notice(group: &RootVolumeGroupObservation) -> Option<&'static str> {
    match group {
        RootVolumeGroupObservation::Confirmed(_) => Some(ScopeVolumeGroup::NOTICE_MERGED),
        RootVolumeGroupObservation::Unconfirmed => Some(ScopeVolumeGroup::NOTICE_UNVERIFIED),
        RootVolumeGroupObservation::NotApplicable => None,
    }
}

pub(super) fn scope_notices(
    existing: impl Iterator<Item = String>,
    group: &RootVolumeGroupObservation,
) -> Vec<String> {
    let mut notices = existing.collect::<std::collections::BTreeSet<_>>();
    if let Some(notice) = group_notice(group) {
        notices.insert(notice.to_owned());
    }
    notices.into_iter().collect()
}

/// The home folder offered beside an unconfirmed volume group, on the screen only
/// (#263): never for another notice set or channel.
pub(super) fn suggestion(
    notices: &[String],
    channel: crate::api::Channel,
    volumes: Option<&dyn VolumePort>,
) -> Option<OsPath> {
    let unverified = notices
        .iter()
        .any(|notice| notice == ScopeVolumeGroup::NOTICE_UNVERIFIED);
    if channel != crate::api::Channel::Gui || !unverified {
        return None;
    }
    let raw = volumes?.home_directory()?;
    Some(OsPath {
        encoding: PathEncoding::Unix,
        raw: du_core_contracts::PathBytes::from_bytes(&raw),
        lossy: std::str::from_utf8(&raw).is_err(),
        display: String::from_utf8_lossy(&raw).into_owned(),
    })
}

#[cfg(test)]
mod tests;
