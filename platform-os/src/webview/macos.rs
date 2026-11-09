use super::contract::{Asset, CSP, WebApp, entry, message, resource};
use block2::DynBlock;
use objc2::{
    AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class,
    rc::{Retained, autoreleasepool},
    runtime::{AnyObject, ProtocolObject},
};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSImage, NSWindow,
    NSWindowDelegate, NSWindowStyleMask,
};
use objc2_foundation::{
    NSData, NSDictionary, NSError, NSHTTPURLResponse, NSNotification, NSObject, NSObjectProtocol,
    NSPoint, NSRect, NSSize, NSString, NSURL, NSURLRequest,
};
use objc2_web_kit::{
    WKContentWorld, WKNavigation, WKNavigationAction, WKNavigationActionPolicy,
    WKNavigationDelegate, WKScriptMessage, WKScriptMessageHandlerWithReply, WKURLSchemeHandler,
    WKURLSchemeTask, WKUserContentController, WKWebView, WKWebViewConfiguration,
    WKWebsiteDataStore,
};
use std::{cell::RefCell, error::Error, ptr};

struct State {
    app: WebApp,
    error: RefCell<Option<String>>,
}

define_class!(
    // SAFETY: All protocols below are invoked on the AppKit main thread. The
    // retained delegate lives until the window closes and handlers are removed.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = State]
    struct Handler;

    unsafe impl NSObjectProtocol for Handler {}

    unsafe impl NSWindowDelegate for Handler {
        #[unsafe(method(windowWillClose:))]
        fn window_will_close(&self, _: &NSNotification) {
            NSApplication::sharedApplication(self.mtm()).stop(None);
        }
    }

    unsafe impl WKURLSchemeHandler for Handler {
        #[unsafe(method(webView:startURLSchemeTask:))]
        unsafe fn start_scheme_task(
            &self,
            _: &WKWebView,
            task: &ProtocolObject<dyn WKURLSchemeTask>,
        ) {
            // SAFETY: These objects and the synchronously completed task remain valid in this callback.
            unsafe {
                serve(self.ivars().app.asset, task);
            }
        }

        #[unsafe(method(webView:stopURLSchemeTask:))]
        unsafe fn stop_scheme_task(&self, _: &WKWebView, _: &ProtocolObject<dyn WKURLSchemeTask>) {
            // Responses complete synchronously. There is no outstanding work.
        }
    }

    unsafe impl WKNavigationDelegate for Handler {
        #[unsafe(method(webView:decidePolicyForNavigationAction:decisionHandler:))]
        unsafe fn decide_navigation(
            &self,
            _: &WKWebView,
            action: &WKNavigationAction,
            reply: &DynBlock<dyn Fn(WKNavigationActionPolicy)>,
        ) {
            // SAFETY: The action and block are borrowed from this main-thread callback.
            let permitted = unsafe {
                action
                    .targetFrame()
                    .is_some_and(|frame| frame.isMainFrame())
                    && request_url(&action.request()).is_some_and(|url| url == entry())
                    && !action.shouldPerformDownload()
            };
            reply.call((if permitted {
                WKNavigationActionPolicy::Allow
            } else {
                WKNavigationActionPolicy::Cancel
            },));
        }

        #[unsafe(method(webView:didFailProvisionalNavigation:withError:))]
        unsafe fn load_failed(&self, _: &WKWebView, _: Option<&WKNavigation>, error: &NSError) {
            // Navigation policy cancellation is expected for blocked external links.
            if error.code() != -999 {
                *self.ivars().error.borrow_mut() = Some(error.localizedDescription().to_string());
                NSApplication::sharedApplication(self.mtm()).stop(None);
            }
        }
    }

    unsafe impl WKScriptMessageHandlerWithReply for Handler {
        #[unsafe(method(userContentController:didReceiveScriptMessage:replyHandler:))]
        unsafe fn receive_message(
            &self,
            _: &WKUserContentController,
            request: &WKScriptMessage,
            reply: &DynBlock<dyn Fn(*mut AnyObject, *mut NSString)>,
        ) {
            // SAFETY: WKWebView provides the authoritative frame metadata on this callback.
            let result = unsafe {
                let frame = request.frameInfo();
                let body = request.body();
                request_url(&frame.request()).and_then(|url| {
                    let data = body.downcast_ref::<NSString>()?.to_string();
                    message(&self.ivars().app.dispatch, frame.isMainFrame(), &url, &data)
                })
            };
            match result {
                Some(response) => {
                    let response = NSString::from_str(&response);
                    reply.call((
                        Retained::as_ptr(&response).cast_mut().cast(),
                        ptr::null_mut(),
                    ));
                }
                None => {
                    let error = NSString::from_str("DiskUnfold IPC request denied");
                    reply.call((ptr::null_mut(), Retained::as_ptr(&error).cast_mut()));
                }
            }
        }
    }
);

