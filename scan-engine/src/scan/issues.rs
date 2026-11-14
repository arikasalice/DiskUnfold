//! Classification from collection operations, independent of error text.
use du_core_contracts::{
    ports::{PortError, PortErrorKind},
    scan_failures::{ScanFailureKind as Kind, ScanFailureStage as Stage},
    scan_observation::{ObservedEntry, ObservedKind},
};

/// The reason key of an explicit volume disconnection.
pub(super) const DISCONNECTED: &str = "scan.disconnected";

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
    if entry.enumerated_inode == 0
        || entry.enumerated_inode != meta.inode
        || (entry.enumerated_kind != ObservedKind::Unknown && entry.enumerated_kind != meta.kind)
        || matches!(meta.kind, ObservedKind::Unknown | ObservedKind::Other)
    {
        return Some((Kind::Uncertain, Stage::Metadata, "scan.unclassified"));
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

#[cfg(test)]
mod reason_tests {
    use super::*;

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
}
