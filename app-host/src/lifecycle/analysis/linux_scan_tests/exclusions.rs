//! Reviewed exclusions applied by the real Linux adapter before any metadata read.
use super::*;
use du_core_contracts::{
    Timestamp,
    ports::PortError,
    scan_observation::{
        AnchoredDirectory, DirectoryCaseEvidence, DirectoryObservations, FilterDecision,
        FilteredBatch, ObservedEntry, OpenedScopeRoot, TimedFilteredBatch, TimedObservedEntry,
        VolumeSpaceReader,
    },
};
use du_platform_os::linux_observation::LinuxScopeRoots;
use std::sync::Mutex;

type Clock<'a> = &'a dyn Fn() -> Option<Timestamp>;

/// Names offered to the scan filter and directories opened, in any traced directory.
#[derive(Default)]
struct Trace {
    offered: Mutex<Vec<(Vec<u8>, FilterDecision)>>,
    opened: Mutex<Vec<Vec<u8>>>,
}

/// The real Linux scope roots, with every directory wrapped by [`Traced`].
struct TracedRoots(Arc<Trace>);

impl ScopeRootPort for TracedRoots {
    fn open_root(&self, path: &[u8]) -> Result<OpenedScopeRoot, PortError> {
        let mut root = LinuxScopeRoots.open_root(path)?;
        root.directory = Box::new(Traced {
            inner: root.directory,
            trace: self.0.clone(),
        });
        Ok(root)
    }
}

/// Delegates every call to the real directory and records filter decisions and opens.
struct Traced {
    inner: Box<dyn AnchoredDirectory>,
    trace: Arc<Trace>,
}

fn record(trace: &Trace, name: &[u8], decision: FilterDecision) -> FilterDecision {
    if let Ok(mut offered) = trace.offered.lock() {
        offered.push((name.to_vec(), decision));
    }
    decision
}

impl DirectoryObservations for Traced {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        self.inner.next_batch()
    }

    fn next_batch_timed(&mut self, clock: Clock<'_>) -> Result<Vec<TimedObservedEntry>, PortError> {
        self.inner.next_batch_timed(clock)
    }

    fn next_filtered(
        &mut self,
        include: &mut dyn FnMut(&[u8]) -> FilterDecision,
    ) -> Result<FilteredBatch, PortError> {
        let trace = self.trace.clone();
        self.inner
            .next_filtered(&mut |name| record(&trace, name, include(name)))
    }

    fn next_filtered_timed(
        &mut self,
        include: &mut dyn FnMut(&[u8]) -> FilterDecision,
        clock: Clock<'_>,
    ) -> Result<TimedFilteredBatch, PortError> {
        let trace = self.trace.clone();
        self.inner
            .next_filtered_timed(&mut |name| record(&trace, name, include(name)), clock)
    }
}

impl AnchoredDirectory for Traced {
    fn open_child(&self, entry: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        if let Ok(mut opened) = self.trace.opened.lock() {
            opened.push(entry.name.clone());
        }
        Ok(Box::new(Traced {
            inner: self.inner.open_child(entry)?,
            trace: self.trace.clone(),
        }))
    }

    fn observe_directory_case(&self, clock: Clock<'_>) -> Option<DirectoryCaseEvidence> {
        self.inner.observe_directory_case(clock)
    }

    fn volume_space_reader(&self) -> Result<Box<dyn VolumeSpaceReader>, PortError> {
        self.inner.volume_space_reader()
    }

    fn observed_modified_at(&self) -> Option<Timestamp> {
        self.inner.observed_modified_at()
    }

    fn observed_modified_at_timed(
        &self,
        clock: Clock<'_>,
    ) -> (Option<Timestamp>, Option<Timestamp>) {
        self.inner.observed_modified_at_timed(clock)
    }

    fn ancestor_file_ids(&self) -> Option<&[Vec<u8>]> {
        self.inner.ancestor_file_ids()
    }
}

#[test]
fn linux_scan_skips_reviewed_exclusions_before_enumeration() -> TestResult {
    let denied = std::env::var_os("DU_TEST_UNPRIVILEGED").is_some();
    let temp = tempfile::tempdir()?;
    let profile = tempfile::tempdir()?;
    let base = temp.path().join("scope");
    fixture(&base, denied)?;
    // A name that differs from the excluded folder only by case.
    fs::create_dir(base.join("Sub"))?;
    fs::write(base.join("Sub").join("d.bin"), vec![5u8; 11])?;
    let outcome = excluded_scan(&base, profile.path(), denied);
    if denied {
        fs::set_permissions(base.join("locked"), fs::Permissions::from_mode(0o700))?;
    }
    outcome
}

