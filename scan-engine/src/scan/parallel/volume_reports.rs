//! Scan-lifetime volume observations from retained verified root handles.
use du_analysis_index::{VolumeReport, VolumeReportPoint};
use du_core_contracts::{
    job::{Checkpoint, JobContext, JobCounts, JobFlow, JobFlowError},
    ports::{PortError, PortErrorKind},
    scan_observation::{AnchoredDirectory, VolumeSpaceReader},
};
use std::collections::HashSet;

struct Slot {
    root: usize,
    report: VolumeReport,
    reader: Option<Box<dyn VolumeSpaceReader>>,
}

pub(super) struct Reports(Vec<Slot>);
impl Reports {
    pub(super) fn new(volumes: Vec<Vec<u8>>) -> Self {
        let mut seen = HashSet::new();
        Self(
            volumes
                .into_iter()
                .enumerate()
                .filter_map(|(root, volume_id)| {
                    if !volume_id.is_empty() && !seen.insert(volume_id.clone()) {
                        return None;
                    }
                    Some(Slot {
                        root,
                        report: VolumeReport {
                            volume_id,
                            start: None,
                            end: None,
                        },
                        reader: None,
                    })
                })
                .collect(),
        )
    }
    /// Runs initial volume probes under one independently registered flow.
    pub(super) fn prepare_start(
        &mut self,
        roots: &[Box<dyn AnchoredDirectory>],
        context: &dyn JobContext,
    ) -> Result<(), JobFlowError> {
        let mut flows = super::acquire_flows(context, 1)?;
        if let Some(flow) = flows.pop() {
            self.start(roots, context, &*flow);
            flow.finish(JobCounts::default());
        }
        Ok(())
    }
    pub(super) fn start(
        &mut self,
        roots: &[Box<dyn AnchoredDirectory>],
        context: &dyn JobContext,
        flow: &dyn JobFlow,
    ) {
        for slot in &mut self.0 {
            if context.checkpoint() == Checkpoint::Cancel || flow.safe_point() == Checkpoint::Cancel
            {
                break;
            }
            let result = if slot.report.volume_id.is_empty() {
                Err(PortError {
                    kind: PortErrorKind::Unsupported,
                    reason_key: "space.volume_report_unavailable".into(),
                    os_code: None,
                })
            } else {
                roots[slot.root].volume_space_reader().and_then(|reader| {
                    let result = reader.read(&slot.report.volume_id);
                    slot.reader = Some(reader);
                    result
                })
            };
            slot.report.start = Some(point(context, result));
            if flow.safe_point() == Checkpoint::Cancel {
                break;
            }
        }
    }
    pub(super) fn finish(&mut self, context: &dyn JobContext, cancelled: bool) {
        if !cancelled
            && self.0.iter().any(|slot| slot.reader.is_some())
            && let Ok(mut flows) = super::acquire_flows(context, 1)
            && let Some(flow) = flows.pop()
        {
            self.end(context, &*flow);
            flow.finish(JobCounts::default());
        }
    }
    pub(super) fn end(&mut self, context: &dyn JobContext, flow: &dyn JobFlow) {
        for slot in &mut self.0 {
            if context.checkpoint() == Checkpoint::Cancel || flow.safe_point() == Checkpoint::Cancel
            {
                break;
            }
            if let Some(reader) = &slot.reader {
                let result = reader.read(&slot.report.volume_id);
                slot.report.end = Some(point(context, result));
                if flow.safe_point() == Checkpoint::Cancel {
                    break;
                }
            }
        }
    }
    pub(super) fn snapshot(&self) -> Vec<VolumeReport> {
        self.0.iter().map(|slot| slot.report.clone()).collect()
    }
}

fn point(
    context: &dyn JobContext,
    result: Result<du_core_contracts::scan_observation::VolumeSpace, PortError>,
) -> VolumeReportPoint {
    VolumeReportPoint {
        observed_at: context.utc_now(),
        value: result.map_err(|error| PortError {
            reason_key: "space.volume_report_unavailable".into(),
            ..error
        }),
    }
}
