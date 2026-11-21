//! Mapping from the shared protection facts contract to planner facts.

use std::sync::Arc;

use du_change_planner::ports::PlatformFactsPort as DomainPlatformFactsPort;

/// Adapts the core protection facts port to the planner's evaluation values.
pub(super) struct ProtectionFactsAdapter(
    Arc<dyn du_core_contracts::protection_facts::ProtectionFactsPort>,
);

impl ProtectionFactsAdapter {
    /// Uses the platform adapter that owns native filesystem observations.
    pub(super) fn new(
        facts: Arc<dyn du_core_contracts::protection_facts::ProtectionFactsPort>,
    ) -> Self {
        Self(facts)
    }
}

impl DomainPlatformFactsPort for ProtectionFactsAdapter {
    fn read(
        &self,
        item: &du_change_planner::ports::AnalysisItemFacts,
        ruleset: &du_change_planner::ports::RulesetSnapshot,
        action: du_change_planner::ports::PlanAction,
    ) -> Result<
        du_change_planner::ports::PlatformProtectionFacts,
        du_core_contracts::ports::PortError,
    > {
        use du_change_planner::ports::PlatformProtectionFacts;
        let observed = observed_item(item);
        let location = read_location(self.0.as_ref(), &observed, ruleset);
        let current_identity = self.0.identity(&observed);
        let identity = map_identity(current_identity, item.identity.as_ref());
        let link_boundary = map_link_boundary(self.0.link_boundary(&observed));
        let volume_role = map_volume_role(self.0.volume_role(&observed));
        let is_root = item.path_components.len() <= 1;
        let known_location = map_known_location(location, is_root);
        let os_component = map_boolean(self.0.os_component(&observed));
        let age_guard = map_guard(self.0.guard_age(&observed));
        let permission_guard = map_guard(
            self.0
                .guard_permission(&observed, protection_operation(action)),
        );
        let disk_backed_guard = map_guard(self.0.guard_disk_backed(&observed));
        let class_id = classify_observed_item(
            &item.path,
            location,
            volume_role,
            os_component,
            item.descendants,
            matches!(
                identity,
                du_change_planner::ports::Fact::Known(
                    du_change_planner::ports::IdentityObservation::Matches
                )
            ),
            is_root,
        );
        let user_rule = map_user_rule(ruleset.user_rules.is_empty());
        Ok(PlatformProtectionFacts {
            identity,
            link_boundary,
            volume_role,
            known_location,
            classification_id: class_id,
            os_component,
            age_guard,
            permission_guard,
            disk_backed_guard,
            user_rule,
            descendants: item.descendants,
            trash_capability: None,
        })
    }
}

fn classify_observed_item(
    path: &du_core_contracts::OsPath,
    location: du_core_contracts::protection_facts::KnownLocationFact,
    role: du_change_planner::ports::Fact<du_change_planner::ports::ProtectionVolumeRole>,
    os_component: du_change_planner::ports::Fact<bool>,
    descendants: du_change_planner::ports::Fact<du_change_planner::ports::DescendantResult>,
    identity_confirmed: bool,
    is_root: bool,
) -> Option<String> {
    if !matches!(
        descendants,
        du_change_planner::ports::Fact::Known(du_change_planner::ports::DescendantResult::Clear)
    ) {
        return None;
    }
    classification_id(
        path,
        location,
        role,
        os_component,
        identity_confirmed,
        is_root,
    )
}

fn protection_operation(
    action: du_change_planner::ports::PlanAction,
) -> du_core_contracts::protection_facts::ProtectionOperation {
    use du_change_planner::ports::PlanAction;
    use du_core_contracts::protection_facts::ProtectionOperation;
    match action {
        PlanAction::Delete => ProtectionOperation::Delete,
        PlanAction::Move => ProtectionOperation::Move,
        PlanAction::Trash => ProtectionOperation::Trash,
    }
}

fn observed_item(
    item: &du_change_planner::ports::AnalysisItemFacts,
) -> du_core_contracts::protection_facts::ObservedProtectionItem {
    use du_core_contracts::protection_facts::{ObservedFileIdentity, ObservedProtectionItem};
    ObservedProtectionItem {
        path: item.path.clone(),
        scan_root_path: item
            .path_components
            .first()
            .map(|root| observed_path(root, item.path.encoding.clone())),
        kind: item.kind,
        volume_id: item.volume_id.clone(),
        identity: item.identity.as_ref().map(|identity| ObservedFileIdentity {
            volume_id: identity.volume_id.clone(),
            file_id: identity.file_id.clone(),
            volume_serial: None,
            mount_id: None,
        }),
        modified_at: item.modified_at.clone(),
    }
}

