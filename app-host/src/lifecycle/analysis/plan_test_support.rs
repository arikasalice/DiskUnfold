//! Shared path fixtures for GUI plan boundary tests.

use du_core_contracts::{OsPath, PathBytes, PathEncoding, result::ResultItemKind};
use du_engine_api::plan::{AnalysisItemFacts, TargetIdentity};

pub(super) fn analysis_item_for_path(path: &std::path::Path) -> TestResult<AnalysisItemFacts> {
    let root = path.parent().ok_or("temporary file has no parent")?;
    let raw_path = native_path_bytes(path);
    let raw_root = native_path_bytes(root);
    let raw_name = native_path_bytes(path.file_name().ok_or("temporary file has no name")?);
    let encoding = if cfg!(windows) {
        PathEncoding::WindowsUtf16le
    } else {
        PathEncoding::Unix
    };
    Ok(AnalysisItemFacts {
        path: OsPath {
            encoding,
            raw: PathBytes::from_bytes(&raw_path),
            display: path.to_string_lossy().into_owned(),
            lossy: false,
        },
        path_components: vec![raw_root, raw_name],
        volume_id: b"fixture-volume".to_vec(),
        identity: Some(TargetIdentity {
            volume_id: b"fixture-volume".to_vec(),
            file_id: b"fixture-file".to_vec(),
        }),
        kind: ResultItemKind::File,
        size: None,
        modified_at: None,
        classification_id: None,
        descendants: du_core_contracts::protection_facts::ProtectionFact::Known(
            du_engine_api::plan::DescendantResult::Clear,
        ),
    })
}

fn native_path_bytes(path: impl AsRef<std::ffi::OsStr>) -> Vec<u8> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        path.as_ref()
            .encode_wide()
            .flat_map(u16::to_le_bytes)
            .collect()
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        path.as_ref().as_bytes().to_vec()
    }
}

pub(super) type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
