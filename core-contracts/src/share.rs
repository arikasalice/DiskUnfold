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

/// A secret check returns only whether text is clean, contains a secret, or
/// could not be checked. It never returns secrets or their verifiers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SecretCheck {
    Clean,
    Contains,
    Unavailable,
}

/// Fixed secret set for one export; implementers clear it when dropped.
pub trait SecretCheckSnapshot: Send {
    /// Checks one free-text value against this export's fixed secret set.
    fn check(&self, text: &str) -> SecretCheck;
}

/// Opens one secret snapshot for an export operation.
pub trait SecretCheckPort: Send + Sync {
    /// Creates a fixed snapshot, or indicates that checking is unavailable.
    fn open_snapshot(&self) -> Result<Box<dyn SecretCheckSnapshot>, SecretCheckUnavailable>;
}

/// A secret snapshot could not be opened.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SecretCheckUnavailable;
