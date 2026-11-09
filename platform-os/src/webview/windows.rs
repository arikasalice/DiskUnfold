mod icon;

use super::contract::{Asset, CSP, Dispatch, WebApp, entry, message, resource};
use std::{
    cell::{Cell, RefCell},
    error::Error,
    os::windows::ffi::OsStrExt,
    path::PathBuf,
    rc::Rc,
};
use webview2_com::{
    AddScriptToExecuteOnDocumentCreatedCompletedHandler, CoreWebView2EnvironmentOptions,
    CreateCoreWebView2ControllerCompletedHandler, CreateCoreWebView2EnvironmentCompletedHandler,
    DownloadStartingEventHandler, Microsoft::Web::WebView2::Win32::*,
    NavigationCompletedEventHandler, NavigationStartingEventHandler,
    NewWindowRequestedEventHandler, PermissionRequestedEventHandler,
    WebMessageReceivedEventHandler, WebResourceRequestedEventHandler, take_pwstr,
};
use windows::{
    Win32::{
        Foundation::{E_POINTER, HWND, RECT},
        System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize},
        UI::Shell::SHCreateMemStream,
    },
    core::{BOOL, HSTRING, Interface, PCWSTR, PWSTR, w},
};

const WM_CLOSE: u32 = 0x0010;
const BRIDGE: &str = r#"(() => {
    if (location.href !== 'https://diskunfold.invalid/index.html' || window !== window.top) return;
    const channel = chrome.webview;
    const pending = new Map();
    let sequence = 0;
    channel.addEventListener('message', ({data}) => {
        if (typeof data !== 'string') return;
        const split = data.indexOf('\n');
        const id = data.slice(0, split);
        const request = pending.get(id);
        if (!request) return;
        pending.delete(id);
        clearTimeout(request.timer);
        const response = data.slice(split + 1);
        if (response) request.resolve(response);
        else request.reject(new Error('DiskUnfold IPC request denied'));
    });
    Object.defineProperty(window, '__diskunfoldRequest', { value: (payload) => new Promise((resolve, reject) => {
        const id = String(++sequence);
        const timer = setTimeout(() => {
            pending.delete(id);
            reject(new Error('DiskUnfold IPC request timed out'));
        }, 10000);
        pending.set(id, { resolve, reject, timer });
        channel.postMessage(id + '\n' + payload);
    }) });
})();"#;

struct Apartment;
impl Drop for Apartment {
    fn drop(&mut self) {
        // SAFETY: run initialized this STA on the same thread.
        unsafe { CoUninitialize() };
    }
}

struct Controller(ICoreWebView2Controller);
impl Drop for Controller {
    fn drop(&mut self) {
        // SAFETY: The controller is owned by the UI thread and the parent still exists.
        let _ = unsafe { self.0.Close() };
    }
}

fn environment() -> Result<ICoreWebView2Environment, Box<dyn Error>> {
    let base = std::env::var_os("DISKUNFOLD_DATA_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("LOCALAPPDATA").map(|path| PathBuf::from(path).join("DiskUnfold"))
        })
        .ok_or("No Windows application data directory")?;
    let path = base.join("webview");
    std::fs::create_dir_all(&path)?;
    let data = HSTRING::from_wide(&path.as_os_str().encode_wide().collect::<Vec<_>>());
    let options: ICoreWebView2EnvironmentOptions = CoreWebView2EnvironmentOptions::default().into();
    let slot = Rc::new(Cell::new(None));
    let output = slot.clone();
    CreateCoreWebView2EnvironmentCompletedHandler::wait_for_async_operation(
        Box::new(move |handler| {
            // SAFETY: The inputs remain valid for the call on the initialized STA.
            unsafe {
                CreateCoreWebView2EnvironmentWithOptions(
                    PCWSTR::null(),
                    PCWSTR(data.as_ptr()),
                    &options,
                    &handler,
                )
            }?;
            Ok(())
        }),
        Box::new(move |status, environment| {
            status?;
            output.set(environment);
            Ok(())
        }),
    )?;
    slot.take()
        .ok_or_else(|| "WebView2 returned no environment".into())
}