fn excluded_scan(base: &Path, profile: &Path, denied: bool) -> TestResult {
    let trace = Arc::new(Trace::default());
    let ports = ScanPorts {
        roots: Arc::new(TracedRoots(trace.clone())),
        volumes: Arc::new(du_platform_os::volume::LinuxVolumes),
    };
    let database = crate::startup_database::open(profile)?;
    let mut builder = EngineBuilder::default();
    let limit: SharedTableLimitSource = Arc::new(SharedTableLimit::default);
    let services = register_analysis(&mut builder, ports, &database, limit, None)?;
    let engine = builder.build()?;
    let path = |path: &Path| {
        json!({
            "encoding": "unix",
            "raw": PathBytes::from_bytes(path.as_os_str().as_bytes()),
            "display": path.to_string_lossy(),
            "lossy": false
        })
    };
    let draft = json!({"includes": [path(base)], "exclusions": [
        {"kind": "path", "path": path(&base.join("sub"))},
        {"kind": "path", "path": path(&base.join("locked"))},
        {"kind": "name_pattern", "pattern": "raw-*"}
    ], "boundary": "current_volume", "includedMounts": [], "source": "gui"});
    let review: ScopeReview = read(
        &engine,
        "scope.review",
        json!({"draft": draft, "requestSeq": 1}),
    )?;
    assert_eq!(review.targets.len(), 1);
    assert!(
        !review
            .notices
            .iter()
            .any(|notice| notice == "scope.exclusion_ineffective"),
        "{:?}",
        review.notices
    );
    let start: ScanStart = read(
        &engine,
        "scan.start",
        json!({"draft": draft, "reviewFingerprint": review.review_fingerprint}),
    )?;
    let finish = finished(&services.events, &start.job_ref)?;
    assert_eq!(finish.body["state"], "done");
    let status: AnalysisJobStatus = read(&engine, "job.get", json!({"jobRef": start.job_ref}))?;
    let version = status.result_version.ok_or("missing result version")?;
    let total: SpaceTotal = read(
        &engine,
        "space.total",
        json!({"resultVersion": version, "requestSeq": 1, "target": {"kind": "scope_root"}}),
    )?;
    // 135 readable fixture bytes, less sub (30) and raw-\xff (5), plus Sub/d.bin (11).
    assert_eq!(logical(&total)?, (111, false));
    let failures: ScanFailures = read(&engine, "scan.failures", json!({"resultVersion": version}))?;
    // Only the untraversed links remain; an excluded unreadable folder is never opened.
    let reasons: Vec<_> = failures.rows.iter().map(|row| &row.reason_key).collect();
    assert_eq!(failures.total_count, 2, "{reasons:?}");
    let offered = trace.offered.lock().map_err(|_| "trace poisoned")?.clone();
    let opened = trace.opened.lock().map_err(|_| "trace poisoned")?.clone();
    let decisions = |name: &[u8]| -> Vec<FilterDecision> {
        offered
            .iter()
            .filter(|(seen, _)| seen == name)
            .map(|(_, decision)| *decision)
            .collect()
    };
    assert_eq!(decisions(b"sub"), [FilterDecision::Exclude]);
    assert_eq!(decisions(b"raw-\xff"), [FilterDecision::Exclude]);
    assert_eq!(
        decisions(b"locked"),
        if denied {
            vec![FilterDecision::Exclude]
        } else {
            Vec::new()
        }
    );
    assert_eq!(decisions(b"Sub"), [FilterDecision::Admit]);
    assert_eq!(decisions(b"d.bin"), [FilterDecision::Admit]);
    // The excluded folders were never opened, so none of their names were enumerated.
    for hidden in [&b"deep"[..], b"b.bin", b"c.bin", b"hidden"] {
        assert!(decisions(hidden).is_empty(), "{hidden:?}");
    }
    assert!(
        !opened
            .iter()
            .any(|name| name == b"sub" || name == b"locked"),
        "{opened:?}"
    );
    assert!(opened.iter().any(|name| name == b"Sub"), "{opened:?}");
    eprintln!(
        "F02_LINUX_EXCLUSION state=done logical_lower_bound=111 offered={} opened={} failure_rows={} denied_case={denied}",
        offered.len(),
        opened.len(),
        failures.total_count
    );
    Ok(())
}
