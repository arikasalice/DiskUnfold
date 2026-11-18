use super::*;

struct CorrelatedRoot {
    step: u8,
    phase: du_core_contracts::scan_observation::FilterCallbackPhase,
}

struct BudgetExhaustionRoot {
    step: u8,
    child: bool,
    exhaust: bool,
    budget: Arc<crate::scan::parallel::correlated_budget::CorrelatedBudget>,
}

fn directory_observation(name: &[u8]) -> du_core_contracts::scan_observation::TimedObservedEntry {
    use du_core_contracts::scan_observation::{MetadataObservation, ObservedKind};
    du_core_contracts::scan_observation::TimedObservedEntry::from(ObservedEntry {
        name: name.to_vec(),
        enumerated_inode: 7,
        enumerated_kind: ObservedKind::Directory,
        metadata: Ok(MetadataObservation {
            kind: ObservedKind::Directory,
            device: 1,
            inode: 7,
            size: Ok(0),
            allocated: None,
            link_count: None,
            space_identity: None,
            modified_at: None,
            native_modified_at: None,
        }),
    })
}

fn oversized_directory_observation(
    name: &[u8],
) -> du_core_contracts::scan_observation::TimedObservedEntry {
    let mut observation = directory_observation(name);
    if let Ok(metadata) = &mut observation.entry.metadata {
        metadata.space_identity = Some(du_core_contracts::scan_observation::SpaceIdentity {
            volume: vec![0; crate::scan::parallel::correlated_budget::MAX_EVENT_PAYLOAD_BYTES + 1],
            file: Vec::new(),
        });
    }
    observation
}

impl DirectoryObservations for BudgetExhaustionRoot {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        Err(PortError {
            kind: PortErrorKind::Io,
            reason_key: "unexpected.legacy".into(),
            os_code: None,
        })
    }

    fn next_filtered(
        &mut self,
        include: &mut dyn FnMut(&[u8]) -> FilterDecision,
    ) -> Result<FilteredBatch, PortError> {
        use du_core_contracts::scan_observation::ObservedKind;
        if !self.child || self.step > 0 {
            return Ok(FilteredBatch::End);
        }
        self.step += 1;
        if include(b"leaf") != FilterDecision::Admit {
            return Ok(FilteredBatch::Undecidable);
        }
        Ok(FilteredBatch::Entries(vec![ObservedEntry {
            name: b"leaf".to_vec(),
            enumerated_inode: 8,
            enumerated_kind: ObservedKind::File,
            metadata: Ok(du_core_contracts::scan_observation::MetadataObservation {
                kind: ObservedKind::File,
                device: 1,
                inode: 8,
                size: Ok(4),
                allocated: None,
                link_count: None,
                space_identity: None,
                modified_at: None,
                native_modified_at: None,
            }),
        }]))
    }

    fn begin_correlated_filtered(&mut self) -> Result<(), PortError> {
        if self.child {
            // The child is read through the ordinary filtered port.
            return Err(PortError {
                kind: PortErrorKind::Unsupported,
                reason_key: "os.unsupported".into(),
                os_code: None,
            });
        }
        Ok(())
    }

    fn next_correlated_event(
        &mut self,
        include: &mut dyn FnMut(&[u8]) -> FilterDecision,
        _: &dyn Fn() -> Option<du_core_contracts::Timestamp>,
    ) -> Result<du_core_contracts::scan_observation::CorrelatedDirectoryEvent, PortError> {
        use du_core_contracts::scan_observation::CorrelatedDirectoryEvent as Event;
        self.step += 1;
        match self.step {
            1 => Ok(Event::FilterCallback {
                child_id: 0,
                raw_name: b"folder".to_vec(),
                decision: include(b"folder"),
                phase: du_core_contracts::scan_observation::FilterCallbackPhase::BeforeMetadata,
            }),
            2 => {
                // Simulate competing scan work consuming all remaining evidence budget
                // after the adapter returned this observation.
                if self.exhaust {
                    self.budget.disable_for_test();
                }
                Ok(Event::Observation {
                    child_id: 0,
                    entry: directory_observation(b"folder"),
                })
            }
            _ => Ok(Event::End {
                child_count_exclusive: 1,
            }),
        }
    }
}

impl AnchoredDirectory for BudgetExhaustionRoot {
    fn observed_identity(&self) -> Option<du_core_contracts::scan_observation::HandleIdentity> {
        crate::scan::test_identity(self)
    }

    fn open_child(&self, _: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        Ok(Box::new(Self {
            step: 0,
            child: true,
            exhaust: self.exhaust,
            budget: Arc::clone(&self.budget),
        }))
    }
}

