use webview2_com::{
    AddScriptToExecuteOnDocumentCreatedCompletedHandler,
    Microsoft::Web::WebView2::Win32::ICoreWebView2,
};
use windows::core::{HSTRING, PCWSTR};

pub(super) fn install_script(
    view: &ICoreWebView2,
    source: &'static str,
) -> Result<(), webview2_com::Error> {
    let view = view.clone();
    AddScriptToExecuteOnDocumentCreatedCompletedHandler::wait_for_async_operation(
        Box::new(move |handler| {
            let script = HSTRING::from(source);
            // SAFETY: The script and handler are valid for this STA call.
            unsafe { view.AddScriptToExecuteOnDocumentCreated(PCWSTR(script.as_ptr()), &handler) }?;
            Ok(())
        }),
        Box::new(|status, _| status),
    )
}
