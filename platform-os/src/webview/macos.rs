mod menu;
mod scheme;
mod titlebar;

use super::contract::{ShutdownCallbacks, WebApp, entry, message};
use crate::cli_link::Language;
use block2::DynBlock;
use objc2::{
    AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, class, define_class, msg_send,
    rc::{Retained, autoreleasepool},
    runtime::{AnyObject, Bool, ProtocolObject},
    sel,
};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate,
    NSApplicationTerminateReply, NSBackingStoreType, NSEvent, NSEventModifierFlags, NSEventType,
    NSImage, NSWindow, NSWindowDelegate, NSWindowStyleMask,
};
use objc2_foundation::{
    NSData, NSError, NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString,
    NSURL, NSURLRequest,
};
use objc2_web_kit::{
    WKContentWorld, WKNavigation, WKNavigationAction, WKNavigationActionPolicy,
    WKNavigationDelegate, WKScriptMessage, WKScriptMessageHandler, WKScriptMessageHandlerWithReply,
    WKURLSchemeHandler, WKURLSchemeTask, WKUserContentController, WKUserScript,
    WKUserScriptInjectionTime, WKWebView, WKWebViewConfiguration, WKWebsiteDataStore,
};
use std::{
    cell::{Cell, RefCell},
    error::Error,
    ptr,
};

struct State {
    app: WebApp,
    /// Language of the native alerts, taken from the application's display language.
    language: Language,
    shutdown: Option<ShutdownCallbacks>,
    error: RefCell<Option<String>>,
    /// The page view, kept so window messages and full-screen changes can reach its window.
    view: RefCell<Option<Retained<WKWebView>>>,
    /// True while a Dock or menu quit is postponed with `TerminateLater` and
    /// AppKit still waits for `replyToApplicationShouldTerminate:`.
    terminate_pending: Cell<bool>,
}

/// Interval of the shutdown progress timer, matching the Windows and Linux tickers.
const SHUTDOWN_TICK_SECONDS: f64 = 0.016;

#[link(name = "Foundation", kind = "framework")]
unsafe extern "C" {
    /// The run-loop mode set that covers tracking, modal and default modes.
    static NSRunLoopCommonModes: &'static NSString;
}

/// Clears the application delegate on every exit path, including early returns
/// and unwinding, so AppKit never keeps a pointer to the released handler.
struct DelegateGuard(Retained<NSApplication>);

impl Drop for DelegateGuard {
    fn drop(&mut self) {
        self.0.setDelegate(None);
    }
}

/// Invalidates the shutdown timer on every exit path.
struct TimerGuard(Retained<AnyObject>);

impl Drop for TimerGuard {
    fn drop(&mut self) {
        // SAFETY: `invalidate` is sent to the NSTimer this guard retains, on the main thread.
        unsafe {
            let () = msg_send![&*self.0, invalidate];
        }
    }
}

/// Schedules the shutdown ticker in the common run-loop modes.
///
/// A postponed quit (`TerminateLater`) runs the loop in a modal mode, and a window
/// resize or menu tracking uses an event-tracking mode. A timer in the default mode
/// alone would stall in both and the shutdown would never advance.
fn schedule_shutdown_timer(handler: &Handler) -> Retained<AnyObject> {
    // SAFETY: Called on the main thread; the timer retains `handler` until invalidated.
    unsafe {
        let timer: Retained<AnyObject> = msg_send![
            class!(NSTimer),
            timerWithTimeInterval: SHUTDOWN_TICK_SECONDS,
            target: handler,
            selector: sel!(shutdownTick:),
            userInfo: Option::<&AnyObject>::None,
            repeats: Bool::YES
        ];
        let run_loop: Retained<AnyObject> = msg_send![class!(NSRunLoop), currentRunLoop];
        let () = msg_send![&*run_loop, addTimer: &*timer, forMode: NSRunLoopCommonModes];
        timer
    }
}

