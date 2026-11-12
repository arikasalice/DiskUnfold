use super::contract::{Asset, CSP, Dispatch, Events, ORIGIN, WebApp, entry, message, resource};
use gtk4::{gio, glib, prelude::*};
use std::{cell::RefCell, error::Error, fmt::Write, rc::Rc};
use webkit6::{prelude::*, soup};
mod events;
use events::{EventLifecycle, retire_session};

fn serve(context: &webkit6::WebContext, asset: fn(&str) -> Option<Asset>) {
    if let Some(security) = context.security_manager() {
        security.register_uri_scheme_as_secure("diskunfold");
        security.register_uri_scheme_as_cors_enabled("diskunfold");
    }
    context.register_uri_scheme("diskunfold", move |request| {
        let uri = request.uri().unwrap_or_default();
        let method = request.http_method().unwrap_or_default();
        let asset = resource(asset, method.as_str(), uri.as_str());
        let bytes = asset.map_or(b"".as_slice(), |a| a.bytes);
        let input = gio::MemoryInputStream::from_bytes(&glib::Bytes::from_static(bytes));
        let response =
            webkit6::URISchemeResponse::new(&input, i64::try_from(bytes.len()).unwrap_or(-1));
        response.set_content_type(asset.map_or("text/plain", |a| a.mime));
        if asset.is_none() {
            response.set_status(404, Some("Not Found"));
        }
        let headers = soup::MessageHeaders::new(soup::MessageHeadersType::Response);
        headers.append("Content-Security-Policy", CSP);
        headers.append("X-Content-Type-Options", "nosniff");
        response.set_http_headers(headers);
        request.finish_with_response(&response);
    });
}

fn bridge(
    manager: &webkit6::UserContentManager,
    view: &webkit6::WebView,
    dispatch: Dispatch,
    expose_locale: bool,
) -> Result<(), Box<dyn Error>> {
    let mut random = [0_u8; 32];
    getrandom::fill(&mut random).map_err(|error| std::io::Error::other(error.to_string()))?;
    let mut token = String::with_capacity(64);
    for byte in random {
        write!(token, "{byte:02x}")?;
    }
    // WebKitGTK's script-message signal supplies no sender frame. Only a top-frame
    // document-start script receives this capability, and subframes are prohibited.
    let locale_marker = if expose_locale {
        "Object.defineProperty(window, '__diskunfoldLocale', { value: true });"
    } else {
        ""
    };
    let script = format!(
        r#"(() => {{
        if (location.href !== '{ORIGIN}/index.html' || window !== window.top) return;
        {locale_marker}
        const handler = window.webkit.messageHandlers.diskunfold;
        const post = handler.postMessage.bind(handler);
        Object.defineProperty(window, '__diskunfoldRequest', {{ value: (payload) => post('{token}\n' + payload) }});
    }})();"#,
        locale_marker = locale_marker,
    );
    manager.add_script(&webkit6::UserScript::new(
        &script,
        webkit6::UserContentInjectedFrames::TopFrame,
        webkit6::UserScriptInjectionTime::Start,
        &[],
        &[],
    ));
    let view = view.downgrade();
    manager.connect_script_message_with_reply_received(
        Some("diskunfold"),
        move |_, value, reply| {
            if !value.is_string() {
                reply.return_error_message("DiskUnfold IPC request denied");
                return true;
            }
            let input = value.to_str();
            let result = input.split_once('\n').and_then(|(key, data)| {
                let uri = view.upgrade()?.uri()?;
                message(&dispatch, key == token, uri.as_str(), data)
            });
            if let (Some(result), Some(context)) = (result, value.context()) {
                reply.return_value(&webkit6::javascriptcore::Value::new_string(
                    &context,
                    Some(&result),
                ));
            } else {
                reply.return_error_message("DiskUnfold IPC request denied");
            }
            true
        },
    );
    if !manager.register_script_message_handler_with_reply("diskunfold", None) {
        return Err("Could not register the DiskUnfold IPC channel".into());
    }
    Ok(())
}

