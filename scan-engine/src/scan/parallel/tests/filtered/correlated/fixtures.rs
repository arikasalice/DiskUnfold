use super::super::*;
use du_core_contracts::scan_observation::ObservedValue;

pub(super) struct CorrelatedRoot {
    pub(super) step: u8,
    pub(super) phase: du_core_contracts::scan_observation::FilterCallbackPhase,
}

pub(super) struct BudgetExhaustionRoot {
    pub(super) step: u8,
    pub(super) child: bool,
    pub(super) exhaust: bool,
    pub(super) budget: Arc<crate::scan::parallel::correlated_budget::CorrelatedBudget>,
}

pub(super) fn directory_observation(
    name: &[u8],
) -> du_core_contracts::scan_observation::TimedObservedEntry {
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
            allocated: ObservedValue::NotApplicable,
            link_count: ObservedValue::NotApplicable,
            space_identity: ObservedValue::NotApplicable,
            modified_at: ObservedValue::NotApplicable,
            native_modified_at: None,
        }),
    })
}

pub(super) fn oversized_directory_observation(
    name: &[u8],
) -> du_core_contracts::scan_observation::TimedObservedEntry {
    let mut observation = directory_observation(name);
    if let Ok(metadata) = &mut observation.entry.metadata {
        metadata.space_identity =
            ObservedValue::Known(du_core_contracts::scan_observation::SpaceIdentity {
                volume: vec![
                    0;
                    crate::scan::parallel::correlated_budget::MAX_EVENT_PAYLOAD_BYTES + 1
                ],
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
                allocated: ObservedValue::NotApplicable,
                link_count: ObservedValue::NotApplicable,
                space_identity: ObservedValue::NotApplicable,
                modified_at: ObservedValue::NotApplicable,
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

pub(super) struct ReturnedDirectoryRoot {
    pub(super) step: u8,
    pub(super) child: bool,
}

/// Simulates an adapter that could not prove the child's identity, such as a
/// failed per-directory case query, and therefore withheld its metadata.
pub(super) struct WithheldMetadataRoot {
    pub(super) step: u8,
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
                allocated: ObservedValue::NotApplicable,
                link_count: ObservedValue::NotApplicable,
                space_identity: ObservedValue::NotApplicable,
                modified_at: ObservedValue::NotApplicable,
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
