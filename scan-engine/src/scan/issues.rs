//! Classification from collection operations, independent of error text.
use du_core_contracts::{
    ports::{PortError, PortErrorKind},
    scan_failures::{ScanFailureKind as Kind, ScanFailureStage as Stage},
    scan_observation::{MetadataObservation, ObservedEntry, ObservedKind},
};

/// The reason key of an explicit volume disconnection.
pub(super) const DISCONNECTED: &str = "scan.disconnected";

/// The reason key of an entry whose file identity changed between enumeration
/// and the metadata lookup.
pub(super) const REPLACED: &str = "scan.item_replaced";
/// The reason key of an entry whose kind changed between the two observations.
pub(super) const KIND_CHANGED: &str = "scan.item_kind_changed";
/// The reason key of an entry whose two observations cannot be compared.
pub(super) const UNVERIFIABLE: &str = "scan.item_unverifiable";

/// Whether an adapter error reports an explicit volume disconnection.
pub(super) fn disconnected(error: &PortError) -> bool {
    error.kind == PortErrorKind::Io && error.reason_key == DISCONNECTED
}

/// Whether a returned entry carries a disconnection in its metadata or size.
pub(super) fn disconnected_entry(entry: &ObservedEntry) -> bool {
    match &entry.metadata {
        Err(error) => disconnected(error),
        Ok(metadata) => metadata.size.as_ref().is_err_and(disconnected),
    }
}

pub(super) fn reason(error: &PortError) -> &'static str {
    match error.kind {
        PortErrorKind::PermissionDenied => "scan.path_denied",
        PortErrorKind::NotFound => "scan.path_vanished",
        PortErrorKind::Io if disconnected(error) => DISCONNECTED,
        _ => "scan.unclassified",
    }
}

pub(super) fn classify(entry: &ObservedEntry) -> Option<(Kind, Stage, &'static str)> {
    let meta = match &entry.metadata {
        Err(error) => return Some((Kind::CollectionFailed, Stage::Metadata, reason(error))),
        Ok(meta) => meta,
    };
    if let Some(reason) = change(entry, meta) {
        return Some((Kind::Uncertain, Stage::Metadata, reason));
    }
    if meta.kind == ObservedKind::Link {
        return Some((
            Kind::UntraversedLink,
            Stage::Traverse,
            "scan.link_untraversed",
        ));
    }
    if meta.kind == ObservedKind::File
        && let Err(error) = &meta.size
    {
        return Some((Kind::CollectionFailed, Stage::Metadata, reason(error)));
    }
    None
}

/// Why the enumeration and the metadata lookup of an entry disagree, or `None`
/// when they agree (F02 FR5.HD1.app-host.DD1). The first match wins:
///
/// 1. vanished: the lookup found nothing. A failed lookup is recorded as a
///    collection failure (`scan.path_vanished`) before this table is reached.
/// 2. replaced: both observations carry a file identity and they differ.
/// 3. kind changed: both observations carry a kind and they differ.
/// 4. size changed: not compared, because enumeration carries no size.
/// 5. unverifiable: an identity is missing, or the lookup could not name the
///    kind (`Unknown`) or named one the scan does not handle (`Other`).
fn change(entry: &ObservedEntry, meta: &MetadataObservation) -> Option<&'static str> {
    let identities = entry.enumerated_inode != 0 && meta.inode != 0;
    if identities && entry.enumerated_inode != meta.inode {
        return Some(REPLACED);
    }
    if entry.enumerated_kind != ObservedKind::Unknown
        && meta.kind != ObservedKind::Unknown
        && entry.enumerated_kind != meta.kind
    {
        return Some(KIND_CHANGED);
    }
    if !identities || matches!(meta.kind, ObservedKind::Unknown | ObservedKind::Other) {
        return Some(UNVERIFIABLE);
    }
    None
}

#[cfg(test)]
mod reason_tests {
    use super::*;
    use du_core_contracts::scan_observation::ObservedValue;

    #[test]
    fn only_explicit_io_disconnection_is_preserved() {
        let mut error = PortError {
            kind: PortErrorKind::Io,
            reason_key: "scan.disconnected".into(),
            os_code: Some(1167),
        };
        assert_eq!(reason(&error), "scan.disconnected");
        error.reason_key = "os.query_failed".into();
        assert_eq!(reason(&error), "scan.unclassified");
        error.kind = PortErrorKind::IdentityChanged;
        error.reason_key = "scan.disconnected".into();
        assert_eq!(reason(&error), "scan.unclassified");
        error.kind = PortErrorKind::PermissionDenied;
        assert_eq!(reason(&error), "scan.path_denied");
    }

    fn entry(enumerated: (u64, ObservedKind), looked_up: (u64, ObservedKind)) -> ObservedEntry {
        ObservedEntry {
            name: b"item".to_vec(),
            enumerated_inode: enumerated.0,
            enumerated_kind: enumerated.1,
            metadata: Ok(MetadataObservation {
                kind: looked_up.1,
                device: 1,
                inode: looked_up.0,
                size: Ok(1),
                modified_at: ObservedValue::NotApplicable,
                native_modified_at: None,
                allocated: ObservedValue::NotApplicable,
                link_count: ObservedValue::NotApplicable,
                space_identity: ObservedValue::NotApplicable,
            }),
        }
    }

    fn change_of(
        enumerated: (u64, ObservedKind),
        looked_up: (u64, ObservedKind),
    ) -> Option<&'static str> {
        classify(&entry(enumerated, looked_up)).map(|(kind, stage, reason)| {
            assert_eq!((kind, stage), (Kind::Uncertain, Stage::Metadata));
            reason
        })
    }

    #[test]
    fn change_reasons_take_the_first_match() {
        use ObservedKind::{Directory as Dir, File, Other, Unknown};
        // Agreeing observations, including an enumeration without a kind.
        assert_eq!(change_of((7, File), (7, File)), None);
        assert_eq!(change_of((7, Unknown), (7, Dir)), None);
        // A different identity is a replacement even when the kind changed too.
        assert_eq!(change_of((7, File), (8, Dir)), Some(REPLACED));
        assert_eq!(change_of((7, File), (8, Other)), Some(REPLACED));
        // Without an identity on either side, a changed kind still shows.
        assert_eq!(change_of((0, File), (7, Dir)), Some(KIND_CHANGED));
        assert_eq!(change_of((7, File), (7, Dir)), Some(KIND_CHANGED));
        // Nothing left to compare.
        assert_eq!(change_of((0, File), (7, File)), Some(UNVERIFIABLE));
        assert_eq!(change_of((7, File), (0, File)), Some(UNVERIFIABLE));
        assert_eq!(change_of((7, File), (7, Unknown)), Some(UNVERIFIABLE));
        assert_eq!(change_of((7, Other), (7, Other)), Some(UNVERIFIABLE));
    }

    #[test]
    fn a_missing_second_lookup_stays_a_collection_failure() {
        let mut gone = entry((7, ObservedKind::File), (7, ObservedKind::File));
        gone.metadata = Err(PortError {
            kind: PortErrorKind::NotFound,
            reason_key: "os.not_found".into(),
            os_code: None,
        });
        assert_eq!(
            classify(&gone),
            Some((
                Kind::CollectionFailed,
                Stage::Metadata,
                "scan.path_vanished"
            ))
        );
    }
}
