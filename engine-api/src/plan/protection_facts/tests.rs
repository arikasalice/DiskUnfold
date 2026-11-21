use super::{classification_id, classify_observed_item, specific_location_class};
use du_change_planner::ports::{DescendantResult, Fact, ProtectionVolumeRole};
use du_core_contracts::{
    OsPath, PathBytes, PathEncoding,
    protection_facts::{KnownLocationFact, KnownLocationId},
    ruleset::classification_id as class,
};

fn unix_path(value: &str) -> OsPath {
    OsPath {
        encoding: PathEncoding::Unix,
        raw: PathBytes::from_bytes(value.as_bytes()),
        display: value.into(),
        lossy: false,
    }
}

#[test]
fn fr28_specific_paths_precede_general_known_locations() {
    let cases = [
        (
            "/home/u/Library/Developer/Xcode/DerivedData/App/Logs",
            class::SYSTEM_CACHE_CLEANABLE,
        ),
        ("/home/u/Library/Logs/Crash.log", class::LOG_DIAGNOSTICS),
        ("/home/u/Library/Caches/App", class::SYSTEM_CACHE_CLEANABLE),
        ("/home/u/CrashDumps/file.dmp", class::LOG_DIAGNOSTICS),
    ];
    for (path, expected) in cases {
        assert_eq!(
            specific_location_class(&unix_path(path), KnownLocationFact::NotListed),
            Some(expected)
        );
    }
    assert_eq!(
        specific_location_class(
            &unix_path("/tmp/file"),
            KnownLocationFact::Listed(KnownLocationId::Temp)
        ),
        Some(class::DIRECT_CLEANABLE)
    );
}

#[test]
fn user_managed_requires_confirmed_identity_and_component() {
    let path = unix_path("/home/u/Downloads/file");
    let location = KnownLocationFact::Listed(KnownLocationId::Downloads);
    let role = Fact::Known(ProtectionVolumeRole::Ordinary);
    for (identity, component, expected) in [
        (true, Fact::Known(false), class::USER_MANAGED),
        (false, Fact::Known(false), class::COMPONENT_UNCONFIRMED),
        (
            true,
            Fact::Unknown("protect.fact.partial"),
            class::COMPONENT_UNCONFIRMED,
        ),
    ] {
        assert_eq!(
            classification_id(&path, location, role, component, identity, false),
            Some(expected.into())
        );
    }
}

#[test]
fn virtual_filesystem_maps_to_memory_backed_temporary_class() {
    assert_eq!(
        classification_id(
            &unix_path("/some/item"),
            KnownLocationFact::NotListed,
            Fact::Known(ProtectionVolumeRole::VirtualFilesystem),
            Fact::Unknown("protect.fact.unsupported"),
            false,
            false,
        ),
        Some(class::MEMORY_BACKED_TEMP.into())
    );
}

#[test]
fn incomplete_folder_does_not_receive_a_classification_id() {
    assert_eq!(
        classify_observed_item(
            &unix_path("/home/u/Downloads/folder"),
            KnownLocationFact::Listed(KnownLocationId::Downloads),
            Fact::Known(ProtectionVolumeRole::Ordinary),
            Fact::Known(false),
            Fact::Known(DescendantResult::Incomplete),
            true,
            false,
        ),
        None
    );
}
