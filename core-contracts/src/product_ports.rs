//! Narrow operating-system ports behind the F15 product and support commands.
use crate::product_ipc::InstallForm;
use std::io::{self, Read};

/// Operating-system release and edition evidence used as support-matrix keys.
///
/// Encodings per family, which support-matrix rows must use:
/// Windows release is `<10|11>-<DisplayVersion>` (for example `11-25H2`) and
/// edition is the registry `EditionID`; macOS release is the major component of
/// the product version and edition is the lowercase product name; Linux release
/// is `VERSION_ID` and edition is `ID` from `os-release`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct OsReleaseEvidence {
    /// Release identifier, or `None` when it was not established.
    pub release: Option<String>,
    /// Edition identifier, or `None` when it was not established.
    pub edition: Option<String>,
}

/// Reads operating-system release and edition without prompting.
///
/// Implementations return `None` for any value they cannot establish. They must
/// not fall back to a default, guess from the CPU, or reuse another platform's value.
pub trait OsReleasePort: Send + Sync {
    /// Returns the observed release and edition.
    fn release_and_edition(&self) -> OsReleaseEvidence;
}

/// Detects how the running executable was installed.
pub trait InstallFormPort: Send + Sync {
    /// Returns the install form, or `InstallForm::Unknown` when the evidence is insufficient.
    fn install_form(&self) -> InstallForm;
}

/// Opens the running process's own executable for reading.
pub trait SelfExecutablePort: Send + Sync {
    /// Opens the executable image. Errors mean the digest is unavailable.
    fn open(&self) -> io::Result<Box<dyn Read + Send>>;
}
