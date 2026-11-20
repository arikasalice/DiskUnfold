//! A disk image attached inside the scope: listed outside the boundary, never
//! entered, and the result stays complete. Totals are compared with `lstat`.
use super::image::{attach, skip};
use super::*;
use crate::lifecycle::analysis::ipc_calls::amount;
use du_core_contracts::{SizeBasis, result::ResultCompleteness, scan_failures::ScanFailureKind};
use std::os::unix::fs::MetadataExt;

/// Entries, logical bytes and allocated bytes below `path`, never crossing `skip`.
fn walk(path: &Path, skip: &Path) -> Result<(u64, u64, u64), std::io::Error> {
    let mut totals = (0, 0, 0);
    for entry in fs::read_dir(path)? {
        let entry = entry?.path();
        let meta = fs::symlink_metadata(&entry)?;
        totals.0 += 1;
        if meta.is_file() {
            totals.1 += meta.size();
            totals.2 += meta.blocks() * 512;
        } else if meta.is_dir() && entry != skip {
            let inner = walk(&entry, skip)?;
            totals = (totals.0 + inner.0, totals.1 + inner.1, totals.2 + inner.2);
        }
    }
    Ok(totals)
}

#[test]
fn a_mounted_image_is_outside_the_boundary_in_a_complete_result() -> TestResult {
    let temp = tempfile::tempdir()?;
    let profile = tempfile::tempdir()?;
    let base = fs::canonicalize(temp.path())?.join("scope");
    fs::create_dir_all(base.join("sub").join("deep"))?;
    fs::create_dir(base.join("empty"))?;
    fs::write(base.join("a.bin"), vec![1u8; 100])?;
    fs::write(base.join("sub").join("b.bin"), vec![2u8; 23])?;
    fs::write(base.join("sub").join("deep").join("c.bin"), vec![3u8; 7])?;
    let mount = base.join("mnt");
    let attached = match attach(&temp.path().join("image.dmg"), &mount, "APFS", "8m") {
        Ok(attached) => attached,
        Err(reason) => {
            return skip(&format!("no disk image could be attached: {reason}"));
        }
    };
    fs::write(mount.join("inside.bin"), vec![9u8; 50])?;
    let outcome = scan_mounted(&base, &mount, profile.path());
    drop(attached);
    outcome
}

fn scan_mounted(base: &Path, mount: &Path, profile: &Path) -> TestResult {
    let (entries, logical_bytes, allocated_bytes) = walk(base, mount)?;
    let (engine, services, _database) = engine(profile)?;
    let draft = draft(base);
    let review: ScopeReview = read(
        &engine,
        "scope.review",
        json!({"draft":draft,"requestSeq":1}),
    )?;
    let start: ScanStart = read(
        &engine,
        "scan.start",
        json!({"draft":draft,"reviewFingerprint":review.review_fingerprint}),
    )?;
    let finish = finished(&services.events, &start.job_ref)?;
    assert_eq!(finish.body["state"], "done");
    assert_eq!(finish.body["failed"], 0);
    let status: AnalysisJobStatus = read(&engine, "job.get", json!({"jobRef":start.job_ref}))?;
    let version = status.result_version.ok_or("missing result version")?;
    let total: SpaceTotal = read(
        &engine,
        "space.total",
        json!({"resultVersion":version,"requestSeq":1,"target":{"kind":"scope_root"}}),
    )?;
    let failures: ScanFailures = read(&engine, "scan.failures", json!({"resultVersion":version}))?;
    let logical = amount(&total, SizeBasis::Logical)?;
    let allocated = amount(&total, SizeBasis::Allocated)?;
    eprintln!(
        "F02_MACOS_MOUNT_SCAN entries={entries} lstat_logical={logical_bytes} lstat_allocated={allocated_bytes} logical={logical:?} allocated={allocated:?} completed={} failed={} completeness={:?} rows={:?}",
        finish.body["completed"],
        finish.body["failed"],
        total.completeness,
        failures
            .rows
            .iter()
            .map(|row| (row.display_path.clone(), row.reason_key.clone()))
            .collect::<Vec<_>>()
    );
    assert_eq!(total.completeness, ResultCompleteness::Complete);
    // The 50 bytes on the image are not part of the scope.
    assert_eq!(logical, (130, true));
    assert_eq!(logical.0, logical_bytes);
    assert_eq!(allocated, (allocated_bytes, true));
    assert_eq!(failures.total_count, 1);
    let row = failures.rows.first().ok_or("missing boundary row")?;
    assert_eq!(row.kind, ScanFailureKind::UnreadRange);
    assert_eq!(row.reason_key, "scan.boundary_other_volume");
    assert!(
        row.display_path
            .as_deref()
            .is_some_and(|path| path.ends_with("/mnt")),
        "{row:?}"
    );
    Ok(())
}