fn create_controller(
    window: &windows_window::Window,
    environment: &ICoreWebView2Environment,
) -> Result<Controller, Box<dyn Error>> {
    let environment = environment.cast::<ICoreWebView2Environment10>()?;
    // SAFETY: The environment and its options are used on their owning STA.
    let options = unsafe { environment.CreateCoreWebView2ControllerOptions()? };
    unsafe { options.SetIsInPrivateModeEnabled(true)? };
    let slot = Rc::new(Cell::new(None));
    let output = slot.clone();
    let handle = HWND(window.hwnd());
    CreateCoreWebView2ControllerCompletedHandler::wait_for_async_operation(
        Box::new(move |handler| {
            // SAFETY: The borrowed window outlives the controller creation.
            unsafe {
                environment.CreateCoreWebView2ControllerWithOptions(handle, &options, &handler)
            }?;
            Ok(())
        }),
        Box::new(move |status, controller| {
            status?;
            output.set(controller);
            Ok(())
        }),
    )?;
    slot.take()
        .map(Controller)
        .ok_or_else(|| "WebView2 returned no controller".into())
}

fn bounds(width: i32, height: i32) -> RECT {
    RECT {
        left: 0,
        top: 0,
        right: width,
        bottom: height,
    }
}

pub(super) fn run(app: WebApp) -> Result<(), Box<dyn Error>> {
    // SAFETY: The launcher calls the GUI on its main thread.
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()? };
    let _apartment = Apartment;
    let current = Rc::new(RefCell::new(None::<ICoreWebView2Controller>));
    let resize = current.clone();
    let icons = icon::WindowIcons::load()?;
    let window = windows_window::Window::new("DiskUnfold")
        .size(900, 600)
        .on_resize(move |w, h| {
            if let Some(controller) = resize.borrow().as_ref() {
                // SAFETY: The window delivers resize events on its owning STA.
                let _ = unsafe { controller.SetBounds(bounds(w, h)) };
            }
        })
        .on_message(|_, kind, _, _| {
            if kind == WM_CLOSE {
                windows_window::quit();
                Some(0)
            } else {
                None
            }
        })
        .create()
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    icons.install(&window);
    let environment = environment()?;
    let native = create_controller(&window, &environment)?;
    let (w, h) = window.client_size();
    // SAFETY: The active controller and view are owned by this STA.
    let view = unsafe {
        native.0.SetBounds(bounds(w, h))?;
        native
            .0
            .cast::<ICoreWebView2Controller4>()?
            .SetAllowExternalDrop(false)?;
        let view = native.0.CoreWebView2()?;
        let settings = view.Settings()?;
        settings.SetAreDevToolsEnabled(false)?;
        settings.SetAreHostObjectsAllowed(false)?;
        settings.SetAreDefaultContextMenusEnabled(false)?;
        settings.SetAreDefaultScriptDialogsEnabled(false)?;
        settings.SetIsBuiltInErrorPageEnabled(false)?;
        view
    };
    let failed = Rc::new(Cell::new(false));
    subscribe(&view, environment, app, failed.clone())?;
    let bootstrap_view = view.clone();
    AddScriptToExecuteOnDocumentCreatedCompletedHandler::wait_for_async_operation(
        Box::new(move |handler| {
            let script = HSTRING::from(BRIDGE);
            // SAFETY: The script and handler are valid for this STA call.
            unsafe {
                bootstrap_view
                    .AddScriptToExecuteOnDocumentCreated(PCWSTR(script.as_ptr()), &handler)
            }?;
            Ok(())
        }),
        Box::new(|status, _| status),
    )?;
    // SAFETY: The live view and controller stay on their owning STA.
    unsafe {
        let url = HSTRING::from(entry());
        view.Navigate(PCWSTR(url.as_ptr()))?;
        native.0.SetIsVisible(true)?;
    }
    current.replace(Some(native.0.clone()));
    windows_window::run();
    current.take();
    if failed.get() {
        return Err("DiskUnfold webview could not load the embedded page".into());
    }
    Ok(())
}