struct ReturnedDirectoryRoot {
    step: u8,
    child: bool,
}

/// Simulates an adapter that could not prove the child's identity, such as a
/// failed per-directory case query, and therefore withheld its metadata.
struct WithheldMetadataRoot {
    step: u8,
}

impl DirectoryObservations for WithheldMetadataRoot {
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
            CorrelatedDirectoryEvent as Event, ObservedKind,
        };
        self.step += 1;
        match self.step {
            1 => Ok(Event::FilterCallback {
                child_id: 0,
                raw_name: b"keep".to_vec(),
                decision: include(b"keep"),
                phase: du_core_contracts::scan_observation::FilterCallbackPhase::BeforeMetadata,
            }),
            2 => Ok(Event::Observation {
                child_id: 0,
                entry: ObservedEntry {
                    name: b"keep".to_vec(),
                    enumerated_inode: 9,
                    enumerated_kind: ObservedKind::File,
                    metadata: Err(PortError {
                        kind: PortErrorKind::IdentityChanged,
                        reason_key: "os.query_failed".into(),
                        os_code: None,
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

impl AnchoredDirectory for WithheldMetadataRoot {
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
fn withheld_correlated_metadata_keeps_the_item_and_does_not_prove_the_attempt()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = worker_test_lock();
    let ids = Ids(AtomicUsize::new(1));
    let context = Context {
        completed: Arc::new(AtomicUsize::new(0)),
        abandoned: Arc::new(AtomicUsize::new(0)),
    };
    let sink = IngestSink::default();
    let build = sink.open_forest(
        ids.next_id(),
        vec![super::super::observed_root("C:\\R0", 1)],
    );
    let outcome = crate::scan::parallel::collect_open_forest_filtered(
        build,
        vec![Box::new(WithheldMetadataRoot { step: 0 })],
        &ids,
        &context,
        1,
        &|_, _, _| FilterDecision::Admit,
        crate::scan::parallel::DevicePlan {
            groups: vec![0],
            limit: 1,
        },
    );
    assert!(outcome.result_version_id.is_some());
    let result = sink
        .pin()
        .map_err(|error| format!("{error:?}"))?
        .ok_or("missing")?;
    let item = result
        .items()
        .iter()
        .find(|item| item.name == b"keep")
        .ok_or("ordinary item was dropped")?;
    assert!(item.parent.is_some());
    assert!(
        result
            .scan_issues()
            .iter()
            .any(|issue| issue.relative == [b"keep".to_vec()])
    );
    assert_eq!(result.unread_folder_counts().cancelled, 0);
    let attempts = result.folder_attempt_summaries().ok_or("missing attempt")?;
    assert_eq!(attempts.len(), 1);
    assert_ne!(
        attempts[0].status,
        du_analysis_index::FolderAttemptStatus::EndedUnproven
    );
    Ok(())
}

impl DirectoryObservations for ReturnedDirectoryRoot {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        Err(PortError {
            kind: PortErrorKind::Io,
            reason_key: "unexpected.legacy".into(),
            os_code: None,
        })
    }

    fn next_filtered(
        &mut self,
        include: &mut dyn FnMut(&[u8]) -> FilterDecision,
    ) -> Result<FilteredBatch, PortError> {
        use du_core_contracts::scan_observation::ObservedKind;
        if !self.child || self.step > 0 {
            return Ok(FilteredBatch::End);
        }
        self.step += 1;
        if include(b"leaf") != FilterDecision::Admit {
            return Ok(FilteredBatch::Undecidable);
        }
        Ok(FilteredBatch::Entries(vec![ObservedEntry {
            name: b"leaf".to_vec(),
            enumerated_inode: 8,
            enumerated_kind: ObservedKind::File,
            metadata: Ok(du_core_contracts::scan_observation::MetadataObservation {
                kind: ObservedKind::File,
                device: 1,
                inode: 8,
                size: Ok(4),
                allocated: None,
                link_count: None,
                space_identity: None,
                modified_at: None,
                native_modified_at: None,
            }),
        }]))
    }

    fn begin_correlated_filtered(&mut self) -> Result<(), PortError> {
        Ok(())
    }

    fn next_correlated_event(
        &mut self,
        include: &mut dyn FnMut(&[u8]) -> FilterDecision,
        _: &dyn Fn() -> Option<du_core_contracts::Timestamp>,
    ) -> Result<du_core_contracts::scan_observation::CorrelatedDirectoryEvent, PortError> {
        use du_core_contracts::scan_observation::CorrelatedDirectoryEvent as Event;
        self.step += 1;
        match self.step {
            1 => Ok(Event::FilterCallback {
                child_id: 0,
                raw_name: b"folder".to_vec(),
                decision: include(b"folder"),
                phase: du_core_contracts::scan_observation::FilterCallbackPhase::BeforeMetadata,
            }),
            2 => Ok(Event::Observation {
                child_id: 0,
                entry: oversized_directory_observation(b"folder"),
            }),
            _ => Ok(Event::End {
                child_count_exclusive: 1,
            }),
        }
    }
}

impl AnchoredDirectory for ReturnedDirectoryRoot {
    fn observed_identity(&self) -> Option<du_core_contracts::scan_observation::HandleIdentity> {
        crate::scan::test_identity(self)
    }

    fn open_child(&self, _: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        Ok(Box::new(Self {
            step: 0,
            child: true,
        }))
    }
}

impl DirectoryObservations for CorrelatedRoot {
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
        use du_core_contracts::scan_observation::CorrelatedDirectoryEvent as Event;
        self.step += 1;
        match self.step {
            1 => Ok(Event::FilterCallback {
                child_id: 0,
                raw_name: b"keep".to_vec(),
                decision: include(b"keep"),
                phase: self.phase,
            }),
            2 => {
                let mut source = SmallRoot {
                    fail: false,
                    done: false,
                    name: b"keep".to_vec(),
                };
                let mut entries = source.next_batch()?;
                match entries.pop() {
                    Some(entry) => Ok(Event::Observation {
                        child_id: 0,
                        entry: entry.into(),
                    }),
                    None => Err(PortError {
                        kind: PortErrorKind::Io,
                        reason_key: "test.no_entry".into(),
                        os_code: None,
                    }),
                }
            }
            _ => Ok(Event::End {
                child_count_exclusive: 1,
            }),
        }
    }
}

impl AnchoredDirectory for CorrelatedRoot {
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
fn correlated_worker_records_observation_without_promoting_attempt()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = worker_test_lock();
    use du_core_contracts::scan_observation::FilterCallbackPhase;
    for phase in [
        FilterCallbackPhase::BeforeMetadata,
        FilterCallbackPhase::AfterMetadata,
    ] {
        let ids = Ids(AtomicUsize::new(1));
        let context = Context {
            completed: Arc::new(AtomicUsize::new(0)),
            abandoned: Arc::new(AtomicUsize::new(0)),
        };
        let sink = IngestSink::default();
        let build = sink.open_forest(
            ids.next_id(),
            vec![super::super::observed_root("C:\\R0", 1)],
        );
        let roots: Vec<Box<dyn AnchoredDirectory>> =
            vec![Box::new(CorrelatedRoot { step: 0, phase })];
        let outcome = crate::scan::parallel::collect_open_forest_filtered(
            build,
            roots,
            &ids,
            &context,
            1,
            &|_, _, _| FilterDecision::Admit,
            crate::scan::parallel::DevicePlan {
                groups: vec![0],
                limit: 4,
            },
        );
        assert!(outcome.result_version_id.is_some());
        let result = sink
            .pin()
            .map_err(|e| format!("{e:?}"))?
            .ok_or("missing result")?;
        assert_eq!(
            result
                .items()
                .iter()
                .filter(|item| item.name == b"keep")
                .count(),
            1
        );
        let attempts = result.folder_attempt_summaries().ok_or("missing attempt")?;
        assert_eq!(attempts.len(), 1);
        assert_eq!(
            attempts[0].status,
            du_analysis_index::FolderAttemptStatus::Held
        );
    }
    Ok(())
}

#[test]
fn path_budget_exhaustion_keeps_item_and_holds_correlated_attempt()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = worker_test_lock();
    let ids = Ids(AtomicUsize::new(1));
    let context = Context {
        completed: Arc::new(AtomicUsize::new(0)),
        abandoned: Arc::new(AtomicUsize::new(0)),
    };
    let sink = IngestSink::default();
    let budget = crate::scan::parallel::correlated_budget::CorrelatedBudget::with_limits(
        8,
        16 * 1024 * 1024,
    );
    let build = sink.open_forest(
        ids.next_id(),
        vec![super::super::observed_root("C:\\R0", 1)],
    );
    let outcome = crate::scan::parallel::collect_open_forest_filtered_with_budget(
        build,
        vec![Box::new(BudgetExhaustionRoot {
            step: 0,
            child: false,
            exhaust: true,
            budget: Arc::clone(&budget),
        })],
        &ids,
        &context,
        1,
        &|_, _, _| FilterDecision::Admit,
        (
            crate::scan::parallel::DevicePlan {
                groups: vec![0],
                limit: 1,
            },
            budget,
        ),
    );
    assert!(outcome.result_version_id.is_some());
    let result = sink
        .pin()
        .map_err(|error| format!("{error:?}"))?
        .ok_or("missing")?;
    assert_eq!(
        result.completeness(),
        du_core_contracts::result::ResultCompleteness::Complete
    );
    assert!(result.items().iter().any(|item| item.name == b"folder"));
    assert!(result.items().iter().any(|item| item.name == b"leaf"));
    assert_eq!(result.unread_folder_counts().cancelled, 0);
    assert_eq!(
        result
            .folder_attempt_summaries()
            .ok_or("missing attempt summary")?[0]
            .status,
        du_analysis_index::FolderAttemptStatus::Held
    );
    Ok(())
}

#[test]
fn oversized_directory_observation_holds_evidence_but_continues_f02_traversal()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = worker_test_lock();
    let ids = Ids(AtomicUsize::new(1));
    let sink = IngestSink::default();
    let context = Context {
        completed: Arc::new(AtomicUsize::new(0)),
        abandoned: Arc::new(AtomicUsize::new(0)),
    };
    let build = sink.open_forest(
        ids.next_id(),
        vec![super::super::observed_root("C:\\R0", 1)],
    );
    let outcome = crate::scan::parallel::collect_open_forest_filtered(
        build,
        vec![Box::new(ReturnedDirectoryRoot {
            step: 0,
            child: false,
        })],
        &ids,
        &context,
        1,
        &|_, _, _| FilterDecision::Admit,
        crate::scan::parallel::DevicePlan {
            groups: vec![0],
            limit: 1,
        },
    );
    assert!(outcome.result_version_id.is_some());
    let result = sink
        .pin()
        .map_err(|error| format!("{error:?}"))?
        .ok_or("missing")?;
    assert_eq!(
        result.completeness(),
        du_core_contracts::result::ResultCompleteness::Complete
    );
    assert!(result.items().iter().any(|item| item.name == b"folder"));
    assert!(result.items().iter().any(|item| item.name == b"leaf"));
    assert_eq!(result.unread_folder_counts().cancelled, 0);
    assert!(result.scan_issues().is_empty());
    assert!(
        result
            .folder_attempt_summaries()
            .ok_or("missing attempt summary")?
            .iter()
            .any(|summary| summary.status == du_analysis_index::FolderAttemptStatus::Held)
    );
    Ok(())
}

struct RejectChildAttempts(bool);

impl RejectChildAttempts {
    fn enable() -> Self {
        let previous = crate::scan::parallel::publisher::REJECT_CHILD_ATTEMPTS
            .with(|enabled| enabled.replace(true));
        Self(previous)
    }
}

impl Drop for RejectChildAttempts {
    fn drop(&mut self) {
        crate::scan::parallel::publisher::REJECT_CHILD_ATTEMPTS.with(|enabled| enabled.set(self.0));
    }
}

#[test]
fn refused_child_attempt_keeps_child_traversal_and_results()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = worker_test_lock();
    let _reject = RejectChildAttempts::enable();
    let ids = Ids(AtomicUsize::new(1));
    let context = Context {
        completed: Arc::new(AtomicUsize::new(0)),
        abandoned: Arc::new(AtomicUsize::new(0)),
    };
    let sink = IngestSink::default();
    let budget = crate::scan::parallel::correlated_budget::CorrelatedBudget::with_limits(
        8,
        16 * 1024 * 1024,
    );
    let build = sink.open_forest(
        ids.next_id(),
        vec![super::super::observed_root("C:\\R0", 1)],
    );
    let outcome = crate::scan::parallel::collect_open_forest_filtered_with_budget(
        build,
        vec![Box::new(BudgetExhaustionRoot {
            step: 0,
            child: false,
            exhaust: false,
            budget: Arc::clone(&budget),
        })],
        &ids,
        &context,
        1,
        &|_, _, _| FilterDecision::Admit,
        (
            crate::scan::parallel::DevicePlan {
                groups: vec![0],
                limit: 1,
            },
            budget,
        ),
    );
    assert!(outcome.result_version_id.is_some());
    let result = sink
        .pin()
        .map_err(|error| format!("{error:?}"))?
        .ok_or("missing")?;
    assert_eq!(
        result.completeness(),
        du_core_contracts::result::ResultCompleteness::Complete
    );
    assert!(result.items().iter().any(|item| item.name == b"folder"));
    assert!(result.items().iter().any(|item| item.name == b"leaf"));
    assert_eq!(result.unread_folder_counts().cancelled, 0);
    assert!(result.scan_issues().is_empty());
    // Only the root attempt was registered; the refused child kept ordinary traversal.
    assert_eq!(
        result
            .folder_attempt_summaries()
            .ok_or("missing attempt summary")?
            .len(),
        1
    );
    Ok(())
}
