//! F24 FR2.HD1.app-host.DD1: a metadata scan reports item progress.
use du_analysis_index::IngestSink;
use du_core_contracts::{
    Id,
    job::{Checkpoint, JobContext, JobCounts, JobFlow, JobFlowError, JobProgress},
    ports::{PortError, PortErrorKind},
    scan_observation::{
        AnchoredDirectory, DirectoryObservations, MetadataObservation, ObservedEntry, ObservedKind,
    },
};
use du_scan_engine::scan::{ResultIds, collect_parallel};
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};

struct Ids(AtomicUsize);
impl ResultIds for Ids {
    fn next_id(&self) -> Id {
        let n = self.0.fetch_add(1, Ordering::SeqCst);
        Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}")).unwrap_or_else(|_| unreachable!())
    }
}

/// Records every progress report the scan body makes.
#[derive(Default)]
struct Recording(Mutex<Vec<JobProgress>>);
impl JobContext for Recording {
    fn checkpoint(&self) -> Checkpoint {
        Checkpoint::Continue
    }
    fn try_start_flow(&self) -> Result<Box<dyn JobFlow>, JobFlowError> {
        Ok(Box::new(Flow))
    }
    fn report_progress(&self, progress: JobProgress) {
        if let Ok(mut reports) = self.0.lock() {
            reports.push(progress);
        }
    }
}
struct Flow;
impl JobFlow for Flow {
    fn report_item(&self, _: &[u8]) -> Result<(), JobFlowError> {
        Ok(())
    }
    fn finish(self: Box<Self>, _: JobCounts) {}
}

fn denied() -> PortError {
    PortError {
        kind: PortErrorKind::PermissionDenied,
        reason_key: "scan.path_denied".into(),
        os_code: None,
    }
}

fn file(name: u8, readable: bool) -> ObservedEntry {
    ObservedEntry {
        name: vec![name],
        enumerated_inode: u64::from(name),
        enumerated_kind: ObservedKind::File,
        metadata: if readable {
            Ok(MetadataObservation {
                kind: ObservedKind::File,
                device: 1,
                inode: u64::from(name),
                size: Ok(3),
                allocated: None,
                link_count: None,
                space_identity: None,
                modified_at: None,
                native_modified_at: None,
            })
        } else {
            Err(denied())
        },
    }
}

/// A root listing three readable files and one whose metadata is denied.
struct Root(Option<Vec<ObservedEntry>>);
impl DirectoryObservations for Root {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        Ok(self.0.take().unwrap_or_default())
    }
}
impl AnchoredDirectory for Root {
    fn open_child(&self, _: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        Err(denied())
    }
}

#[test]
fn recorded_and_failed_items_are_reported_with_an_unknown_total()
-> Result<(), Box<dyn std::error::Error>> {
    let ids = Ids(AtomicUsize::new(1));
    let sink = IngestSink::default();
    let context = Recording::default();
    let root = Root(Some(vec![
        file(b'a', true),
        file(b'b', true),
        file(b'c', false),
        file(b'd', true),
    ]));
    let outcome = collect_parallel(
        &sink,
        ids.next_id(),
        Box::new(root),
        &ids,
        &context,
        2,
        &|_| true,
    );
    assert_eq!((outcome.counts.completed, outcome.counts.failed), (3, 1));
    let reports = context.0.lock().map_err(|_| "reports")?.clone();
    assert!(reports.iter().all(|report| report.total.is_none()));
    assert!(
        reports
            .windows(2)
            .all(|pair| pair[0].completed <= pair[1].completed)
    );
    assert_eq!(reports.last().map(|report| report.completed), Some(4));
    Ok(())
}
