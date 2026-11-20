//! scan.failures rows for ranges that the scan admission table did not enter.
//!
//! A real sequential collection supplies the rows, so the reason keys, kind,
//! stage and completeness are those the collector publishes.
use super::*;
use du_core_contracts::scan_observation::ObservedValue;
use du_core_contracts::{
    job::Checkpoint,
    result::ResultVersionRead,
    scan_failures::unentered_reason::{
        BOUNDARY_OTHER_VOLUME, BOUNDARY_UNVERIFIABLE, REVISIT_BLOCKED,
    },
    scan_observation::{
        AnchoredDirectory, DirectoryObservations, HandleIdentity, MetadataObservation,
        ObservedEntry, ObservedKind,
    },
};
use du_scan_engine::scan::{ResultIds, collect};
use std::sync::atomic::{AtomicU8, Ordering};

const ROOT: u64 = 1_000;

struct Ids(AtomicU8, Id);

impl ResultIds for Ids {
    fn next_id(&self) -> Id {
        id(self.0.fetch_add(1, Ordering::Relaxed)).unwrap_or_else(|_| self.1.clone())
    }
}

/// A root that lists its entries once and never opens a child.
struct Root {
    entries: Option<Vec<ObservedEntry>>,
    identity: bool,
}

impl DirectoryObservations for Root {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        Ok(self.entries.take().unwrap_or_default())
    }
}

impl AnchoredDirectory for Root {
    // An entered directory would add an open_directory failure.
    fn open_child(&self, _: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        Err(PortError {
            kind: PortErrorKind::Unsupported,
            reason_key: "os.unsupported".into(),
            os_code: None,
        })
    }

    fn observed_identity(&self) -> Option<HandleIdentity> {
        self.identity.then(|| HandleIdentity {
            device: 1,
            inode: ROOT,
            ancestors: Vec::new(),
        })
    }
}

fn observed(name: &str, kind: ObservedKind, device: u64, inode: u64) -> ObservedEntry {
    let size = if device == 1 {
        Ok(3)
    } else {
        Err(PortError {
            kind: PortErrorKind::Unsupported,
            reason_key: "os.unsupported".into(),
            os_code: None,
        })
    };
    ObservedEntry {
        name: name.encode_utf16().flat_map(u16::to_le_bytes).collect(),
        enumerated_inode: inode,
        enumerated_kind: kind.clone(),
        metadata: Ok(MetadataObservation {
            kind,
            device,
            inode,
            size,
            allocated: ObservedValue::NotApplicable,
            link_count: ObservedValue::NotApplicable,
            space_identity: ObservedValue::NotApplicable,
            modified_at: ObservedValue::NotApplicable,
            native_modified_at: None,
        }),
    }
}

/// Another volume, a mount point, a mounted file, the root listed again and
/// a plain file.
fn entries() -> Vec<ObservedEntry> {
    let mut mount = observed("vol", ObservedKind::MountPoint, 3, 60);
    // Enumerated as the directory that the mount covers.
    mount.enumerated_kind = ObservedKind::Directory;
    mount.enumerated_inode = 61;
    let mut hosts = observed("hosts", ObservedKind::MountedFile, 4, 70);
    // Enumerated as the file that the bind mount covers.
    hosts.enumerated_kind = ObservedKind::File;
    hosts.enumerated_inode = 71;
    vec![
        hosts,
        observed("mnt", ObservedKind::Directory, 2, 50),
        mount,
        observed("twin", ObservedKind::Directory, 1, ROOT),
        observed("top", ObservedKind::File, 1, 5),
    ]
}

fn scan(identity: bool) -> Result<(IngestSink, Id, ResultCompleteness), String> {
    let store = IngestSink::default();
    let ids = Ids(AtomicU8::new(1), id(255)?);
    let root = Root {
        entries: Some(entries()),
        identity,
    };
    let outcome = collect(
        &store,
        ids.next_id(),
        Box::new(root),
        &ids,
        || Checkpoint::Continue,
        |_| true,
    );
    let version = outcome.result_version_id.ok_or("missing result")?;
    let pinned = store
        .pin()
        .map_err(|e| format!("{e:?}"))?
        .ok_or("missing")?;
    Ok((store, version, pinned.completeness()))
}

/// Root-relative depth and reason key of each unread-range row, in page order.
fn unread_rows(engine: &Engine, version: &Id) -> Result<Vec<(usize, String)>, String> {
    let body = json!({"resultVersion": version, "kind": "unread_range"});
    let page = page(engine, body)?;
    assert!(page.error.is_none());
    let rows = page.body["rows"].as_array().ok_or("missing rows")?;
    assert_eq!(page.body["totalCount"], rows.len());
    rows.iter()
        .map(|row| {
            assert_eq!(row["kind"], "unread_range");
            assert_eq!(row["stage"], "traverse");
            let depth = row["pathRef"]["relative"].as_array().map_or(0, Vec::len);
            let reason = row["reasonKey"].as_str().ok_or("missing reason")?;
            Ok((depth, reason.to_string()))
        })
        .collect()
}

#[test]
fn other_volumes_mount_points_and_revisits_are_listed_in_a_complete_result() -> Result<(), String> {
    let (store, version, completeness) = scan(true)?;
    assert_eq!(completeness, ResultCompleteness::Complete);
    let rows = unread_rows(&engine(store, true)?, &version)?;
    // Rows follow path bytes: hosts, mnt, twin, vol. A mounted file is
    // another volume, not an unclassified failure.
    let expected = [
        BOUNDARY_OTHER_VOLUME,
        BOUNDARY_OTHER_VOLUME,
        REVISIT_BLOCKED,
        BOUNDARY_OTHER_VOLUME,
    ];
    assert_eq!(rows, expected.map(|reason| (1, reason.to_string())));
    Ok(())
}

#[test]
fn an_unidentified_root_lists_unverifiable_ranges_in_a_result_with_missing_ranges()
-> Result<(), String> {
    let (store, version, completeness) = scan(false)?;
    assert_eq!(completeness, ResultCompleteness::Missing);
    let rows = unread_rows(&engine(store, true)?, &version)?;
    // The root, then the mounted file, which stays outside, mnt, twin and the
    // mount point, which stays outside as well.
    let expected = [
        (0, BOUNDARY_UNVERIFIABLE),
        (1, BOUNDARY_OTHER_VOLUME),
        (1, BOUNDARY_UNVERIFIABLE),
        (1, BOUNDARY_UNVERIFIABLE),
        (1, BOUNDARY_OTHER_VOLUME),
    ];
    assert_eq!(
        rows,
        expected.map(|(depth, reason)| (depth, reason.to_string()))
    );
    Ok(())
}
