//! Owns the local clipboard write and keeps text out of the response envelope.
use du_core_contracts::{
    browse_ipc::{BrowseCopyPathOutcome, BrowseCopyPathRefusal, BrowseCopyPathRequest},
    ports::ClipboardPort,
    result::ResultNativePath,
};
use du_engine_api::{
    api::{CallerCtx, Channel, UseCase},
    browse::copy_path::{PathCopy, PreparedPathCopy},
    error::EngineError,
};

pub(crate) struct CopyPath<P, T> {
    source: PathCopy,
    clipboard: P,
    path_text: T,
    line_ending: &'static str,
}

impl<P, T> CopyPath<P, T> {
    /// Builds the local action for a dispatcher bound to the main webview.
    pub(crate) fn new(
        source: PathCopy,
        clipboard: P,
        path_text: T,
        line_ending: &'static str,
    ) -> Self {
        Self {
            source,
            clipboard,
            path_text,
            line_ending,
        }
    }
}

impl<P, T> UseCase for CopyPath<P, T>
where
    P: ClipboardPort + 'static,
    T: Fn(&ResultNativePath) -> Option<String> + Send + Sync + 'static,
{
    const NAME: &'static str = "browse.copy_path";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = BrowseCopyPathRequest;
    type Response = BrowseCopyPathOutcome;

    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        // The host registers this action only on the trusted main webview.
        Ok(())
    }

    fn execute(
        &self,
        request: Self::Request,
        _: &CallerCtx,
    ) -> Result<Self::Response, EngineError> {
        let prepared = self
            .source
            .prepare(request, self.line_ending, &self.path_text)?;
        Ok(write(prepared, &self.clipboard))
    }
}

fn write(
    prepared: Result<PreparedPathCopy, BrowseCopyPathRefusal>,
    clipboard: &impl ClipboardPort,
) -> BrowseCopyPathOutcome {
    let reason_key = match prepared {
        Err(reason) => reason,
        Ok(_) if !clipboard.can_write_text() => BrowseCopyPathRefusal::Unsupported,
        Ok(prepared) => {
            if clipboard.write_text(&prepared.text).is_ok() {
                return BrowseCopyPathOutcome::Copied {
                    count: prepared.count,
                };
            }
            BrowseCopyPathRefusal::Failed
        }
    };
    BrowseCopyPathOutcome::Refused { reason_key }
}

#[cfg(test)]
mod tests {
    use super::*;
    use du_core_contracts::ports::{PortError, PortErrorKind};
    use std::sync::Mutex;

    struct Clipboard {
        supported: bool,
        fail: bool,
        calls: Mutex<Vec<String>>,
    }

    impl ClipboardPort for Clipboard {
        fn can_write_text(&self) -> bool {
            self.supported
        }

        fn write_text(&self, text: &str) -> Result<(), PortError> {
            self.calls.lock().map_err(|_| failed())?.push(text.into());
            if self.fail { Err(failed()) } else { Ok(()) }
        }
    }

    fn failed() -> PortError {
        PortError {
            kind: PortErrorKind::Io,
            reason_key: "clipboard.failed".into(),
            os_code: None,
        }
    }

    #[test]
    fn records_only_complete_prepared_text_once_without_returning_it() -> Result<(), String> {
        use serde_json::json;
        let text = "C:\\한글 \r\nC:\\e\u{301}";
        for (supported, fail, expected) in [
            (true, false, json!({"outcome":"copied", "count":2})),
            (
                true,
                true,
                json!({"outcome":"refused", "reasonKey":"clipboard.failed"}),
            ),
            (
                false,
                false,
                json!({"outcome":"refused", "reasonKey":"clipboard.unsupported"}),
            ),
        ] {
            let clipboard = Clipboard {
                supported,
                fail,
                calls: Mutex::default(),
            };
            let outcome = write(
                Ok(PreparedPathCopy {
                    text: text.into(),
                    count: 2,
                }),
                &clipboard,
            );
            assert_eq!(
                serde_json::to_value(outcome).map_err(|e| e.to_string())?,
                expected
            );
            let expected_calls = if supported { vec![text] } else { vec![] };
            assert_eq!(
                *clipboard.calls.lock().map_err(|e| e.to_string())?,
                expected_calls
            );
        }
        Ok(())
    }

    #[test]
    fn preparation_refusal_skips_clipboard_entirely() -> Result<(), String> {
        let clipboard = Clipboard {
            supported: true,
            fail: false,
            calls: Mutex::default(),
        };
        let reason_key = BrowseCopyPathRefusal::Multiline;
        assert_eq!(
            write(Err(reason_key), &clipboard),
            BrowseCopyPathOutcome::Refused { reason_key }
        );
        assert!(
            clipboard
                .calls
                .lock()
                .map_err(|e| e.to_string())?
                .is_empty()
        );
        Ok(())
    }
}

#[cfg(test)]
mod request_tests;