fn read_location(
    facts: &dyn du_core_contracts::protection_facts::ProtectionFactsPort,
    observed: &du_core_contracts::protection_facts::ObservedProtectionItem,
    ruleset: &du_change_planner::ports::RulesetSnapshot,
) -> du_core_contracts::protection_facts::KnownLocationFact {
    let Some(version) =
        du_core_contracts::ruleset::KnownLocationTableVersion::new(ruleset.known_location_version)
    else {
        return du_core_contracts::protection_facts::KnownLocationFact::Unknown(
            du_core_contracts::protection_facts::fact_reason::STALE_TABLE,
        );
    };
    facts.known_location(observed, version)
}

fn map_identity(
    current: du_core_contracts::protection_facts::ProtectionFact<
        du_core_contracts::protection_facts::ObservedFileIdentity,
    >,
    expected: Option<&du_change_planner::ports::TargetIdentity>,
) -> du_change_planner::ports::Fact<du_change_planner::ports::IdentityObservation> {
    use du_change_planner::ports::{Fact, IdentityObservation};
    use du_core_contracts::protection_facts::ProtectionFact;
    match (current, expected) {
        (ProtectionFact::Known(current), Some(expected)) => {
            let matches = current.volume_id == expected.volume_id
                && (current.file_id == expected.file_id
                    || (current.file_id.len() == 16
                        && expected.file_id.len() == 8
                        && current.file_id[..8] == expected.file_id));
            Fact::Known(if matches {
                IdentityObservation::Matches
            } else {
                IdentityObservation::Mismatch
            })
        }
        (ProtectionFact::Unknown(reason), _) => Fact::Unknown(reason),
        (ProtectionFact::Known(_), None) => {
            Fact::Unknown(du_core_contracts::protection_facts::fact_reason::PARTIAL)
        }
    }
}

fn map_link_boundary(
    fact: du_core_contracts::protection_facts::ProtectionFact<
        du_core_contracts::protection_facts::LinkBoundaryFact,
    >,
) -> du_change_planner::ports::Fact<du_change_planner::ports::LinkBoundary> {
    use du_change_planner::ports::{Fact, LinkBoundary};
    use du_core_contracts::protection_facts::{LinkBoundaryFact, ProtectionFact};
    match fact {
        ProtectionFact::Known(LinkBoundaryFact::WithinVolume) => {
            Fact::Known(LinkBoundary::WithinVolume)
        }
        ProtectionFact::Known(LinkBoundaryFact::OutsideVolume) => {
            Fact::Known(LinkBoundary::OutsideVolume)
        }
        ProtectionFact::Unknown(reason) => Fact::Unknown(reason),
    }
}

fn map_volume_role(
    fact: du_core_contracts::protection_facts::ProtectionFact<
        du_core_contracts::protection_facts::VolumeRoleFact,
    >,
) -> du_change_planner::ports::Fact<du_change_planner::ports::ProtectionVolumeRole> {
    use du_change_planner::ports::{Fact, ProtectionVolumeRole};
    use du_core_contracts::protection_facts::{ProtectionFact, VolumeRoleFact};
    match fact {
        ProtectionFact::Known(VolumeRoleFact::Ordinary) => {
            Fact::Known(ProtectionVolumeRole::Ordinary)
        }
        ProtectionFact::Known(VolumeRoleFact::OsOnly) => Fact::Known(ProtectionVolumeRole::OsOnly),
        ProtectionFact::Known(VolumeRoleFact::SystemPartition) => {
            Fact::Known(ProtectionVolumeRole::SystemPartition)
        }
        ProtectionFact::Known(VolumeRoleFact::VirtualFilesystem) => {
            Fact::Known(ProtectionVolumeRole::VirtualFilesystem)
        }
        ProtectionFact::Known(VolumeRoleFact::MemoryTemporary) => {
            Fact::Known(ProtectionVolumeRole::MemoryTemporary)
        }
        ProtectionFact::Unknown(reason) => Fact::Unknown(reason),
    }
}

