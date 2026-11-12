//! Validated, deterministic release distribution inventory generation.
mod model;
mod render;
#[cfg(test)]
mod tests;
mod validation;

pub use model::*;
pub use render::{render_checksums, render_json, render_markdown};
pub use validation::{prepare_inventory, validate_and_sort};

impl ReleaseInventory {
    /// Returns the product name represented by this normalized inventory.
    pub fn product_name(&self) -> &str {
        &self.product_name
    }

    /// Returns the release version represented by this normalized inventory.
    pub fn version(&self) -> &str {
        &self.version
    }

    /// Returns distribution rows in format, OS range, and CPU order.
    pub fn rows(&self) -> &[DistributionRow] {
        &self.rows
    }

    /// Returns Store and winget references in deterministic order.
    pub fn channel_references(&self) -> &[ChannelReference] {
        &self.channel_references
    }
}

impl VerifiedFile {
    /// Returns the artifact row associated with this staged file.
    pub fn artifact_file_name(&self) -> &str {
        &self.artifact_file_name
    }

    /// Returns the staged package or executable filename.
    pub fn file_name(&self) -> &str {
        &self.file_name
    }

    /// Returns the staged file role.
    pub fn kind(&self) -> StagedFileKind {
        self.kind
    }

    /// Returns the digest recomputed from the staged path.
    pub fn sha256(&self) -> &Sha256Digest {
        &self.sha256
    }
}

impl DistributionChannel {
    fn as_str(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::Store => "store",
            Self::Winget => "winget",
        }
    }
}

impl UpdateChannel {
    fn as_str(self) -> &'static str {
        match self {
            Self::Store => "store",
            Self::Winget => "winget",
        }
    }
}

impl SignatureMethod {
    fn as_str(self) -> &'static str {
        match self {
            Self::SignPath => "SignPath",
            Self::Store => "Store signing",
            Self::HardenedRuntimeAdHoc => "Hardened Runtime ad-hoc",
            Self::Unsigned => "unsigned",
        }
    }
}

impl UpdateMethod {
    fn as_str(self) -> &'static str {
        match self {
            Self::Automatic => "automatic",
            Self::ChannelCommand => "channel command",
            Self::ReplaceFile => "direct replacement",
        }
    }
}
