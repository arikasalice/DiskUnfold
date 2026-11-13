//! Home directory of the real user from the passwd database.
//!
//! `$HOME` is inherited and can be unset or redirected, which would point the
//! Full Disk Access probe at the wrong tree. `getpwuid_r` answers for the
//! process's real user id and is thread-safe.

use std::{
    ffi::{CStr, OsStr},
    os::unix::ffi::OsStrExt,
    path::PathBuf,
};

const INITIAL_BUFFER: usize = 1024;
const MAX_BUFFER: usize = 1 << 20;

/// Returns the passwd home directory of the real user, or `None` when the
/// lookup fails or yields an empty entry. Callers treat `None` as no evidence.
pub(super) fn home_directory() -> Option<PathBuf> {
    let mut size = INITIAL_BUFFER;
    while size <= MAX_BUFFER {
        let mut buffer = vec![0u8; size];
        let mut entry = std::mem::MaybeUninit::<libc::passwd>::uninit();
        let mut found: *mut libc::passwd = std::ptr::null_mut();
        // SAFETY: `entry`, `buffer` and `found` outlive the call. `buffer.len()`
        // is the exact capacity passed, so libc writes only inside it. On success
        // `found` points at `entry`, whose strings point into `buffer`; both stay
        // alive and unmodified while `pw_dir` is copied out below.
        let status = unsafe {
            libc::getpwuid_r(
                libc::getuid(),
                entry.as_mut_ptr(),
                buffer.as_mut_ptr().cast(),
                buffer.len(),
                &raw mut found,
            )
        };
        if status == libc::ERANGE {
            size = size.saturating_mul(2);
            continue;
        }
        if status != 0 || found.is_null() {
            return None;
        }
        // SAFETY: `getpwuid_r` returned success with a non-null `found`, so `entry`
        // is initialized and `pw_dir` is null or a NUL-terminated string in `buffer`.
        let directory = unsafe {
            let pw_dir = (*found).pw_dir;
            if pw_dir.is_null() {
                return None;
            }
            CStr::from_ptr(pw_dir).to_bytes().to_vec()
        };
        if directory.is_empty() {
            return None;
        }
        return Some(PathBuf::from(OsStr::from_bytes(&directory)));
    }
    None
}
