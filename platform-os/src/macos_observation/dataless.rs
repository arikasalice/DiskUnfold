//! Thread-scoped refusal to materialize dataless (cloud placeholder) objects.
use super::errors;
use du_core_contracts::ports::PortError;
use std::ffi::c_int;

// Values from <sys/resource.h>; libc 0.2.191 does not export them.
const MATERIALIZE_DATALESS_FILES: c_int = 3;
const THREAD: c_int = 1;
const OFF: c_int = 1;

/// `SF_DATALESS` from <sys/stat.h>: the object has no local content.
pub(super) const SF_DATALESS: u32 = 0x4000_0000;

// SAFETY: both libSystem functions take and return plain integers and only read
// or change an I/O policy value; any argument combination is memory safe.
unsafe extern "C" {
    safe fn getiopolicy_np(iotype: c_int, scope: c_int) -> c_int;
    safe fn setiopolicy_np(iotype: c_int, scope: c_int, policy: c_int) -> c_int;
}

/// Runs `call` while this thread fails, rather than performs, materialization.
///
/// The previous thread policy is restored afterwards. When the policy cannot be
/// established, `call` is not run. A failed restore leaves the stricter policy.
pub(super) fn without_materialization<T>(call: impl FnOnce() -> T) -> Result<T, PortError> {
    let previous = getiopolicy_np(MATERIALIZE_DATALESS_FILES, THREAD);
    if previous < 0 {
        return Err(errors::last());
    }
    if previous == OFF {
        return Ok(call());
    }
    if setiopolicy_np(MATERIALIZE_DATALESS_FILES, THREAD, OFF) != 0 {
        return Err(errors::last());
    }
    let result = call();
    let _ = setiopolicy_np(MATERIALIZE_DATALESS_FILES, THREAD, previous);
    Ok(result)
}

/// Current thread policy, for tests that check the restore boundary.
#[cfg(test)]
pub(super) fn thread_policy() -> (c_int, c_int) {
    (getiopolicy_np(MATERIALIZE_DATALESS_FILES, THREAD), OFF)
}
