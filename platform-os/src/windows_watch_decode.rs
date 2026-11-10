//! Validation of completed ReadDirectoryChangesW records before delivery.
//!
//! This module does not own a native handle or report registration coverage.
use du_core_contracts::change_notify::{ChangeHint, WatchError};

/// One native relative UTF-16LE path and its unverified change hint.
#[derive(Debug, PartialEq, Eq)]
pub struct ChangeRecord {
    pub relative_path: Vec<u8>,
    pub hint: ChangeHint,
}

/// An incomplete, malformed or overflowing notification requires rescan.
#[derive(Debug, PartialEq, Eq)]
pub enum Completion {
    Changes(Vec<ChangeRecord>),
    Loss,
    Cancelled,
    Interrupted(WatchError),
}

/// Classifies the OS completion before parsing any records.
/// `status` is zero on success or the Win32 error code on failure.
pub fn decode_completion(status: u32, explicitly_stopping: bool, bytes: &[u8]) -> Completion {
    const ERROR_NOTIFY_ENUM_DIR: u32 = 1022;
    if explicitly_stopping {
        return Completion::Cancelled;
    }
    match status {
        ERROR_NOTIFY_ENUM_DIR => Completion::Loss,
        0 if bytes.is_empty() => Completion::Loss,
        0 => parse_records(bytes).map_or(Completion::Loss, Completion::Changes),
        _ => Completion::Interrupted(interruption_reason(status)),
    }
}

/// Maps an asynchronous Win32 failure to an IF-11 interruption reason.
pub(crate) fn interruption_reason(status: u32) -> WatchError {
    const ERROR_ACCESS_DENIED: u32 = 5;
    const ERROR_DEV_NOT_EXIST: u32 = 55;
    const ERROR_DEVICE_NOT_CONNECTED: u32 = 1167;
    match status {
        ERROR_ACCESS_DENIED => WatchError::AccessLost,
        ERROR_DEV_NOT_EXIST | ERROR_DEVICE_NOT_CONNECTED => WatchError::Disconnected,
        _ => WatchError::Other,
    }
}

fn word(bytes: &[u8], offset: usize) -> Option<u32> {
    let slice: [u8; 4] = bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?;
    Some(u32::from_le_bytes(slice))
}

fn parse_records(bytes: &[u8]) -> Option<Vec<ChangeRecord>> {
    let mut offset = 0usize;
    let mut records = Vec::new();
    loop {
        let next = usize::try_from(word(bytes, offset)?).ok()?;
        let action = word(bytes, offset.checked_add(4)?)?;
        let length = usize::try_from(word(bytes, offset.checked_add(8)?)?).ok()?;
        if length == 0 || length % 2 != 0 {
            return None;
        }
        let start = offset.checked_add(12)?;
        let end = start.checked_add(length)?;
        let path = bytes.get(start..end)?;
        if path.as_chunks::<2>().0.contains(&[0, 0]) {
            return None;
        }
        let hint = match action {
            1 => ChangeHint::Create,
            2 => ChangeHint::Delete,
            3 => ChangeHint::Modify,
            4 | 5 => ChangeHint::Rename,
            _ => ChangeHint::Unknown,
        };
        records.push(ChangeRecord {
            relative_path: path.to_vec(),
            hint,
        });
        if next == 0 {
            return Some(records);
        }
        if next % 4 != 0 || next < 12 + length {
            return None;
        }
        offset = offset.checked_add(next)?;
        if offset >= bytes.len() {
            return None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completion_status_does_not_turn_loss_or_stop_into_changes() {
        assert_eq!(decode_completion(0, false, &[]), Completion::Loss);
        assert_eq!(decode_completion(1022, false, &[1]), Completion::Loss);
        for (status, reason) in [
            (5, WatchError::AccessLost),
            (55, WatchError::Disconnected),
            (1167, WatchError::Disconnected),
            (995, WatchError::Other),
            (8, WatchError::Other),
        ] {
            assert_eq!(
                decode_completion(status, false, &[1]),
                Completion::Interrupted(reason)
            );
        }
        assert_eq!(decode_completion(995, true, &[]), Completion::Cancelled);
        assert_eq!(decode_completion(0, true, &[1]), Completion::Cancelled);
    }

    #[test]
    fn validates_record_lengths_and_preserves_native_name() {
        let mut one = vec![0, 0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0, b'x', 0];
        assert_eq!(
            decode_completion(0, false, &one),
            Completion::Changes(vec![ChangeRecord {
                relative_path: vec![b'x', 0],
                hint: ChangeHint::Create
            }])
        );
        one[8] = 4;
        assert_eq!(decode_completion(0, false, &one), Completion::Loss);
        one[8] = 1;
        assert_eq!(decode_completion(0, false, &one), Completion::Loss);
        one[8] = 2;
        one[0] = 4;
        assert_eq!(decode_completion(0, false, &one), Completion::Loss);
    }
}
