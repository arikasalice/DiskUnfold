use du_core_contracts::{
    SizeValue, Timestamp, result::ResultItemKind, scan_observation::NativeModifiedTime,
};

mod attribute_reasons;
pub(crate) use attribute_reasons::AttributeReasons;
pub use attribute_reasons::ObservedAttribute;

pub(crate) struct StoredObservation {
    pub(crate) size: Option<SizeValue>,
    pub(crate) kind: ResultItemKind,
    pub(crate) modified_at: Option<Timestamp>,
    pub(crate) native_modified_at: Option<NativeModifiedTime>,
    pub(crate) enumerated_inode: Option<std::num::NonZeroU64>,
    pub(crate) placeholder_observation:
        Option<du_core_contracts::scan_observation::PlaceholderObservation>,
    pub(crate) observed_at: Option<Timestamp>,
}

/// Raw component names relative to the root; the root path is empty.
#[derive(Clone, Debug)]
pub struct Observation {
    pub path: Vec<Vec<u8>>,
    pub size: Option<SizeValue>,
}
