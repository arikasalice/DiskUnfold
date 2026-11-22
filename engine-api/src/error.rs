use du_core_contracts::{ErrorEnvelope, Id, ItemRef};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EngineError {
    reason: &'static str,
    internal_reason: Option<&'static str>,
    items: Vec<ItemRef>,
}

impl EngineError {
    pub fn new(reason: &'static str, items: Vec<ItemRef>) -> Self {
        Self {
            reason,
            internal_reason: None,
            items,
        }
    }

    pub(crate) fn reason(reason: &'static str) -> Self {
        Self::new(reason, Vec::new())
    }

    pub(crate) fn reason_with_internal(
        reason: &'static str,
        internal_reason: &'static str,
    ) -> Self {
        Self {
            reason,
            internal_reason: Some(internal_reason),
            items: Vec::new(),
        }
    }

    pub(crate) fn into_envelope(self, correlation_id: Id) -> ErrorEnvelope {
        let Self {
            reason,
            internal_reason,
            items,
        } = self;
        let _internal_reason = internal_reason;
        ErrorEnvelope::from_reason_or_internal(reason, correlation_id, items)
    }

    #[cfg(test)]
    pub(crate) fn internal_reason(&self) -> Option<&'static str> {
        self.internal_reason
    }
}