fn webview(
    app: WebApp,
    expose_locale: bool,
) -> Result<(webkit6::WebView, webkit6::UserContentManager), Box<dyn Error>> {
    let context = webkit6::WebContext::new();
    let manager = webkit6::UserContentManager::new();
    let session = webkit6::NetworkSession::new_ephemeral();
    let view = webkit6::WebView::builder()
        .web_context(&context)
        .network_session(&session)
        .user_content_manager(&manager)
        .build();
    serve(&context, app.asset);
    bridge(&manager, &view, app.dispatch, expose_locale)?;
    view.connect_decide_policy(|_, decision, kind| {
        let allowed = kind == webkit6::PolicyDecisionType::NavigationAction
            && decision
                .downcast_ref::<webkit6::NavigationPolicyDecision>()
                .and_then(|nav| nav.navigation_action())
                .and_then(|action| action.request())
                .and_then(|request| request.uri())
                .is_some_and(|url| url.as_str() == entry());
        if allowed || kind == webkit6::PolicyDecisionType::Response {
            decision.use_();
        } else {
            decision.ignore();
        }
        true
    });
    view.connect_permission_request(|_, request| {
        request.deny();
        true
    });
    session.connect_download_started(|_, download| download.cancel());
    Ok((view, manager))
}

pub(super) fn run(app: WebApp, events: Option<Events>) -> Result<(), Box<dyn Error>> {
    gtk4::init()?;
    let (view, manager) = webview(app, events.is_some())?;
    let lifecycle = events
        .as_ref()
        .map(|events| EventLifecycle::attach(&view, events.clone()));
    let window = gtk4::Window::builder()
        .title("DiskUnfold")
        .default_width(900)
        .default_height(600)
        .child(&view)
        .build();
    let event_loop = glib::MainLoop::new(None, false);
    let close = event_loop.clone();
    let close_events = events.clone();
    let close_lifecycle = lifecycle
        .as_ref()
        .map(|lifecycle| Rc::clone(&lifecycle.state));
    window.connect_close_request(move |_| {
        if let Some(events) = &close_events {
            if events.has_shutdown_callbacks() {
                events.request_window_close();
                if !events.shutdown_is_complete() {
                    return glib::Propagation::Stop;
                }
            }
        }
        if let Some(state) = &close_lifecycle {
            retire_session(state, false);
        }
        close.quit();
        glib::Propagation::Proceed
    });
    let failure = Rc::new(RefCell::new(None));
    let report = failure.clone();
    let quit = event_loop.clone();
    let failed_lifecycle = lifecycle
        .as_ref()
        .map(|lifecycle| Rc::clone(&lifecycle.state));
    view.connect_load_failed(move |_, _, url, error| {
        if url == entry() {
            if let Some(state) = &failed_lifecycle {
                retire_session(state, false);
            }
            *report.borrow_mut() = Some(error.to_string());
            quit.quit();
        }
        true
    });
    let event_timer = lifecycle
        .as_ref()
        .zip(events.as_ref())
        .map(|(lifecycle, events)| lifecycle.start_polling(&view, events.clone()));
    let shutdown_timer = events
        .as_ref()
        .filter(|events| events.has_shutdown_callbacks())
        .map(|events| {
            let events = events.clone();
            let shutdown_loop = event_loop.clone();
            glib::timeout_add_local(std::time::Duration::from_millis(16), move || {
                events.pump_shutdown();
                if events.shutdown_is_complete() {
                    shutdown_loop.quit();
                    glib::ControlFlow::Break
                } else {
                    glib::ControlFlow::Continue
                }
            })
        });
    view.load_uri(&entry());
    window.present();
    if failure.borrow().is_none() {
        event_loop.run();
    }
    if let Some(timer) = event_timer {
        timer.remove();
    }
    if let Some(timer) = shutdown_timer {
        timer.remove();
    }
    drop(lifecycle);
    manager.unregister_script_message_handler("diskunfold", None);
    match failure.take() {
        Some(error) => Err(error.into()),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::webview::Asset;
    use std::sync::Arc;

    const INDEX: &[u8] =
        br#"<!doctype html><title>Fixture</title><script type="module" src="/check.js"></script>"#;
    const SCRIPT: &[u8] = br#"
        try {
            const nativeEvent = new Promise((resolve, reject) => {
                const timer = setTimeout(() => reject(new Error('event not received')), 5000);
                window.addEventListener('du://event', ({ detail }) => {
                    clearTimeout(timer);
                    resolve(detail);
                }, { once: true });
            });
            const marker = Object.getOwnPropertyDescriptor(window, '__diskunfoldLocale');
            if (marker?.value !== true || marker.writable || marker.configurable) throw new Error('locale marker');
            const response = await window.__diskunfoldRequest('ping');
            if (response !== 'pong') throw new Error('bridge reply');
            let denied = false;
            try { await window.webkit.messageHandlers.diskunfold.postMessage('ping'); }
            catch { denied = true; }
            if (!denied) throw new Error('missing capability was accepted');
            const frame = document.createElement('iframe');
            frame.src = '/frame.html';
            document.body.append(frame);
            await new Promise(resolve => setTimeout(resolve, 300));
            if (frame.contentDocument?.title === 'Frame fixture') throw new Error('subframe rendered');
            const envelope = await nativeEvent;
            if (envelope.body.text !== '\"); window.__diskunfoldInjected = true; //') throw new Error('event payload');
            if (window.__diskunfoldInjected === true) throw new Error('event script injection');
            document.title = 'IPC passed';
        } catch (error) { document.title = `IPC failed: ${error.message}`; }
    "#;

    const EVENT: &str = r#"{"contractVersion":1,"topic":"fixture.event","occurredAt":"2026-11-12T00:00:00.000Z","seq":1,"body":{"text":"\"); window.__diskunfoldInjected = true; //"}}"#;

    #[test]
    #[ignore = "requires a native WebKitGTK display"]
    fn linux_webview_smoke() -> Result<(), Box<dyn Error>> {
        gtk4::init()?;
        let (view, manager) = webview(
            WebApp {
                asset: |path| match path {
                    "/index.html" => Some(Asset {
                        mime: "text/html",
                        bytes: INDEX,
                    }),
                    "/check.js" => Some(Asset {
                        mime: "text/javascript",
                        bytes: SCRIPT,
                    }),
                    "/frame.html" => Some(Asset {
                        mime: "text/html",
                        bytes: b"<title>Frame fixture</title>",
                    }),
                    _ => None,
                },
                dispatch: std::sync::Arc::new(|payload| {
                    (payload == "ping").then(|| "pong".to_owned())
                }),
                icon: &[],
            },
            true,
        )?;
        let next_event = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let source = Arc::clone(&next_event);
        let started = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let started_callback = Arc::clone(&started);
        let ready = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let ready_callback = Arc::clone(&ready);
        let closed = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let closed_callback = Arc::clone(&closed);
        let failures = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let failed_callback = Arc::clone(&failures);
        let events = Events::new(
            move || {
                source
                    .swap(false, std::sync::atomic::Ordering::SeqCst)
                    .then(|| EVENT.to_owned())
            },
            super::super::contract::EventCallbacks::new(
                move |_| {
                    failed_callback.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                },
                move || {
                    started_callback.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                },
                move || {
                    ready_callback.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                },
                move || {
                    closed_callback.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                },
            ),
        );
        let lifecycle = EventLifecycle::attach(&view, events.clone());
        let event_timer = lifecycle.start_polling(&view, events);
        let window = gtk4::Window::builder().child(&view).build();
        let event_loop = glib::MainLoop::new(None, false);
        let quit = event_loop.clone();
        view.connect_title_notify(move |view| {
            if view.title().is_some_and(|t| t.starts_with("IPC ")) {
                quit.quit();
            }
        });
        let timeout = event_loop.clone();
        glib::timeout_add_seconds_local_once(10, move || timeout.quit());
        view.load_uri(&entry());
        window.present();
        event_loop.run();
        event_timer.remove();
        drop(lifecycle);
        manager.unregister_script_message_handler("diskunfold", None);
        assert_eq!(view.title().as_deref(), Some("IPC passed"));
        assert_eq!(started.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(ready.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(closed.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(failures.load(std::sync::atomic::Ordering::SeqCst), 0);
        Ok(())
    }
}
