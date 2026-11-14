use super::*;

#[test]
fn test_hook_retains_initial_shared_ready_peak_for_each_scan()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "worker test lock")?;
    let ids = Ids(AtomicUsize::new(1));
    let context = Context {
        completed: Arc::new(AtomicUsize::new(0)),
        abandoned: Arc::new(AtomicUsize::new(0)),
    };
    for count in [3, 1] {
        let sink = IngestSink::default();
        let roots = (0..count)
            .map(|n| observed_root(&format!("C:\\{n}"), n as u8))
            .collect();
        let build = sink
            .open_forest(ids.next_id(), roots)
            .map_err(|e| format!("{e:?}"))?;
        let dirs = (0..count)
            .map(|_| {
                Box::new(SmallRoot {
                    fail: false,
                    done: false,
                    name: native("item"),
                }) as Box<dyn AnchoredDirectory>
            })
            .collect();
        let peak = crate::scan_job::ScanDiagnostics::default();
        let control = Control {
            ids: &ids,
            context: &context,
            limit: 1,
            policy: Policy::Legacy(&|_| true),
            scan_control: None,
        };
        let mut counts = JobCounts::default();
        let devices = DevicePlan {
            groups: vec![0; count],
            limit: 4,
        };
        run(
            build,
            dirs,
            control,
            &mut counts,
            devices,
            &ContextClock(&context),
            Some(&peak),
        )
        .map_err(|e| format!("{e:?}"))?;
        assert_eq!(peak.shared_ready_peak(), Some(count));
    }
    Ok(())
}