/// Ends `NSApplication::run` from outside an event handler.
///
/// `stop:` only takes effect after the next event, so a timer callback must post
/// one for the loop to observe the request.
fn stop_application(application: &NSApplication) {
    application.stop(None);
    let wake = NSEvent::otherEventWithType_location_modifierFlags_timestamp_windowNumber_context_subtype_data1_data2(
        NSEventType::ApplicationDefined,
        NSPoint::new(0.0, 0.0),
        NSEventModifierFlags::empty(),
        0.0,
        0,
        None,
        0,
        0,
        0,
    );
    if let Some(wake) = wake {
        application.postEvent_atStart(&wake, true);
    }
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
        /// Window close is a normal exit: it asks the host to shut down in order and
        /// keeps the window open until the coordinator reports completion.
        #[unsafe(method(windowShouldClose:))]
        fn window_should_close(&self, _: &NSWindow) -> Bool {
            match &self.ivars().shutdown {
                Some(shutdown) => {
                    shutdown.request_window_close();
                    Bool::new(shutdown.is_complete())
                }
                None => Bool::YES,
            }
        }

        #[unsafe(method(windowWillClose:))]
        fn window_will_close(&self, _: &NSNotification) {
            NSApplication::sharedApplication(self.mtm()).stop(None);
        }

        /// The native title bar is hidden in full screen, so the page's bar collapses.
        #[unsafe(method(windowDidEnterFullScreen:))]
        fn window_did_enter_full_screen(&self, _: &NSNotification) {
            publish_titlebar(self.ivars());
        }

        #[unsafe(method(windowDidExitFullScreen:))]
        fn window_did_exit_full_screen(&self, _: &NSNotification) {
            publish_titlebar(self.ivars());
        }
    }

    unsafe impl NSApplicationDelegate for Handler {
        /// Dock and menu quit use the same orderly path and never terminate early.
        ///
        /// While the shutdown runs the quit is postponed with `TerminateLater`; the
        /// tick answers it when the shutdown finishes. A quit that waits for the
        /// user's confirmation is cancelled because nothing is running to answer it.
        #[unsafe(method(applicationShouldTerminate:))]
        fn application_should_terminate(&self, _: &NSApplication) -> NSApplicationTerminateReply {
            match &self.ivars().shutdown {
                Some(shutdown) => {
                    shutdown.request_window_close();
                    if shutdown.is_complete() {
                        NSApplicationTerminateReply::TerminateNow
                    } else if shutdown.is_closing() {
                        self.ivars().terminate_pending.set(true);
                        NSApplicationTerminateReply::TerminateLater
                    } else {
                        NSApplicationTerminateReply::TerminateCancel
                    }
                }
                None => NSApplicationTerminateReply::TerminateNow,
            }
        }
    }

    impl Handler {
        /// Menu action: links the `diskunfold` command into the user's `~/.local/bin`.
        #[unsafe(method(installCommandLineTool:))]
        fn install_command_line_tool(&self, _: &AnyObject) {
            menu::install_command_line_tool(self.mtm(), self.ivars().language);
        }

        /// Advances the host shutdown and leaves the run loop once it completes.
        #[unsafe(method(shutdownTick:))]
        fn shutdown_tick(&self, _: &AnyObject) {
            if let Some(shutdown) = &self.ivars().shutdown {
                shutdown.pump();
                if shutdown.is_complete() {
                    let application = NSApplication::sharedApplication(self.mtm());
                    if self.ivars().terminate_pending.replace(false) {
                        // Answer the postponed quit with a cancel and end the loop through
                        // `stop:`. A terminate reply makes AppKit call exit() inside the
                        // loop, which would skip the host's exit status and cleanup.
                        application.replyToApplicationShouldTerminate(false);
                    }
                    stop_application(&application);
                }
            }
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
                scheme::serve(self.ivars().app.asset, task);
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

        /// The buttons are laid out once the window is on screen, so the page gets their
        /// measured space after the first load.
        #[unsafe(method(webView:didFinishNavigation:))]
        unsafe fn navigation_finished(&self, _: &WKWebView, _: Option<&WKNavigation>) {
            publish_titlebar(self.ivars());
        }
    }

    unsafe impl WKScriptMessageHandler for Handler {
        /// Window messages from the page's bar: `drag` moves the window with the mouse press
        /// that is being handled and `zoom` zooms it. Only the app document's main frame may
        /// send them.
        #[unsafe(method(userContentController:didReceiveScriptMessage:))]
        unsafe fn receive_window_message(
            &self,
            _: &WKUserContentController,
            message: &WKScriptMessage,
        ) {
            // SAFETY: WKWebView provides the frame metadata and body on this callback.
            let (trusted, body) = unsafe {
                let frame = message.frameInfo();
                (
                    frame.isMainFrame()
                        && request_url(&frame.request()).is_some_and(|url| url == entry()),
                    message
                        .body()
                        .downcast_ref::<NSString>()
                        .map(ToString::to_string),
                )
            };
            if !trusted {
                return;
            }
            let Some(window) = self
                .ivars()
                .view
                .borrow()
                .as_ref()
                .and_then(|view| view.window())
            else {
                return;
            };
            match body.as_deref() {
                Some("drag") => {
                    if let Some(event) = NSApplication::sharedApplication(self.mtm()).currentEvent()
                    {
                        window.performWindowDragWithEvent(&event);
                    }
                }
                Some("zoom") => window.zoom(None),
                _ => {}
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

/// Publishes the measured title bar space to the page's CSS custom properties.
fn publish_titlebar(state: &State) {
    let view = state.view.borrow();
    let Some(view) = view.as_ref() else {
        return;
    };
    let Some(window) = view.window() else {
        return;
    };
    let script = titlebar::publish_script(titlebar::metrics(&window));
    // SAFETY: Called on the main thread with a valid script string and no completion handler.
    unsafe { view.evaluateJavaScript_completionHandler(&NSString::from_str(&script), None) };
}

pub(super) fn run(app: WebApp, shutdown: Option<ShutdownCallbacks>) -> Result<(), Box<dyn Error>> {
    let mtm = MainThreadMarker::new().ok_or("AppKit requires the main thread")?;
    autoreleasepool(|_| {
        // SAFETY: AppKit and WebKit are initialized and operated exclusively on the main thread.
        unsafe {
            let application = NSApplication::sharedApplication(mtm);
            let locale_enabled = app.locale_enabled;
            let language = Language::from_tag(&app.native_language);
            application.setActivationPolicy(NSApplicationActivationPolicy::Regular);
            if let Some(icon) =
                NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(app.icon))
            {
                application.setApplicationIconImage(Some(&icon));
            }
            let handler = objc2::msg_send![
                super(Handler::alloc(mtm).set_ivars(State {
                    app,
                    language,
                    shutdown,
                    error: RefCell::new(None),
                    view: RefCell::new(None),
                    terminate_pending: Cell::new(false)
                })),
                init
            ];
            let handler: Retained<Handler> = handler;
            let has_shutdown = handler.ivars().shutdown.is_some();
            if has_shutdown {
                application.setDelegate(Some(ProtocolObject::from_ref(&*handler)));
            }
            let _delegate = has_shutdown.then(|| DelegateGuard(application.clone()));
            menu::install_main_menu(mtm, &application, &handler, has_shutdown);
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
            let window = NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                bounds,
                NSWindowStyleMask::Titled
                    | NSWindowStyleMask::Closable
                    | NSWindowStyleMask::Resizable
                    | titlebar::style_mask(),
                NSBackingStoreType::Buffered,
                false,
            );
            titlebar::configure(&window);
            let window_channel = NSString::from_str("diskunfoldWindow");
            manager
                .addScriptMessageHandler_name(ProtocolObject::from_ref(&*handler), &window_channel);
            let bar_script = WKUserScript::initWithSource_injectionTime_forMainFrameOnly(
                WKUserScript::alloc(mtm),
                &NSString::from_str(&titlebar::marker_script(titlebar::metrics(&window))),
                WKUserScriptInjectionTime::AtDocumentStart,
                true,
            );
            manager.addUserScript(&bar_script);
            if locale_enabled {
                let source = NSString::from_str(
                    "if (window === window.top && location.href === 'diskunfold://app/index.html') { Object.defineProperty(window, '__diskunfoldLocale', { value: true }); }",
                );
                let script = WKUserScript::initWithSource_injectionTime_forMainFrameOnly(
                    WKUserScript::alloc(mtm),
                    &source,
                    WKUserScriptInjectionTime::AtDocumentStart,
                    true,
                );
                manager.addUserScript(&script);
            }
            let view =
                WKWebView::initWithFrame_configuration(WKWebView::alloc(mtm), bounds, &config);
            view.setNavigationDelegate(Some(ProtocolObject::from_ref(&*handler)));
            *handler.ivars().view.borrow_mut() = Some(view.clone());
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
            let _timer = has_shutdown.then(|| TimerGuard(schedule_shutdown_timer(&handler)));
            application.run();
            manager.removeScriptMessageHandlerForName_contentWorld(&channel, &world);
            manager.removeScriptMessageHandlerForName(&window_channel);
            view.setNavigationDelegate(None);
            handler.ivars().view.borrow_mut().take();
            window.setDelegate(None);
            match handler.ivars().error.take() {
                Some(error) => Err(error.into()),
                None => Ok(()),
            }
        }
    })
}
