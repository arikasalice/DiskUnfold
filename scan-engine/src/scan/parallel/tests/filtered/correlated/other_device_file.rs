//! A file on another volume in a folder read through correlated evidence.
//!
//! The collector stores such a file, with or without kernel mount evidence, as
//! a boundary file and sends a boundary final, so the folder attempt ends
//! instead of being held, and the file is one unread range, not a failure.
use super::super::*;
use du_core_contracts::{
    result::ResultCompleteness,
    scan_observation::{MetadataObservation, ObservedKind},
};

/// A correlated root that lists one file whose metadata shows another device.
struct OtherDeviceFileRoot {
    step: u8,
    kind: ObservedKind,
}

impl DirectoryObservations for OtherDeviceFileRoot {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        Err(PortError {
            kind: PortErrorKind::Io,
            reason_key: "unexpected.legacy".into(),
            os_code: None,
        })
    }

    fn begin_correlated_filtered(&mut self) -> Result<(), PortError> {
        Ok(())
    }

    fn next_correlated_event(
        &mut self,
        include: &mut dyn FnMut(&[u8]) -> FilterDecision,
        _: &dyn Fn() -> Option<du_core_contracts::Timestamp>,
    ) -> Result<du_core_contracts::scan_observation::CorrelatedDirectoryEvent, PortError> {
        use du_core_contracts::scan_observation::{
            CorrelatedDirectoryEvent as Event, FilterCallbackPhase,
        };
        self.step += 1;
        match self.step {
            1 => Ok(Event::FilterCallback {
                child_id: 0,
                raw_name: b"foreign.bin".to_vec(),
                decision: include(b"foreign.bin"),
                phase: FilterCallbackPhase::BeforeMetadata,
            }),
            2 => Ok(Event::Observation {
                child_id: 0,
                // A mounted file is listed with the covered file's inode.
                entry: ObservedEntry {
                    name: b"foreign.bin".to_vec(),
                    enumerated_inode: 9,
                    enumerated_kind: ObservedKind::File,
                    metadata: Ok(MetadataObservation {
                        kind: self.kind.clone(),
                        device: 2,
                        inode: if self.kind == ObservedKind::MountedFile {
                            10
                        } else {
                            9
                        },
                        size: Err(PortError {
                            kind: PortErrorKind::Unsupported,
                            reason_key: "os.unsupported".into(),
                            os_code: None,
                        }),
                        allocated: None,
                        link_count: None,
                        space_identity: None,
                        modified_at: None,
                        native_modified_at: None,
                    }),
                }
                .into(),
            }),
            _ => Ok(Event::End {
                child_count_exclusive: 1,
            }),
        }
    }
}

impl AnchoredDirectory for OtherDeviceFileRoot {
    fn observed_identity(&self) -> Option<du_core_contracts::scan_observation::HandleIdentity> {
        crate::scan::test_identity(self)
    }

    fn open_child(&self, _: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        Err(PortError {
            kind: PortErrorKind::Unsupported,
            reason_key: "os.unsupported".into(),
            os_code: None,
        })
    }
}

#[test]
fn a_file_on_another_volume_ends_the_correlated_attempt_as_a_boundary()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = worker_test_lock();
    // Without and with kernel mount evidence.
    for kind in [ObservedKind::File, ObservedKind::MountedFile] {
        let ids = Ids(AtomicUsize::new(1));
        let context = Context {
            completed: Arc::new(AtomicUsize::new(0)),
            abandoned: Arc::new(AtomicUsize::new(0)),
        };
        let sink = IngestSink::default();
        let build = sink.open_forest(
            ids.next_id(),
            vec![super::super::super::observed_root("C:\\R0", 1)],
        );
        let root = OtherDeviceFileRoot {
            step: 0,
            kind: kind.clone(),
        };
        let outcome = crate::scan::parallel::collect_open_forest_filtered(
            build,
            vec![Box::new(root)],
            &ids,
            &context,
            1,
            &|_, _, _| FilterDecision::Admit,
            crate::scan::parallel::DevicePlan {
                groups: vec![0],
                limit: 1,
            },
        );
        assert!(outcome.result_version_id.is_some(), "{kind:?}");
        assert_eq!(outcome.counts.failed, 0, "{kind:?}");
        let result = sink
            .pin()
            .map_err(|error| format!("{error:?}"))?
            .ok_or("missing result")?;
        assert_eq!(
            result.completeness(),
            ResultCompleteness::Complete,
            "{kind:?}"
        );
        let item = result
            .items()
            .iter()
            .find(|item| item.name == b"foreign.bin")
            .ok_or("the boundary file was dropped")?;
        assert_eq!(
            (item.kind, item.size.is_some()),
            (du_core_contracts::result::ResultItemKind::File, false),
            "{kind:?}"
        );
        let rows: Vec<_> = result
            .scan_issues()
            .iter()
            .map(|issue| (issue.relative.clone(), issue.reason_key.clone()))
            .collect();
        assert_eq!(
            rows,
            [(
                vec![b"foreign.bin".to_vec()],
                du_core_contracts::scan_failures::unentered_reason::BOUNDARY_OTHER_VOLUME
                    .to_string()
            )],
            "{kind:?}"
        );
        let attempts = result.folder_attempt_summaries().ok_or("missing attempt")?;
        assert_eq!(attempts.len(), 1, "{kind:?}");
        assert_eq!(
            attempts[0].status,
            du_analysis_index::FolderAttemptStatus::EndedUnproven,
            "{kind:?}"
        );
    }
    Ok(())
}
