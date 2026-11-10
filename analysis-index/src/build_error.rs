#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BuildError {
    Busy,
    InvalidTree,
    DuplicateVersion,
    CapacityExceeded,
    Unavailable,
}
