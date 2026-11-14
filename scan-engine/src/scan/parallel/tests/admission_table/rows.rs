//! The first-match rows for a single verified or unverified root.
use super::*;

#[test]
fn table_rows_decide_entry_without_opening_out_of_scope_directories() -> Result<(), String> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "lock")?;
    let tree = tree(0);
    let run = filtered_run(&tree, 4, true)?;
    assert_eq!(run.outcome.counts.failed, 0);
    for inode in [SKIP, ODD, MOUNT] {
        assert_eq!(tree.enumerated(inode), 0, "inode {inode} was enumerated");
    }
    for inode in [ROOT, 101, 102] {
        assert_eq!(
            tree.enumerated(inode),
            1,
            "inode {inode} was not entered once"
        );
    }
    assert!(!tree.probed("odd") && !tree.probed("skip") && !tree.probed("other.txt"));
    let (items, issues, gaps) = run.summary;
    let item = |parts: &[&str], kind, size| (0, path(parts), kind, size);
    let expected_items: Vec<Item> = vec![
        item(&[], ResultItemKind::Directory, false),
        item(&["a"], ResultItemKind::Directory, false),
        item(&["a", "deep.txt"], ResultItemKind::File, true),
        item(&["a", "loop"], ResultItemKind::Directory, false),
        item(&["a", "up"], ResultItemKind::UntraversedLink, false),
        item(&["away"], ResultItemKind::UntraversedLink, false),
        item(&["b"], ResultItemKind::Directory, false),
        item(&["b", "b.txt"], ResultItemKind::File, true),
        item(&["foreign.bin"], ResultItemKind::Unknown, false),
        item(&["mnt"], ResultItemKind::Directory, false),
        item(&["top.txt"], ResultItemKind::File, true),
        item(&["twin"], ResultItemKind::Directory, false),
    ];
    assert_eq!(items, expected_items);
    let mut expected_issues = vec![
        unread(0, &["a", "loop"]),
        unread(0, &["foreign.bin"]),
        unread(0, &["mnt"]),
        unread(0, &["odd"]),
        unread(0, &["twin"]),
        link(0, &["a", "up"]),
        link(0, &["away"]),
    ];
    // Published order: root, relative path bytes, then kind and stage.
    expected_issues.sort_by(|a, b| (a.0, &a.1, a.2).cmp(&(b.0, &b.1, b.2)));
    assert_eq!(issues, expected_issues);
    let gap_paths: Vec<_> = gaps.into_iter().map(|(_, gap, _)| gap).collect();
    let expected_gaps = [
        &["a", "loop"][..],
        &["foreign.bin"],
        &["mnt"],
        &["odd"],
        &["twin"],
    ];
    assert_eq!(gap_paths, expected_gaps.map(path));
    Ok(())
}

#[test]
fn worker_count_and_delay_order_keep_the_same_result() -> Result<(), String> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "lock")?;
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
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "lock")?;
    let tree = tree(0);
    let run = filtered_run(&tree, 2, false)?;
    assert!(run.outcome.result_version_id.is_some());
    assert_eq!(run.completeness, ResultCompleteness::Missing);
    assert_eq!(tree.enumerated(ROOT), 1);
    for inode in [101, 102, SKIP, ODD, MOUNT] {
        assert_eq!(tree.enumerated(inode), 0);
    }
    let (items, issues, gaps) = run.summary;
    assert!(items.contains(&(0, path(&["top.txt"]), ResultItemKind::File, true)));
    // The root itself is the unread range that reports the missing identity.
    assert!(issues.contains(&unread(0, &[])));
    assert!(gaps.contains(&(0, Vec::new(), "scan.unclassified".into())));
    for unverifiable in [&["a"][..], &["b"], &["twin"], &["mnt"], &["odd"]] {
        assert!(issues.contains(&unread(0, unverifiable)));
    }
    // Without a boundary, the other-volume file is recorded without a size.
    let failed_size = (
        0,
        path(&["foreign.bin"]),
        Kind::CollectionFailed,
        Stage::Metadata,
        "scan.unclassified".to_string(),
    );
    assert!(issues.contains(&failed_size));
    assert_eq!(run.outcome.counts.failed, 2);
    Ok(())
}

#[test]
fn sequential_and_legacy_parallel_paths_share_the_table() -> Result<(), String> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "lock")?;
    let include = |path: &[Vec<u8>]| path.first().is_none_or(|first| first != b"skip");
    let ids = Ids(AtomicUsize::new(1));
    let sequential_tree = tree(0);
    let sink = IngestSink::default();
    let outcome = crate::scan::collect(
        &sink,
        ids.next_id(),
        Dir::root(&sequential_tree, true),
        &ids,
        || Checkpoint::Continue,
        include,
    );
    assert!(outcome.result_version_id.is_some());
    let sequential = summary(&*sink.pin().map_err(|e| format!("{e:?}"))?.ok_or("missing")?);
    let parallel_tree = tree(0);
    let sink = IngestSink::default();
    let build = sink.open_forest(ids.next_id(), vec![observed_root("C:\\A", 1)]);
    let roots = vec![Dir::root(&parallel_tree, true)];
    collect_open_forest(build, roots, &ids, &context(), 3, &include);
    let parallel = summary(&*sink.pin().map_err(|e| format!("{e:?}"))?.ok_or("missing")?);
    assert_eq!(sequential, parallel);
    for tree in [&sequential_tree, &parallel_tree] {
        assert_eq!(tree.enumerated(SKIP), 0);
        assert_eq!(tree.enumerated(MOUNT), 0);
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
    let (_, issues, _) = summary(&version);
    assert!(issues.contains(&unread(0, &[])));
    assert!(issues.contains(&unread(0, &["a"])));
    assert_eq!(tree.enumerated(101), 0);
    Ok(())
}

#[test]
fn priority_requests_and_header_keep_the_frozen_scope() -> Result<(), String> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "lock")?;
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
