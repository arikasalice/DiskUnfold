//! Field declarations for data considered by the sharing policy.

/// The nine published sharing classifications.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FieldClass {
    Secret,
    FileContent,
    PathOrName,
    ItemReference,
    ItemMetadata,
    Aggregate,
    ResultMetadata,
    ScopeMarker,
    FreeText,
}

/// One serialized JSON leaf path and its declared sharing classification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExportField {
    /// Exact JSON Pointer for the serialized leaf.
    pub path: &'static str,
    pub class: FieldClass,
}

/// Declares the complete serialized leaves of a type; it does not permit export.
pub trait Exportable {
    const FIELDS: &'static [ExportField];
}
