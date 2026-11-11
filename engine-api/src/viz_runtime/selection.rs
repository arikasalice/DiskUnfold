//! Trusted window selection input for opt-in F06 result binding.
use du_core_contracts::Id;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SelectionError {
    Unavailable,
}

/// Resolves the result revision already selected by the owning desktop window.
///
/// Implementations must not infer a selection from the result store or request
/// contents. The runtime calls this only after validating the view channel.
pub trait WindowScanSelection: Send + Sync + 'static {
    fn selected_result_revision(&self, view_channel: &Id) -> Result<Option<Id>, SelectionError>;
}
