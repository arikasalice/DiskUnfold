//! The value state of one optional attribute of an observed entry
//! (F02 FR2.HD1.app-host.DD1): known, unknown for a registered reason, or not
//! applicable to the entry's kind.
use super::{MetadataObservation, ObservedKind};
use crate::ports::PortError;

/// One optional attribute of a metadata observation.
///
/// The reason of [`ObservedValue::Unknown`] is one of the constants in
/// [`attribute_reason`]. It is a static key, so an entry that misses several
/// attributes allocates nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObservedValue<T> {
    Known(T),
    /// The entry has this attribute but the observation did not establish it.
    Unknown(&'static str),
    /// The entry's kind has no such value, for example the allocation of a
    /// directory. It carries no reason because nothing is missing.
    NotApplicable,
}

impl<T> ObservedValue<T> {
    /// The value when it is known.
    pub fn known(&self) -> Option<&T> {
        match self {
            Self::Known(value) => Some(value),
            Self::Unknown(_) | Self::NotApplicable => None,
        }
    }

    /// The value when it is known, consuming the state.
    pub fn into_known(self) -> Option<T> {
        match self {
            Self::Known(value) => Some(value),
            Self::Unknown(_) | Self::NotApplicable => None,
        }
    }

    /// Why the value is unknown; `None` when it is known or not applicable.
    pub fn reason(&self) -> Option<&'static str> {
        match self {
            Self::Unknown(reason) => Some(reason),
            Self::Known(_) | Self::NotApplicable => None,
        }
    }

    /// Maps a known value and keeps the other states.
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> ObservedValue<U> {
        match self {
            Self::Known(value) => ObservedValue::Known(f(value)),
            Self::Unknown(reason) => ObservedValue::Unknown(reason),
            Self::NotApplicable => ObservedValue::NotApplicable,
        }
    }

    /// A known value from `Some`, otherwise unknown for `reason`.
    pub fn or_unknown(value: Option<T>, reason: &'static str) -> Self {
        value.map_or(Self::Unknown(reason), Self::Known)
    }
}

impl<T: Clone> ObservedValue<T> {
    /// A copy of the value when it is known.
    pub fn get(&self) -> Option<T> {
        self.known().cloned()
    }
}

/// Registered reasons of an unknown attribute. The keys are shared with the
/// result layer, so a result can report them without a mapping.
pub mod attribute_reason {
    /// The OS call that reports the attribute failed or did not return it.
    pub const QUERY_FAILED: &str = "os.query_failed";
    /// The volume or the OS does not report the attribute.
    pub const UNSUPPORTED: &str = "os.attribute_unsupported";
    /// The entry is on another volume, so by rule only its identity is kept.
    pub const OTHER_VOLUME: &str = "scan.boundary_other_volume";
    /// The file is a placeholder whose local data is not downloaded.
    pub const PLACEHOLDER: &str = "space.placeholder_local_unknown";
}

impl MetadataObservation {
    /// The observation of an entry on another volume, or of a mount point or a
    /// mounted file: its kind and identity only. Its time and, for a file, its
    /// allocation, link count and space identity are withheld by rule.
    pub fn other_volume(kind: ObservedKind, device: u64, inode: u64, size: PortError) -> Self {
        fn withheld<T>(file: bool) -> ObservedValue<T> {
            if file {
                ObservedValue::Unknown(attribute_reason::OTHER_VOLUME)
            } else {
                ObservedValue::NotApplicable
            }
        }
        let file = matches!(kind, ObservedKind::File | ObservedKind::MountedFile);
        Self {
            kind,
            device,
            inode,
            size: Err(size),
            modified_at: ObservedValue::Unknown(attribute_reason::OTHER_VOLUME),
            native_modified_at: None,
            allocated: withheld(file),
            link_count: withheld(file),
            space_identity: withheld(file),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_known_value_is_read_and_only_an_unknown_one_has_a_reason() {
        let known = ObservedValue::Known(4_u64);
        let unknown = ObservedValue::<u64>::Unknown(attribute_reason::UNSUPPORTED);
        let absent = ObservedValue::<u64>::NotApplicable;
        assert_eq!((known.get(), known.reason()), (Some(4), None));
        assert_eq!(
            (unknown.get(), unknown.reason()),
            (None, Some("os.attribute_unsupported"))
        );
        assert_eq!((absent.get(), absent.reason()), (None, None));
        assert_eq!(known.map(|value| value * 2), ObservedValue::Known(8));
        assert_eq!(
            ObservedValue::or_unknown(None::<u64>, attribute_reason::QUERY_FAILED),
            ObservedValue::Unknown("os.query_failed")
        );
    }
}