fn subscribe(
    view: &ICoreWebView2,
    environment: ICoreWebView2Environment,
    app: WebApp,
    failed: Rc<Cell<bool>>,
) -> windows::core::Result<()> {
    let WebApp {
        asset, dispatch, ..
    } = app;
    // SAFETY: All callbacks run on the view's STA. Closing its controller releases the registrations.
    unsafe {
        let mut token = 0;
        view.cast::<ICoreWebView2_22>()?
            .AddWebResourceRequestedFilterWithRequestSourceKinds(
                w!("*"),
                COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
                COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL,
            )?;
        view.add_WebResourceRequested(
            &WebResourceRequestedEventHandler::create(Box::new(move |_, args| {
                if let Some(args) = args {
                    serve(&environment, asset, &args)?;
                }
                Ok(())
            })),
            &mut token,
        )?;
        view.add_NavigationStarting(
            &NavigationStartingEventHandler::create(Box::new(|_, args| {
                if let Some(args) = args {
                    let mut url = PWSTR::null();
                    args.Uri(&mut url)?;
                    args.SetCancel(take_pwstr(url) != entry())?;
                }
                Ok(())
            })),
            &mut token,
        )?;
        let ready = Cell::new(false);
        view.add_NavigationCompleted(
            &NavigationCompletedEventHandler::create(Box::new(move |_, args| {
                if let Some(args) = args {
                    let mut success = BOOL::default();
                    args.IsSuccess(&mut success)?;
                    if success.as_bool() {
                        ready.set(true);
                    } else if !ready.get() {
                        failed.set(true);
                        windows_window::quit();
                    }
                }
                Ok(())
            })),
            &mut token,
        )?;
        view.add_NewWindowRequested(
            &NewWindowRequestedEventHandler::create(Box::new(|_, args| {
                if let Some(args) = args {
                    args.SetHandled(true)?;
                }
                Ok(())
            })),
            &mut token,
        )?;
        view.add_PermissionRequested(
            &PermissionRequestedEventHandler::create(Box::new(|_, args| {
                if let Some(args) = args {
                    args.SetState(COREWEBVIEW2_PERMISSION_STATE_DENY)?;
                }
                Ok(())
            })),
            &mut token,
        )?;
        view.cast::<ICoreWebView2_4>()?.add_DownloadStarting(
            &DownloadStartingEventHandler::create(Box::new(|_, args| {
                if let Some(args) = args {
                    args.SetCancel(true)?;
                }
                Ok(())
            })),
            &mut token,
        )?;
        view.add_WebMessageReceived(
            &WebMessageReceivedEventHandler::create(Box::new(move |view, args| {
                if let (Some(view), Some(args)) = (view, args) {
                    reply(&view, &dispatch, &args)?;
                }
                Ok(())
            })),
            &mut token,
        )
    }
}

unsafe fn serve(
    environment: &ICoreWebView2Environment,
    asset: fn(&str) -> Option<Asset>,
    args: &ICoreWebView2WebResourceRequestedEventArgs,
) -> windows::core::Result<()> {
    // SAFETY: The request arguments are valid during this synchronous STA callback.
    unsafe {
        let blocked =
            environment.CreateWebResourceResponse(None, 403, w!("Forbidden"), PCWSTR::null())?;
        args.SetResponse(&blocked)?;
        let request = args.Request()?;
        let mut method = PWSTR::null();
        request.Method(&mut method)?;
        let method = take_pwstr(method);
        let mut uri = PWSTR::null();
        request.Uri(&mut uri)?;
        let Some(asset) = resource(asset, &method, &take_pwstr(uri)) else {
            return Ok(());
        };
        let stream = SHCreateMemStream(Some(asset.bytes)).ok_or(E_POINTER)?;
        let headers = HSTRING::from(format!(
            "Content-Type: {}\r\nContent-Security-Policy: {CSP}\r\nX-Content-Type-Options: nosniff\r\n",
            asset.mime
        ));
        let response = environment.CreateWebResourceResponse(
            &stream,
            200,
            w!("OK"),
            PCWSTR(headers.as_ptr()),
        )?;
        args.SetResponse(&response)
    }
}

unsafe fn reply(
    view: &ICoreWebView2,
    dispatch: &Dispatch,
    args: &ICoreWebView2WebMessageReceivedEventArgs,
) -> windows::core::Result<()> {
    // SAFETY: This is the top-level WebView2 event. Frame events are not registered.
    unsafe {
        let mut text = PWSTR::null();
        if args.TryGetWebMessageAsString(&mut text).is_err() {
            return Ok(());
        }
        let text = take_pwstr(text);
        let Some((id, data)) = text
            .split_once('\n')
            .filter(|(id, _)| id.parse::<u64>().is_ok())
        else {
            return Ok(());
        };
        let mut source = PWSTR::null();
        args.Source(&mut source)?;
        let response = message(dispatch, true, &take_pwstr(source), data).unwrap_or_default();
        let reply = HSTRING::from(format!("{id}\n{response}"));
        view.PostWebMessageAsString(PCWSTR(reply.as_ptr()))
    }
}
