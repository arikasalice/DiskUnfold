use super::*;
use du_core_contracts::{
    ByteSize, Id, ItemRef, OsPath, PathBytes, PathEncoding, SizeAmount, SizeBasis, SizeEvidence,
    SizeValue, Timestamp, data_source::DataSource, result::ResultItemKind,
};
use du_engine_api::plan::{AnalysisFactsPort, PlanAnalysisSource, TargetIdentity};
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Clone)]
pub(in crate::lifecycle::analysis) struct FakeAnalysis {
    version_id: Id,
    data_source: DataSource,
    calls: Arc<AtomicUsize>,
    block_on_call: Option<usize>,
    entered: Option<mpsc::Sender<()>>,
    release: Option<Arc<Mutex<mpsc::Receiver<()>>>>,
    path: Option<std::path::PathBuf>,
    trash_facts: bool,
}

impl FakeAnalysis {
    pub(in crate::lifecycle::analysis) fn real() -> Self {
        Self::new(DataSource::Real, None, None, None)
    }

    pub(in crate::lifecycle::analysis) fn new(
        data_source: DataSource,
        block_on_call: Option<usize>,
        entered: Option<mpsc::Sender<()>>,
        release: Option<Arc<Mutex<mpsc::Receiver<()>>>>,
    ) -> Self {
        Self {
            version_id: Id::from_v7_bytes([1; 16]),
            data_source,
            calls: Arc::new(AtomicUsize::new(0)),
            block_on_call,
            entered,
            release,
            path: None,
            trash_facts: false,
        }
    }

    pub(in crate::lifecycle::analysis) fn with_path(mut self, path: std::path::PathBuf) -> Self {
        self.path = Some(path);
        self
    }

    pub(in crate::lifecycle::analysis) fn with_trash_facts(mut self) -> Self {
        self.trash_facts = true;
        self
    }

    pub(in crate::lifecycle::analysis) fn item_ref(&self, item_no: u32) -> ItemRef {
        ItemRef {
            result_version: self.version_id.clone(),
            item_no,
        }
    }
}

impl PlanAnalysisSource for FakeAnalysis {
    fn pin(
        &self,
        version: &Id,
    ) -> Result<Arc<dyn AnalysisFactsPort>, du_core_contracts::ports::PortError> {
        if version != &self.version_id {
            return Err(port_error("plan.result_stale"));
        }
        Ok(Arc::new(self.clone()))
    }
}

impl AnalysisFactsPort for FakeAnalysis {
    fn data_source(&self, version: &Id) -> Result<DataSource, du_core_contracts::ports::PortError> {
        (version == &self.version_id)
            .then_some(self.data_source)
            .ok_or_else(|| port_error("plan.result_stale"))
    }

    fn read_item(
        &self,
        item: &ItemRef,
    ) -> Result<AnalysisItemFacts, du_core_contracts::ports::PortError> {
        if item.result_version != self.version_id || item.item_no == 0 {
            return Err(port_error("plan.result_stale"));
        }
        let call = self.calls.fetch_add(1, Ordering::AcqRel) + 1;
        if self.block_on_call == Some(call) {
            if let Some(entered) = &self.entered {
                let _ = entered.send(());
            }
            if let Some(release) = &self.release {
                release
                    .lock()
                    .map_err(|_| port_error("support.internal_error"))?
                    .recv()
                    .map_err(|_| port_error("support.internal_error"))?;
            }
        }
        if let Some(path) = &self.path {
            return super::plan_test_support::analysis_item_for_path(path)
                .map_err(|_| port_error("plan.source_not_current"));
        }
        if self.trash_facts {
            let name = format!("file-{}", item.item_no);
            let path = format!("/fixture/{name}").into_bytes();
            let modified_at = Timestamp::from_unix_millis(123)
                .ok_or_else(|| port_error("support.internal_error"))?;
            return Ok(AnalysisItemFacts {
                path: OsPath {
                    encoding: PathEncoding::Unix,
                    raw: PathBytes::from_bytes(&path),
                    display: String::from_utf8_lossy(&path).into_owned(),
                    lossy: false,
                },
                path_components: vec![b"/fixture".to_vec(), name.into_bytes()],
                volume_id: b"fixture-volume".to_vec(),
                identity: Some(TargetIdentity {
                    volume_id: b"fixture-volume".to_vec(),
                    file_id: format!("fixture-file-{}", item.item_no).into_bytes(),
                }),
                kind: ResultItemKind::File,
                size: Some(SizeValue {
                    basis: SizeBasis::Logical,
                    evidence: SizeEvidence::Measured,
                    amount: SizeAmount::Exact {
                        bytes: ByteSize::from(12),
                    },
                }),
                modified_at: Some(modified_at),
                classification_id: Some("fixture.cleanable".into()),
                descendants: Fact::Known(DescendantResult::Clear),
            });
        }
        Ok(AnalysisItemFacts {
            path: OsPath {
                encoding: PathEncoding::Unix,
                raw: PathBytes::from_bytes(b"/fixture/file"),
                display: "/fixture/file".into(),
                lossy: false,
            },
            path_components: vec![b"/fixture".to_vec(), b"file".to_vec()],
            volume_id: b"fixture-volume".to_vec(),
            identity: Some(TargetIdentity {
                volume_id: b"fixture-volume".to_vec(),
                file_id: b"fixture-file".to_vec(),
            }),
            kind: ResultItemKind::File,
            size: None,
            modified_at: None,
            classification_id: Some("fixture.cleanable".into()),
            descendants: Fact::Known(DescendantResult::Clear),
        })
    }
}