fn map_known_location(
    location: du_core_contracts::protection_facts::KnownLocationFact,
    is_root: bool,
) -> du_change_planner::ports::Fact<du_change_planner::ports::KnownLocation> {
    use du_change_planner::ports::{Fact, KnownLocation};
    use du_core_contracts::protection_facts::KnownLocationFact;
    if is_root {
        return match location {
            KnownLocationFact::Unknown(reason) => Fact::Unknown(reason),
            KnownLocationFact::Listed(_) | KnownLocationFact::NotListed => {
                Fact::Known(KnownLocation::Root)
            }
        };
    }
    match location {
        KnownLocationFact::Listed(location) => Fact::Known(KnownLocation::Under(location)),
        KnownLocationFact::NotListed => Fact::Known(KnownLocation::NotListed),
        KnownLocationFact::Unknown(reason) => Fact::Unknown(reason),
    }
}

fn map_boolean(
    fact: du_core_contracts::protection_facts::ProtectionFact<bool>,
) -> du_change_planner::ports::Fact<bool> {
    use du_change_planner::ports::Fact;
    use du_core_contracts::protection_facts::ProtectionFact;
    match fact {
        ProtectionFact::Known(value) => Fact::Known(value),
        ProtectionFact::Unknown(reason) => Fact::Unknown(reason),
    }
}

fn map_user_rule(
    empty: bool,
) -> du_change_planner::ports::Fact<du_change_planner::ports::GuardResult> {
    use du_change_planner::ports::{Fact, GuardResult};
    if empty {
        Fact::Known(GuardResult::Clear)
    } else {
        Fact::Unknown(du_core_contracts::protection_facts::fact_reason::UNSUPPORTED)
    }
}

fn observed_path(
    raw: &[u8],
    encoding: du_core_contracts::PathEncoding,
) -> du_core_contracts::OsPath {
    let (display, lossy) = match encoding {
        du_core_contracts::PathEncoding::Unix => match std::str::from_utf8(raw) {
            Ok(value) => (value.to_owned(), false),
            Err(_) => (String::from_utf8_lossy(raw).into_owned(), true),
        },
        du_core_contracts::PathEncoding::WindowsUtf16le => {
            let (chunks, _) = raw.as_chunks::<2>();
            let words = chunks
                .iter()
                .map(|pair| u16::from_le_bytes(*pair))
                .collect::<Vec<_>>();
            let display = String::from_utf16_lossy(&words);
            let lossy = std::char::decode_utf16(words).any(|value| value.is_err());
            (display, lossy)
        }
    };
    du_core_contracts::OsPath {
        encoding,
        raw: du_core_contracts::PathBytes::from_bytes(raw),
        display,
        lossy,
    }
}

fn map_guard(
    fact: du_core_contracts::protection_facts::ProtectionFact<
        du_core_contracts::protection_facts::ProtectionGuardFact,
    >,
) -> du_change_planner::ports::Fact<du_change_planner::ports::GuardResult> {
    use du_change_planner::ports::{Fact, GuardResult};
    use du_core_contracts::protection_facts::{ProtectionFact, ProtectionGuardFact};
    match fact {
        ProtectionFact::Known(ProtectionGuardFact::Clear) => Fact::Known(GuardResult::Clear),
        ProtectionFact::Known(ProtectionGuardFact::Protected) => {
            Fact::Known(GuardResult::Protected)
        }
        ProtectionFact::Unknown(reason) => Fact::Unknown(reason),
    }
}

