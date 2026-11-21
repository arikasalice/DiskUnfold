use du_core_contracts::settings::SettingsStoreError;

/// Error returned by settings operations.
#[derive(Debug, PartialEq, Eq)]
pub enum SettingsError {
    UnknownKey,
    NotEditable,
    InvalidValue,
    StaleVersion,
    StaleReview,
    Store(SettingsStoreError),
    Encoding,
    SchemaNewer,
    SchemaUnsupported,
    InvalidStoredValue,
    RestoreLocked,
}
