//! Whether a folder row could not be read (F06 README 48, 53; #346).

use serde::{Deserialize, Deserializer, Serialize, de::Error as _};

/// A folder row's unread state, beside its value on the screen.
///
/// Only on item rows of folders in a real scan; omitted (not null) when nothing is
/// unread, on group and file rows, and for samples. The kind is an open set: a receiver
/// that meets one it does not know shows nothing, rather than claiming the folder was
/// or was not read. When both apply, `itself` wins: a folder whose listing failed
/// cannot tell what lies below it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VizUnread {
    /// The folder's own listing failed.
    Itself,
    /// The folder was read, but ranges below it were not.
    Inside {
        /// Unread ranges recorded below the folder, a positive u64 in decimal; omitted
        /// when a cancellation or disconnection left folders below it uncounted.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "positive_count"
        )]
        #[cfg_attr(feature = "typescript", ts(optional))]
        count: Option<String>,
    },
    /// A kind this receiver does not know.
    #[serde(other)]
    Unknown,
}

impl VizUnread {
    /// The state of a folder that was read with `count` unread ranges below it, or
    /// with an unknown number of them.
    pub fn inside(count: Option<u64>) -> Option<Self> {
        match count {
            Some(0) => None,
            count => Some(Self::Inside {
                count: count.map(|count| count.to_string()),
            }),
        }
    }
}

fn positive_count<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    let text = String::deserialize(d)?;
    let canonical = text.bytes().all(|byte| byte.is_ascii_digit())
        && !text.starts_with('0')
        && text.parse::<u64>().is_ok();
    if canonical {
        Ok(Some(text))
    } else {
        Err(D::Error::custom(
            "unread count is not a positive decimal u64",
        ))
    }
}
