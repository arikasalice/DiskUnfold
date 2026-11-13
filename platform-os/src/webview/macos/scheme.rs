//! Custom `diskunfold://` scheme responses for the macOS webview.
use super::super::contract::{Asset, CSP, resource};
use objc2::{AnyThread, runtime::ProtocolObject};
use objc2_foundation::{NSData, NSDictionary, NSError, NSHTTPURLResponse, NSString};
use objc2_web_kit::WKURLSchemeTask;

/// Answers one scheme task synchronously from the embedded asset table.
///
/// # Safety
///
/// Call only from `startURLSchemeTask`, and complete the task before returning.
pub(super) unsafe fn serve(
    asset: fn(&str) -> Option<Asset>,
    task: &ProtocolObject<dyn WKURLSchemeTask>,
) {
    // SAFETY: Called only during startURLSchemeTask; complete synchronously before returning.
    unsafe {
        let request = task.request();
        let Some(url) = request.URL() else {
            task.didFailWithError(&NSError::errorWithDomain_code_userInfo(
                &NSString::from_str("DiskUnfold"),
                1,
                None,
            ));
            return;
        };
        let method = request
            .HTTPMethod()
            .map_or_else(String::new, |x| x.to_string());
        let absolute = url
            .absoluteString()
            .map_or_else(String::new, |x| x.to_string());
        let asset = resource(asset, &method, &absolute);
        let keys = [
            NSString::from_str("Content-Type"),
            NSString::from_str("Content-Security-Policy"),
            NSString::from_str("X-Content-Type-Options"),
            NSString::from_str("Access-Control-Allow-Origin"),
        ];
        let values = [
            NSString::from_str(asset.map_or("text/plain", |a| a.mime)),
            NSString::from_str(CSP),
            NSString::from_str("nosniff"),
            NSString::from_str("*"),
        ];
        let headers = NSDictionary::from_slices(
            &keys.each_ref().map(|x| &**x),
            &values.each_ref().map(|x| &**x),
        );
        if let Some(response) = NSHTTPURLResponse::initWithURL_statusCode_HTTPVersion_headerFields(
            NSHTTPURLResponse::alloc(),
            &url,
            if asset.is_some() { 200 } else { 403 },
            None,
            Some(&headers),
        ) {
            task.didReceiveResponse(&response);
            if let Some(asset) = asset {
                task.didReceiveData(&NSData::with_bytes(asset.bytes));
            }
            task.didFinish();
        } else {
            task.didFailWithError(&NSError::errorWithDomain_code_userInfo(
                &NSString::from_str("DiskUnfold"),
                1,
                None,
            ));
        }
    }
}
