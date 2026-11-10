//! Completion-owned Windows directory notification request.
//!
//! This low-level request does not establish scope identity or mount coverage.
use crate::windows_watch_decode::{Completion, decode_completion, interruption_reason};
use du_core_contracts::change_notify::WatchError;
use std::{
    fs::{File, OpenOptions},
    io,
    os::windows::{
        fs::OpenOptionsExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::Path,
};
use windows::{
    Win32::{
        Foundation::{ERROR_IO_INCOMPLETE, ERROR_NOT_FOUND, HANDLE},
        Storage::FileSystem::{
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OVERLAPPED, FILE_LIST_DIRECTORY,
            FILE_NOTIFY_CHANGE_ATTRIBUTES, FILE_NOTIFY_CHANGE_DIR_NAME,
            FILE_NOTIFY_CHANGE_FILE_NAME, FILE_NOTIFY_CHANGE_LAST_WRITE,
            FILE_NOTIFY_CHANGE_SECURITY, FILE_NOTIFY_CHANGE_SIZE, FILE_SHARE_DELETE,
            FILE_SHARE_READ, FILE_SHARE_WRITE, ReadDirectoryChangesW,
        },
        System::{
            IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED},
            Threading::{CreateEventW, ResetEvent},
        },
    },
    core::PCWSTR,
};

const NOTIFICATION_BYTES: usize = 64 * 1024;

/// The Windows notification API requires DWORD alignment for its output buffer.
#[repr(align(4))]
struct NotificationBuffer([u8; NOTIFICATION_BYTES]);

/// Owns the directory, event, stable request and buffer through completion.
pub struct NativeWatch {
    directory: File,
    event: OwnedHandle,
    request: Box<OVERLAPPED>,
    buffer: Box<NotificationBuffer>,
    pending: bool,
    stopped: bool,
    deferred_interruption: Option<WatchError>,
    #[cfg(test)]
    fail_next_submit: Option<i32>,
}

impl NativeWatch {
    /// Opens a directory and submits its first recursive notification request.
    /// The caller must independently establish volume and mount boundaries.
    pub fn start(path: &Path) -> io::Result<Self> {
        let directory = OpenOptions::new()
            .access_mode(FILE_LIST_DIRECTORY.0)
            .share_mode(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0 | FILE_SHARE_DELETE.0)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS.0 | FILE_FLAG_OVERLAPPED.0)
            .open(path)?;
        // SAFETY: The event has default security, starts unsignaled and has no name.
        let event_handle =
            unsafe { CreateEventW(None, true, false, PCWSTR::null()) }.map_err(to_io)?;
        // SAFETY: The successful event handle transfers to a single closing owner.
        let event = unsafe { OwnedHandle::from_raw_handle(event_handle.0) };
        let mut request = Box::<OVERLAPPED>::default();
        request.hEvent = event_handle;
        let mut watch = Self {
            directory,
            event,
            request,
            buffer: Box::new(NotificationBuffer([0; NOTIFICATION_BYTES])),
            pending: false,
            stopped: false,
            deferred_interruption: None,
            #[cfg(test)]
            fail_next_submit: None,
        };
        watch.submit()?;
        Ok(watch)
    }

    fn handle(&self) -> HANDLE {
        HANDLE(self.directory.as_raw_handle())
    }

    fn submit(&mut self) -> io::Result<()> {
        #[cfg(test)]
        if let Some(code) = self.fail_next_submit.take() {
            return Err(io::Error::from_raw_os_error(code));
        }
        *self.request = OVERLAPPED::default();
        self.request.hEvent = HANDLE(self.event.as_raw_handle());
        // SAFETY: The request and buffer are heap allocated, stay in self until
        // completion, and only one request uses this directory handle at a time.
        unsafe {
            ResetEvent(HANDLE(self.event.as_raw_handle())).map_err(to_io)?;
            ReadDirectoryChangesW(
                self.handle(),
                self.buffer.0.as_mut_ptr().cast(),
                NOTIFICATION_BYTES as u32,
                true,
                FILE_NOTIFY_CHANGE_FILE_NAME
                    | FILE_NOTIFY_CHANGE_DIR_NAME
                    | FILE_NOTIFY_CHANGE_ATTRIBUTES
                    | FILE_NOTIFY_CHANGE_SIZE
                    | FILE_NOTIFY_CHANGE_LAST_WRITE
                    | FILE_NOTIFY_CHANGE_SECURITY,
                None,
                Some(&mut *self.request),
                None,
            )
            .map_err(to_io)?;
        }
        self.pending = true;
        Ok(())
    }

    /// Collects a completed request without waiting, then submits the next one.
    /// If rearming fails, the completed evidence is returned first and the next
    /// poll reports one interruption. Callers must drain until `None`.
    pub fn poll(&mut self) -> io::Result<Option<Completion>> {
        if let Some(reason) = self.deferred_interruption.take() {
            self.stopped = true;
            return Ok(Some(Completion::Interrupted(reason)));
        }
        if self.stopped || !self.pending {
            return Ok(None);
        }
        let mut count = 0;
        // SAFETY: The request belongs to this live handle and remains allocated.
        let result =
            unsafe { GetOverlappedResult(self.handle(), &*self.request, &mut count, false) };
        let code = result
            .err()
            .map(|e| e.code().0 as u32 & 0xffff)
            .unwrap_or(0);
        if code == ERROR_IO_INCOMPLETE.0 {
            return Ok(None);
        }
        self.pending = false;
        let bytes = if code == 0 {
            self.buffer.0.get(..count as usize).unwrap_or_default()
        } else {
            &[]
        };
        let completion = decode_completion(code, false, bytes);
        Ok(Some(self.complete(completion)))
    }

    fn complete(&mut self, completion: Completion) -> Completion {
        if matches!(completion, Completion::Interrupted(_)) {
            self.stopped = true;
        } else if let Err(error) = self.submit() {
            // Deliver the completed evidence first; report interruption on the
            // next poll instead of silently discarding that evidence.
            let code = error.raw_os_error().unwrap_or_default() as u32;
            self.deferred_interruption = Some(interruption_reason(code));
        }
        completion
    }

    /// Fences delivery, cancels the exact request, and waits for its completion.
    pub fn stop(mut self) -> io::Result<()> {
        self.drain()
    }

    fn drain(&mut self) -> io::Result<()> {
        self.stopped = true;
        self.deferred_interruption = None;
        if !self.pending {
            return Ok(());
        }
        let handle = self.handle();
        let request = &*self.request;
        let result = cancel_then_reap(
            || {
                // SAFETY: The pointer identifies this live request and handle.
                unsafe { CancelIoEx(handle, Some(request)) }
                    .or_else(|error| {
                        if (error.code().0 as u32 & 0xffff) == ERROR_NOT_FOUND.0 {
                            Ok(())
                        } else {
                            Err(error)
                        }
                    })
                    .map_err(to_io)
            },
            || {
                let mut count = 0;
                // SAFETY: The live owner retains the request, buffer and handles.
                loop {
                    let completed =
                        unsafe { GetOverlappedResult(handle, request, &mut count, true) };
                    if completed.as_ref().err().is_none_or(|error| {
                        (error.code().0 as u32 & 0xffff) != ERROR_IO_INCOMPLETE.0
                    }) {
                        break;
                    }
                }
            },
        );
        self.pending = false;
        result
    }
}

