use du_core_contracts::{ErrorEnvelope, Id, ItemRef};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EngineError {
    reason: &'static str,
    items: Vec<ItemRef>,
}

impl EngineError {
    pub fn new(reason: &'static str, items: Vec<ItemRef>) -> Self {
        Self { reason, items }
    }

    pub(crate) fn reason(reason: &'static str) -> Self {
        Self::new(reason, Vec::new())
    }

    pub(crate) fn into_envelope(self, correlation_id: Id) -> ErrorEnvelope {
        ErrorEnvelope::from_reason_or_internal(self.reason, correlation_id, self.items)
    }
}
