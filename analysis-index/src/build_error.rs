#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BuildError {
    Busy,
    InvalidTree,
    DuplicateVersion,
    CapacityExceeded,
    BaseSuperseded,
    Unavailable,
}

impl From<crate::observation_store::InsertError> for BuildError {
    fn from(error: crate::observation_store::InsertError) -> Self {
        match error {
            crate::observation_store::InsertError::Duplicate => Self::InvalidTree,
            crate::observation_store::InsertError::Capacity => Self::CapacityExceeded,
        }
    }
}
