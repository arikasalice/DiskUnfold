//! Historical placeholder facts observed with an entry's metadata.

/// Facts observed by the scan, not a current locality or no-recall guarantee.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaceholderObservation {
    /// The Windows metadata handle did not report the known offline or recall-on-data flags.
    WindowsNoRecallDataFlags,
    /// The macOS metadata reported `SF_DATALESS`: the content is not on this device, and
    /// macOS reports no separate local occupancy, so the allocation is unknown.
    MacDataless,
}

impl PlaceholderObservation {
    /// True for a placeholder whose local occupancy the OS does not report (F03 FR6.HD1):
    /// its allocation is unknown and never taken as zero.
    pub fn local_occupancy_unknown(self) -> bool {
        matches!(self, Self::MacDataless)
    }
}
