//! The first-match rows for a single verified or unverified root.
use super::*;

#[test]
fn table_rows_decide_entry_without_opening_out_of_scope_directories() -> Result<(), String> {
    let _guard = worker_test_lock();
    let tree = tree(0);
    let run = filtered_run(&tree, 4, true)?;
    // Only the undecidable name is a failure and a gap.
    assert_eq!(run.outcome.counts.failed, 1);
    assert_eq!(run.completeness, ResultCompleteness::Missing);
    for inode in [SKIP, ODD, MOUNT, MOUNTED_ROOT, COVERED] {
        assert_eq!(tree.enumerated(inode), 0, "inode {inode} was enumerated");
    }
    for inode in [ROOT, 101, 102] {
        assert_eq!(
            tree.enumerated(inode),
            1,
            "inode {inode} was not entered once"
        );
    }
    for name in ["odd", "skip", "other.txt", "mounted.txt", "covered.txt"] {
        assert!(!tree.probed(name), "{name} was observed");
    }
    let (items, issues, gaps) = run.summary;
    let item = |parts: &[&str], kind, size| (0, path(parts), kind, size);
    let expected_items: Vec<Item> = vec![
        item(&[], ResultItemKind::Directory, false),
        item(&["a"], ResultItemKind::Directory, false),
        item(&["a", "deep.txt"], ResultItemKind::File, true),
        item(&["a", "loop"], ResultItemKind::Directory, false),
        item(&["a", "up"], ResultItemKind::UntraversedLink, true),
        item(&["away"], ResultItemKind::UntraversedLink, true),
        item(&["b"], ResultItemKind::Directory, false),
        item(&["b", "b.txt"], ResultItemKind::File, true),
        item(&["foreign.bin"], ResultItemKind::File, false),
        item(&["mnt"], ResultItemKind::Directory, false),
        item(&["top.txt"], ResultItemKind::File, true),
        item(&["twin"], ResultItemKind::Directory, false),
        item(&["vol"], ResultItemKind::Directory, false),
    ];
    assert_eq!(items, expected_items);
    let mut expected_issues = vec![
        revisit(0, &["a", "loop"]),
        other_volume(0, &["foreign.bin"]),
        other_volume(0, &["mnt"]),
        unverifiable(0, &["odd"]),
        revisit(0, &["twin"]),
        other_volume(0, &["vol"]),
        link(0, &["a", "up"]),
        link(0, &["away"]),
    ];
    // Published order: root, relative path bytes, then kind and stage.
    expected_issues.sort_by(|a, b| (a.0, &a.1, a.2).cmp(&(b.0, &b.1, b.2)));
    assert_eq!(issues, expected_issues);
    assert_eq!(gaps, [unverifiable_gap(0, &["odd"])]);
    Ok(())
}

#[test]
fn worker_count_and_delay_order_keep_the_same_result() -> Result<(), String> {
    let _guard = worker_test_lock();
    let baseline = filtered_run(&tree(0), 1, true)?.summary;
    for (workers, seed) in RUNS {
        let tree = tree(seed);
        let summary = filtered_run(&tree, workers, true)?.summary;
        assert_eq!(summary, baseline, "workers={workers} seed={seed}");
        assert_eq!(tree.enumerated(ROOT), 1);
    }
    Ok(())
}

#[test]
fn unverified_root_identity_is_an_unread_root_and_keeps_the_result_missing() -> Result<(), String> {
    let _guard = worker_test_lock();
    let tree = tree(0);
    let run = filtered_run(&tree, 2, false)?;
    assert!(run.outcome.result_version_id.is_some());
    assert_eq!(run.completeness, ResultCompleteness::Missing);
    assert_eq!(tree.enumerated(ROOT), 1);
    for inode in [101, 102, SKIP, ODD, MOUNT, MOUNTED_ROOT, COVERED] {
        assert_eq!(tree.enumerated(inode), 0);
    }
    let (items, issues, gaps) = run.summary;
    assert!(items.contains(&(0, path(&["top.txt"]), ResultItemKind::File, true)));
    // The root itself is the unread range that reports the missing identity.
    assert!(issues.contains(&unverifiable(0, &[])));
    let unread = [&[][..], &["a"], &["b"], &["mnt"], &["odd"], &["twin"]];
    for parts in unread {
        assert!(issues.contains(&unverifiable(0, parts)), "{parts:?}");
    }
    let mut expected_gaps = unread.map(|parts| unverifiable_gap(0, parts)).to_vec();
    // The other-volume file without a size is an unread value of its own.
    expected_gaps.push((0, path(&["foreign.bin"]), "scan.unclassified".into()));
    expected_gaps.sort();
    assert_eq!(gaps, expected_gaps);
    // The adapter established the mount, so it stays outside the boundary.
    assert!(issues.contains(&other_volume(0, &["vol"])));
    // Without a boundary, the other-volume file is recorded without a size.
    let failed_size = (
        0,
        path(&["foreign.bin"]),
        Kind::CollectionFailed,
        Stage::Metadata,
        "scan.unclassified".to_string(),
    );
    assert!(issues.contains(&failed_size));
    // The root, five unverifiable ranges and the file without a size.
    assert_eq!(run.outcome.counts.failed, 7);
    Ok(())
}