fn request_url(request: &NSURLRequest) -> Option<String> {
    Some(request.URL()?.absoluteString()?.to_string())
}

unsafe fn serve(asset: fn(&str) -> Option<Asset>, task: &ProtocolObject<dyn WKURLSchemeTask>) {
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

pub(super) fn run(app: WebApp) -> Result<(), Box<dyn Error>> {
    let mtm = MainThreadMarker::new().ok_or("AppKit requires the main thread")?;
    autoreleasepool(|_| {
        // SAFETY: AppKit and WebKit are initialized and operated exclusively on the main thread.
        unsafe {
            let application = NSApplication::sharedApplication(mtm);
            application.setActivationPolicy(NSApplicationActivationPolicy::Regular);
            if let Some(icon) =
                NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(app.icon))
            {
                application.setApplicationIconImage(Some(&icon));
            }
            let handler = objc2::msg_send![
                super(Handler::alloc(mtm).set_ivars(State {
                    app,
                    error: RefCell::new(None)
                })),
                init
            ];
            let handler: Retained<Handler> = handler;
            let config = WKWebViewConfiguration::new(mtm);
            config.setWebsiteDataStore(&WKWebsiteDataStore::nonPersistentDataStore(mtm));
            config.setURLSchemeHandler_forURLScheme(
                Some(ProtocolObject::from_ref(&*handler)),
                &NSString::from_str("diskunfold"),
            );
            let manager = config.userContentController();
            let world = WKContentWorld::pageWorld(mtm);
            let channel = NSString::from_str("diskunfold");
            manager.addScriptMessageHandlerWithReply_contentWorld_name(
                ProtocolObject::from_ref(&*handler),
                &world,
                &channel,
            );
            let bounds = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(900.0, 600.0));
            let view =
                WKWebView::initWithFrame_configuration(WKWebView::alloc(mtm), bounds, &config);
            view.setNavigationDelegate(Some(ProtocolObject::from_ref(&*handler)));
            let window = NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                bounds,
                NSWindowStyleMask::Titled
                    | NSWindowStyleMask::Closable
                    | NSWindowStyleMask::Resizable,
                NSBackingStoreType::Buffered,
                false,
            );
            window.setReleasedWhenClosed(false);
            window.setTitle(&NSString::from_str("DiskUnfold"));
            window.setContentView(Some(&view));
            window.setDelegate(Some(ProtocolObject::from_ref(&*handler)));
            let url = NSURL::URLWithString(&NSString::from_str(&entry()))
                .ok_or("Invalid embedded page URL")?;
            view.loadRequest(&NSURLRequest::requestWithURL(&url));
            window.center();
            window.makeKeyAndOrderFront(None);
            application.activate();
            application.run();
            manager.removeScriptMessageHandlerForName_contentWorld(&channel, &world);
            view.setNavigationDelegate(None);
            window.setDelegate(None);
            match handler.ivars().error.take() {
                Some(error) => Err(error.into()),
                None => Ok(()),
            }
        }
    })
}