fn classification_id(
    path: &du_core_contracts::OsPath,
    location: du_core_contracts::protection_facts::KnownLocationFact,
    role: du_change_planner::ports::Fact<du_change_planner::ports::ProtectionVolumeRole>,
    os_component: du_change_planner::ports::Fact<bool>,
    identity_confirmed: bool,
    is_root: bool,
) -> Option<String> {
    use du_change_planner::ports::{Fact, ProtectionVolumeRole};
    use du_core_contracts::{
        protection_facts::{KnownLocationFact, KnownLocationId},
        ruleset::classification_id as class,
    };
    if is_root {
        return (!matches!(location, KnownLocationFact::Unknown(_)))
            .then(|| class::ROOT.to_owned());
    }
    if matches!(
        role,
        Fact::Known(ProtectionVolumeRole::OsOnly | ProtectionVolumeRole::SystemPartition)
    ) {
        return Some(class::OS_COMPONENT.into());
    }
    if matches!(
        role,
        Fact::Known(
            ProtectionVolumeRole::VirtualFilesystem | ProtectionVolumeRole::MemoryTemporary
        )
    ) {
        return Some(class::MEMORY_BACKED_TEMP.into());
    }
    if matches!(os_component, Fact::Known(true)) {
        return Some(class::OS_COMPONENT.into());
    }
    if let Some(specific_class) = specific_location_class(path, location) {
        return Some(specific_class.into());
    }
    if matches!(
        location,
        KnownLocationFact::Listed(KnownLocationId::Downloads | KnownLocationId::UserHome)
    ) {
        return Some(
            if identity_confirmed && matches!(os_component, Fact::Known(false)) {
                class::USER_MANAGED
            } else {
                class::COMPONENT_UNCONFIRMED
            }
            .into(),
        );
    }
    if !matches!(os_component, Fact::Known(false))
        || !matches!(role, Fact::Known(ProtectionVolumeRole::Ordinary))
    {
        return None;
    }
    let KnownLocationFact::Listed(location) = location else {
        return None;
    };
    let id = match location {
        KnownLocationId::AppData => class::APP_MANAGED_USER_DATA,
        KnownLocationId::UserCache => class::SYSTEM_CACHE_CLEANABLE,
        KnownLocationId::Temp => class::DIRECT_CLEANABLE,
        KnownLocationId::Downloads | KnownLocationId::UserHome => return None,
        KnownLocationId::MemoryManaged => class::MEMORY_BACKED_TEMP,
        KnownLocationId::TrashStore => class::TRASH_STORE,
    };
    Some(id.into())
}

/// Applies the specific FR28 location rows before their broader F16 roots.
fn specific_location_class(
    path: &du_core_contracts::OsPath,
    location: du_core_contracts::protection_facts::KnownLocationFact,
) -> Option<&'static str> {
    use du_core_contracts::protection_facts::{KnownLocationFact, KnownLocationId};
    use du_core_contracts::ruleset::classification_id as class;

    if matches!(location, KnownLocationFact::Listed(KnownLocationId::Temp)) {
        return Some(class::DIRECT_CLEANABLE);
    }
    let segments = path_segments(path)?;
    if has_segment_sequence(&segments, &["Xcode", "DerivedData"], path.encoding.clone()) {
        return Some(class::SYSTEM_CACHE_CLEANABLE);
    }
    if has_segment(&segments, "CrashDumps", path.encoding.clone())
        || has_segment(&segments, "Logs", path.encoding.clone())
    {
        return Some(class::LOG_DIAGNOSTICS);
    }
    if has_segment(&segments, "Caches", path.encoding.clone()) {
        return Some(class::SYSTEM_CACHE_CLEANABLE);
    }
    None
}

fn path_segments(path: &du_core_contracts::OsPath) -> Option<Vec<String>> {
    let raw = path.raw.bytes().ok()?;
    let decoded = match &path.encoding {
        du_core_contracts::PathEncoding::Unix => String::from_utf8(raw).ok()?,
        du_core_contracts::PathEncoding::WindowsUtf16le => {
            let (chunks, remainder) = raw.as_chunks::<2>();
            if !remainder.is_empty() {
                return None;
            }
            let units = chunks
                .iter()
                .map(|pair| u16::from_le_bytes(*pair))
                .collect::<Vec<_>>();
            char::decode_utf16(units)
                .collect::<Result<String, _>>()
                .ok()?
        }
    };
    Some(
        decoded
            .split(['/', '\\'])
            .filter(|part| !part.is_empty())
            .map(str::to_owned)
            .collect(),
    )
}

fn has_segment(
    segments: &[String],
    expected: &str,
    encoding: du_core_contracts::PathEncoding,
) -> bool {
    segments
        .iter()
        .any(|segment| segment_eq(segment, expected, encoding.clone()))
}

fn has_segment_sequence(
    segments: &[String],
    expected: &[&str],
    encoding: du_core_contracts::PathEncoding,
) -> bool {
    segments.windows(expected.len()).any(|window| {
        window
            .iter()
            .zip(expected)
            .all(|(segment, expected)| segment_eq(segment, expected, encoding.clone()))
    })
}

fn segment_eq(actual: &str, expected: &str, encoding: du_core_contracts::PathEncoding) -> bool {
    match encoding {
        du_core_contracts::PathEncoding::Unix => actual == expected,
        du_core_contracts::PathEncoding::WindowsUtf16le => actual.eq_ignore_ascii_case(expected),
    }
}

#[cfg(test)]
mod tests;
