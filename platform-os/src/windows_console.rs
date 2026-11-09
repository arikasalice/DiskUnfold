//! Supply terminal streams for Windows CLI mode when the launcher uses the GUI subsystem.

use windows::Win32::System::Console::{
    ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE,
};

pub fn attach_parent() {
    // SAFETY: This runs at CLI startup. Windows owns the inherited handles, and attaching
    // uses only the existing parent console. Redirected handles are inherited as-is.
    unsafe {
        let stdout = GetStdHandle(STD_OUTPUT_HANDLE).is_ok_and(|handle| !handle.is_invalid());
        let stderr = GetStdHandle(STD_ERROR_HANDLE).is_ok_and(|handle| !handle.is_invalid());
        if !stdout || !stderr {
            let _ = AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }
}