#[test]
fn sequential_and_legacy_parallel_paths_share_the_table() -> Result<(), String> {
    let _guard = worker_test_lock();
    let include = |path: &[Vec<u8>]| path.first().is_none_or(|first| first != b"skip");
    let ids = Ids(AtomicUsize::new(1));
    let sequential_tree = tree(0);
    let sink = IngestSink::default();
    let sequential_outcome = crate::scan::collect(
        &sink,
        ids.next_id(),
        Dir::root(&sequential_tree, true),
        &ids,
        || Checkpoint::Continue,
        include,
    );
    let sequential_version = sink.pin().map_err(|e| format!("{e:?}"))?.ok_or("missing")?;
    let sequential = summary(&sequential_version);
    let parallel_tree = tree(0);
    let sink = IngestSink::default();
    let build = sink.open_forest(ids.next_id(), vec![observed_root("C:\\A", 1)]);
    let roots = vec![Dir::root(&parallel_tree, true)];
    let parallel_outcome = collect_open_forest(build, roots, &ids, &context(), 3, &include);
    let parallel_version = sink.pin().map_err(|e| format!("{e:?}"))?.ok_or("missing")?;
    let parallel = summary(&parallel_version);
    assert_eq!(sequential, parallel);
    // Other volumes, the mount point and the revisits leave no gap or failure.
    assert!(sequential.2.is_empty());
    for (outcome, version) in [
        (&sequential_outcome, &sequential_version),
        (&parallel_outcome, &parallel_version),
    ] {
        assert!(outcome.result_version_id.is_some());
        assert_eq!(outcome.counts.failed, 0);
        assert_eq!(version.completeness(), ResultCompleteness::Complete);
    }
    for tree in [&sequential_tree, &parallel_tree] {
        assert_eq!(tree.enumerated(SKIP), 0);
        assert_eq!(tree.enumerated(MOUNT), 0);
        assert_eq!(tree.enumerated(MOUNTED_ROOT), 0);
        assert_eq!(tree.enumerated(COVERED), 0);
        assert_eq!(tree.enumerated(ROOT), 1);
        assert_eq!(tree.enumerated(ODD), 1);
    }
    Ok(())
}

#[test]
fn sequential_collector_reports_an_unverified_root() -> Result<(), String> {
    let tree = tree(0);
    let ids = Ids(AtomicUsize::new(1));
    let sink = IngestSink::default();
    let root = Dir::root(&tree, false);
    let outcome = crate::scan::collect(
        &sink,
        ids.next_id(),
        root,
        &ids,
        || Checkpoint::Continue,
        |_| true,
    );
    assert!(outcome.completed_scope);
    let version = sink.pin().map_err(|e| format!("{e:?}"))?.ok_or("missing")?;
    assert_eq!(version.completeness(), ResultCompleteness::Missing);
    let (_, issues, gaps) = summary(&version);
    assert!(issues.contains(&unverifiable(0, &[])));
    assert!(issues.contains(&unverifiable(0, &["a"])));
    assert!(issues.contains(&other_volume(0, &["vol"])));
    assert!(gaps.contains(&unverifiable_gap(0, &[])));
    assert!(gaps.contains(&unverifiable_gap(0, &["a"])));
    assert_eq!(tree.enumerated(101), 0);
    Ok(())
}

#[test]
fn priority_requests_and_header_keep_the_frozen_scope() -> Result<(), String> {
    let _guard = worker_test_lock();
    let tree = tree(0);
    let ids = Arc::new(Ids(AtomicUsize::new(1)));
    let sink = IngestSink::default();
    let scope = ids.next_id();
    let root = OpenedScopeRoot {
        canonical_path: OsPath {
            encoding: PathEncoding::WindowsUtf16le,
            raw: PathBytes::from_bytes(&native("C:\\A")),
            display: "C:\\A".into(),
            lossy: false,
        },
        volume_id: vec![1],
        file_id: vec![1],
        directory: Dir::root(&tree, true),
    };
    let job = crate::scan_job::MetadataScan::new(root, sink.clone(), scope.clone(), ids)
        .with_scope_fingerprint("scope-fingerprint".into())
        .with_pre_metadata_filter(Arc::new(filter))
        .with_priority_control();
    if let Ok(mut control) = tree.control.lock() {
        *control = job.control_handle();
    }
    let outcome = job.run(Arc::new(context()));
    let version = sink.pin().map_err(|e| format!("{e:?}"))?.ok_or("missing")?;
    assert_eq!(
        outcome.result_version_id.as_ref(),
        Some(version.version_id())
    );
    assert_eq!(version.scope_version_id(), &scope);
    assert_eq!(version.scope_fingerprint(), Some("scope-fingerprint"));
    let outcomes = tree.priority.lock().map_err(|_| "priority")?.clone();
    assert_eq!(outcomes.len(), 5);
    assert!(outcomes[..4].iter().all(|outcome| matches!(
        outcome,
        Ok(ScanPriorityOutcome::NoEffect {
            same_region: false,
            remaining_directories: 0,
        })
    )));
    assert_eq!(
        outcomes[4],
        Ok(ScanPriorityOutcome::Accepted {
            remaining_directories: 1
        })
    );
    for inode in [SKIP, ODD, MOUNT] {
        assert_eq!(tree.enumerated(inode), 0);
    }
    assert_eq!(tree.enumerated(102), 1);
    Ok(())
}
