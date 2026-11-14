//! Capability table of the sample source.
use du_core_contracts::{
    sample_ipc::{CAPABILITY_SAMPLE_EXAMPLE, CAPABILITY_SAMPLE_SOURCE},
    volume::{Capability, CapabilityState},
};

use crate::sample_bundle::ExampleCapabilities;

/// One row of the sample source capability table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SampleCapability {
    /// Directory enumeration.
    Enumerate,
    /// Metadata lookup.
    MetadataLookup,
    /// Volume identification.
    VolumeIdentity,
    /// File identification.
    FileIdentity,
    /// Content reading.
    ContentRead,
    /// Allocated size reporting (bundle example).
    AllocatedSize,
    /// Link detection (bundle example).
    LinkDetection,
    /// Placeholder detection (bundle example).
    PlaceholderDetection,
    /// Extra attributes (bundle example).
    ExtraAttributes,
    /// Moving to the trash.
    Trash,
    /// Moving within the same volume.
    SameVolumeMove,
    /// Deleting.
    Delete,
    /// Restoring.
    Restore,
    /// Opening in the file manager.
    OpenInFileManager,
    /// Previewing content.
    ContentPreview,
    /// Change notification.
    ChangeNotification,
}

impl SampleCapability {
    /// Every row of the table in presentation order.
    pub const ALL: [Self; 16] = [
        Self::Enumerate,
        Self::MetadataLookup,
        Self::VolumeIdentity,
        Self::FileIdentity,
        Self::ContentRead,
        Self::AllocatedSize,
        Self::LinkDetection,
        Self::PlaceholderDetection,
        Self::ExtraAttributes,
        Self::Trash,
        Self::SameVolumeMove,
        Self::Delete,
        Self::Restore,
        Self::OpenInFileManager,
        Self::ContentPreview,
        Self::ChangeNotification,
    ];
}

/// Looks up one row of the sample source capability table.
///
/// Read-only rows are supported with no reason. Example rows carry the state recorded in the
/// bundle and reason `sample.example`. Every operation that would act on the user's storage is
/// unsupported with reason `sample.source`.
pub fn sample_capability(row: SampleCapability, examples: &ExampleCapabilities) -> Capability {
    use SampleCapability::{
        AllocatedSize, ContentRead, Enumerate, ExtraAttributes, FileIdentity, LinkDetection,
        MetadataLookup, PlaceholderDetection, VolumeIdentity,
    };
    match row {
        Enumerate | MetadataLookup | VolumeIdentity | FileIdentity | ContentRead => Capability {
            state: CapabilityState::Supported,
            reason_key: None,
        },
        AllocatedSize => example(examples.allocated_size),
        LinkDetection => example(examples.link_detection),
        PlaceholderDetection => example(examples.placeholder_detection),
        ExtraAttributes => example(examples.extra_attributes),
        _ => Capability {
            state: CapabilityState::Unsupported,
            reason_key: Some(CAPABILITY_SAMPLE_SOURCE),
        },
    }
}

fn example(state: CapabilityState) -> Capability {
    Capability {
        state,
        reason_key: Some(CAPABILITY_SAMPLE_EXAMPLE),
    }
}
