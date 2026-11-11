#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ResultOrigin {
    Scan,
    Incremental,
}

impl ResultOrigin {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Scan => "scan",
            Self::Incremental => "incremental",
        }
    }
}
