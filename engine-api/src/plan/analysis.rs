use std::sync::Arc;

use du_analysis_index::{IngestSink, ResultVersion};
use du_change_planner::ports::{AnalysisFactsPort, AnalysisItemFacts, TargetIdentity};
use du_core_contracts::{
    Id, ItemRef, OsPath, PathEncoding, SizeBasis,
    data_source::DataSource,
    ports::{PortError, PortErrorKind},
    result::ResultVersionRead,
};

use crate::{api::Channel, source_exposure::pin_current_for};

/// Pins one selected analysis version for the lifetime of its plan draft.
pub trait PlanAnalysisSource: Send + Sync {
    /// Returns an immutable fact reader for the exact selected current version.
    fn pin(&self, version: &Id) -> Result<Arc<dyn AnalysisFactsPort>, PortError>;
}

/// Production analysis source backed by the desktop's immutable result index.
pub struct IngestPlanAnalysisSource(IngestSink);

impl IngestPlanAnalysisSource {
    /// Creates a plan source over the desktop analysis index.
    pub fn new(source: IngestSink) -> Self {
        Self(source)
    }
}

impl PlanAnalysisSource for IngestPlanAnalysisSource {
    fn pin(&self, version: &Id) -> Result<Arc<dyn AnalysisFactsPort>, PortError> {
        let pinned = pin_current_for(&self.0, version, Channel::Gui).map_err(|_| PortError {
            kind: PortErrorKind::IdentityChanged,
            reason_key: "plan.result_stale".into(),
            os_code: None,
        })?;
        Ok(Arc::new(PinnedAnalysis(pinned)))
    }
}

struct PinnedAnalysis(Arc<ResultVersion>);

impl AnalysisFactsPort for PinnedAnalysis {
    fn data_source(&self, version: &Id) -> Result<DataSource, PortError> {
        if self.0.version_id() != version {
            return Err(unavailable("plan.result_stale"));
        }
        Ok(self.0.data_source())
    }

    fn read_item(&self, item: &ItemRef) -> Result<AnalysisItemFacts, PortError> {
        let detail = self
            .0
            .detail(item)
            .map_err(|_| unavailable("plan.result_stale"))?;
        let (root_index, root) = self
            .0
            .observed_root_for(item)
            .map_err(|_| unavailable("plan.item.path_unavailable"))?;
        let root_item = self
            .0
            .scope_roots()
            .get(root_index as usize)
            .ok_or_else(|| unavailable("plan.item.path_unavailable"))?;
        let (_, parts) = self
            .0
            .observed_path_parts(item)
            .map_err(|_| unavailable("plan.item.path_unavailable"))?;
        let native_path = self
            .0
            .native_path(item)
            .map_err(|_| unavailable("plan.item.path_unavailable"))?;
        let mut path_components = vec![
            root.path
                .raw
                .bytes()
                .map_err(|_| unavailable("plan.item.path_unavailable"))?,
        ];
        path_components.extend(parts.map(<[u8]>::to_vec));
        let raw = native_path
            .raw
            .bytes()
            .map_err(|_| unavailable("plan.item.path_unavailable"))?;
        let (display, lossy) = display_path(&raw, &native_path.encoding);
        let path = OsPath {
            encoding: native_path.encoding,
            raw: native_path.raw,
            display,
            lossy,
        };
        let identity = self
            .0
            .file_identity(item.item_no)
            .map(|identity| TargetIdentity {
                volume_id: identity.volume.clone(),
                file_id: identity.file.clone(),
            })
            .or_else(|| {
                (detail.item.item_no == root_item.item_no).then(|| TargetIdentity {
                    volume_id: root.volume_id.clone(),
                    file_id: root.file_id.clone(),
                })
            });
        Ok(AnalysisItemFacts {
            path,
            path_components,
            volume_id: root.volume_id.clone(),
            identity,
            kind: detail.item.kind,
            size: self.0.item_size(item.item_no, SizeBasis::Logical).cloned(),
            modified_at: self.0.item_modified_at(item.item_no).cloned(),
            classification_id: None,
        })
    }
}

fn display_path(raw: &[u8], encoding: &PathEncoding) -> (String, bool) {
    match encoding {
        PathEncoding::Unix => match std::str::from_utf8(raw) {
            Ok(value) => (value.to_owned(), false),
            Err(_) => (String::from_utf8_lossy(raw).into_owned(), true),
        },
        PathEncoding::WindowsUtf16le => {
            let (chunks, _) = raw.as_chunks::<2>();
            let words = chunks
                .iter()
                .map(|pair| u16::from_le_bytes(*pair))
                .collect::<Vec<_>>();
            let display = String::from_utf16_lossy(&words);
            let lossy = std::char::decode_utf16(words).any(|value| value.is_err());
            (display, lossy)
        }
    }
}

fn unavailable(reason_key: &str) -> PortError {
    PortError {
        kind: PortErrorKind::Io,
        reason_key: reason_key.to_owned(),
        os_code: None,
    }
}
