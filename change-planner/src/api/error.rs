use du_core_contracts::ports::PortError;

/// Errors raised before a plan version can be returned.
#[derive(Debug)]
pub enum PlanError {
    /// The request contains no selected items.
    Empty,
    /// The request exceeds the fixed 100,000-item limit.
    TooManyItems,
    /// A source kind is not admitted on the request channel.
    SourceChannelDenied,
    /// A selected item does not belong to the pinned source version.
    SourceNotCurrent,
    /// A required read-only port failed.
    PortFailure {
        /// Boundary that failed.
        boundary: &'static str,
        /// Original port error.
        error: PortError,
    },
    /// A source path could not be decoded into its native byte form.
    InvalidSourcePath,
    /// Plan ID allocation failed.
    IdUnavailable(PortError),
    /// The clock did not provide a valid UTC time.
    ClockUnavailable,
    /// The stored parent version was unavailable or corrupt.
    ParentUnavailable,
    /// The stored plan hash did not match its canonical content.
    StoredHashMismatch,
    /// A confirmation summary exceeded the supported unsigned range.
    ConfirmationOverflow,
}

impl std::fmt::Display for PlanError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => formatter.write_str("plan.empty"),
            Self::TooManyItems => formatter.write_str("plan.too_many_items"),
            Self::SourceChannelDenied => formatter.write_str("plan.source_channel_denied"),
            Self::SourceNotCurrent => formatter.write_str("plan.source_not_current"),
            Self::PortFailure { boundary, error } => {
                write!(formatter, "{boundary}: {}", error.reason_key)
            }
            Self::InvalidSourcePath => formatter.write_str("plan.item.invalid_path"),
            Self::IdUnavailable(error) => {
                write!(formatter, "plan.id_unavailable: {}", error.reason_key)
            }
            Self::ClockUnavailable => formatter.write_str("plan.clock_unavailable"),
            Self::ParentUnavailable => formatter.write_str("plan.parent_unavailable"),
            Self::StoredHashMismatch => formatter.write_str("db.corrupt"),
            Self::ConfirmationOverflow => formatter.write_str("plan.confirmation_overflow"),
        }
    }
}

impl std::error::Error for PlanError {}
