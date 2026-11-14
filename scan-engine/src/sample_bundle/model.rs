//! Immutable records decoded from a validated sample bundle.
use du_core_contracts::{Timestamp, volume::CapabilityState};

/// Kind of one sample item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SampleItemKind {
    /// A directory that may contain other items.
    Directory,
    /// A regular file.
    File,
    /// A link that the sample scan reports without traversing.
    Link,
}

/// Observation state example recorded for one item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SampleObservationState {
    /// Reading the item was denied (valid for directories and files).
    AccessDenied,
    /// The item disappeared before its metadata could be read (files only).
    Vanished,
    /// The link was listed but not followed (links only).
    LinkUntraversed,
    /// The item exists but its logical size could not be determined (files only).
    SizeUnknown,
}

/// One entry of an observation point, addressed by its path components.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SampleItem {
    /// Path components below the sample root. Never empty.
    pub path: Vec<String>,
    /// Item kind.
    pub kind: SampleItemKind,
    /// Logical size in bytes; zero for directories and links.
    pub size: u64,
    /// Example allocated size, when the bundle records one.
    pub allocated: Option<u64>,
    /// Last modification time, when the bundle records one.
    pub modified_at: Option<Timestamp>,
    /// Observation state example, when the item demonstrates one.
    pub state: Option<SampleObservationState>,
    /// Index into the bundle's synthetic content fragments (files only).
    pub content: Option<usize>,
    /// Scenario that the item belongs to.
    pub scenario: String,
}

/// One observation point: the sample tree as seen at `observed_at`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodedObservation {
    /// Zero-based position in the bundle.
    pub index: u32,
    /// Time the observation represents.
    pub observed_at: Timestamp,
    /// Items in parent-before-child order.
    pub items: Vec<SampleItem>,
}

/// A scenario the bundle demonstrates.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SampleScenario {
    /// Stable scenario identifier.
    pub id: String,
    /// Fixed English description for authors and tests.
    pub summary: String,
}

/// Capability values the bundle records as examples (state only).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExampleCapabilities {
    /// Allocated size reporting.
    pub allocated_size: CapabilityState,
    /// Link detection.
    pub link_detection: CapabilityState,
    /// Placeholder detection.
    pub placeholder_detection: CapabilityState,
    /// Extra attributes.
    pub extra_attributes: CapabilityState,
}

impl Default for ExampleCapabilities {
    fn default() -> Self {
        Self {
            allocated_size: CapabilityState::Unsupported,
            link_detection: CapabilityState::Unsupported,
            placeholder_detection: CapabilityState::Unsupported,
            extra_attributes: CapabilityState::Unsupported,
        }
    }
}

/// Example protection rule shown in the sample cleanup plan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProtectionRule {
    /// Stable rule identifier.
    pub id: String,
    /// Path components of the protected item or directory.
    pub path: Vec<String>,
    /// Reason key explaining the protection.
    pub reason_key: String,
}

/// Cleanup operation a candidate is offered for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CandidateOperation {
    /// Move to the trash.
    Trash,
    /// Move within the same volume.
    Move,
    /// Delete permanently.
    Delete,
}

/// A sample cleanup candidate offered for one operation kind.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CleanupCandidate {
    /// Operation the candidate is offered for.
    pub operation: CandidateOperation,
    /// Path components of an item in the last observation.
    pub path: Vec<String>,
}