fn cancel_then_reap(
    cancel: impl FnOnce() -> io::Result<()>,
    reap: impl FnOnce(),
) -> io::Result<()> {
    let result = cancel();
    reap();
    result
}

impl Drop for NativeWatch {
    fn drop(&mut self) {
        let _ = self.drain();
    }
}

fn to_io(error: windows::core::Error) -> io::Error {
    io::Error::from_raw_os_error((error.code().0 as u32 & 0xffff) as i32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs, thread,
        time::{Duration, Instant},
    };

    #[test]
    fn real_registration_change_and_release() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let mut watch = NativeWatch::start(directory.path())?;
        fs::write(directory.path().join("watch.txt"), b"x")?;
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut found = false;
        while Instant::now() < deadline {
            if let Some(Completion::Changes(changes)) = watch.poll()?
                && changes.iter().any(|c| {
                    c.relative_path
                        == "watch.txt"
                            .encode_utf16()
                            .flat_map(u16::to_le_bytes)
                            .collect::<Vec<_>>()
                })
            {
                found = true;
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert!(found);
        watch.stop()?;
        fs::write(directory.path().join("after.txt"), b"x")?;
        Ok(())
    }

    #[test]
    fn missing_directory_and_completion_cancel_race() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        assert!(NativeWatch::start(&directory.path().join("missing")).is_err());
        for index in 0..8 {
            let watch = NativeWatch::start(directory.path())?;
            fs::write(directory.path().join(format!("race-{index}")), b"x")?;
            watch.stop()?;
        }
        Ok(())
    }

    #[test]
    fn failed_cancel_still_reaps_before_returning() {
        let calls = std::cell::RefCell::new(Vec::new());
        let result = cancel_then_reap(
            || {
                calls.borrow_mut().push("cancel");
                Err(io::Error::other("injected"))
            },
            || calls.borrow_mut().push("reap"),
        );
        assert!(result.is_err());
        assert_eq!(*calls.borrow(), ["cancel", "reap"]);
    }

    #[test]
    fn rearm_failure_delivers_completed_change_then_one_interruption()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let mut watch = NativeWatch::start(directory.path())?;
        watch.fail_next_submit = Some(5);
        fs::write(directory.path().join("change"), b"x")?;
        let deadline = Instant::now() + Duration::from_secs(10);
        let completion = loop {
            if let Some(completion) = watch.poll()? {
                break completion;
            }
            if Instant::now() >= deadline {
                return Err("notification timed out".into());
            }
            thread::sleep(Duration::from_millis(10));
        };
        assert!(matches!(completion, Completion::Changes(_)));
        assert_eq!(
            watch.poll()?,
            Some(Completion::Interrupted(WatchError::AccessLost))
        );
        assert!(watch.poll()?.is_none());
        watch.stop()?;
        Ok(())
    }

    #[test]
    fn rearm_failure_preserves_loss_before_interruption() -> Result<(), Box<dyn std::error::Error>>
    {
        let directory = tempfile::tempdir()?;
        let mut watch = NativeWatch::start(directory.path())?;
        watch.drain()?;
        watch.stopped = false;
        watch.fail_next_submit = Some(1167);
        assert!(matches!(watch.complete(Completion::Loss), Completion::Loss));
        assert_eq!(
            watch.poll()?,
            Some(Completion::Interrupted(WatchError::Disconnected))
        );
        assert!(watch.poll()?.is_none());
        watch.stop()?;
        Ok(())
    }
}
