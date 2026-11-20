use super::*;
use du_core_contracts::job::{LaneReport, LaneStatusReport, LaneWait};

pub(super) fn lane_status(remaining_directories: u64) -> LaneStatusReport {
    LaneStatusReport {
        lanes: vec![LaneReport {
            lane_index: 0,
            root_indexes: vec![0],
            in_flight: 1,
            limit: 2,
            device_kind: du_core_contracts::scan_observation::DeviceKind::Unknown,
            device_kind_source: du_core_contracts::scan_observation::DeviceKindSource::Unknown,
            mode: None,
            permit: None,
            share: None,
            strategy: None,
            priority_area: None,
            remaining_prioritized_directories: None,
            wait: LaneWait::IoWait,
            wait_confirmed: true,
            remaining_directories,
            failures: 0,
        }],
    }
}

#[test]
fn lane_status_batches_with_progress_in_shared_sequence_order() -> TestResult {
    let mut job = recorded(ProgressUnit::Items)?;
    job.report(5, Some(12))?;
    job.report_lane_status(lane_status(8))?;
    job.report_lane_status(lane_status(3))?;
    job.runtime.tick_progress()?;
    job.runtime.flush()?;
    assert!(job.sink.progress().is_empty());
    assert!(job.sink.lane_statuses().is_empty());

    job.next_batch()?;
    let progress = job.sink.progress();
    let statuses = job.sink.lane_statuses();
    assert_eq!(progress.len(), 1);
    assert_eq!(progress[0].sequence, 3);
    assert_eq!(statuses.len(), 1);
    assert_eq!(statuses[0].sequence, 4);
    assert_eq!(statuses[0].status, lane_status(3));
    assert_eq!(statuses[0].observed_at_ms, 1_000);
    assert_eq!(
        job.sink
            .seen()
            .iter()
            .filter_map(|event| match event {
                Seen::Progress(event) => Some(event.sequence),
                Seen::LaneStatus(event) => Some(event.sequence),
                _ => None,
            })
            .collect::<Vec<_>>(),
        vec![3, 4]
    );

    job.next_batch()?;
    assert_eq!(job.sink.progress().len(), 1);
    assert_eq!(job.sink.lane_statuses().len(), 1);
    job.finish()?;
    check_order(&job.sink.seen())?;
    Ok(())
}

#[test]
fn lane_status_reported_after_job_finished_is_discarded() -> TestResult {
    let mut job = recorded(ProgressUnit::Items)?;
    job.finish()?;
    job.late_lane_status(lane_status(1))?;
    job.next_batch()?;
    assert!(job.sink.lane_statuses().is_empty());
    check_order(&job.sink.seen())?;
    Ok(())
}
