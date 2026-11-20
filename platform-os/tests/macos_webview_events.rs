//! #228: the macOS WebView delivers host events to the trusted page.
//!
//! AppKit runs only on the main thread, so this test has no libtest harness. It opens a
//! window, so it runs only with `DU_TEST_MAC_WEBVIEW=1` in a logged-in GUI session (for
//! example started by a LaunchAgent in `gui/<uid>`); otherwise it reports a skip.
#[cfg(target_os = "macos")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    real::run()
}

#[cfg(not(target_os = "macos"))]
fn main() {}

#[cfg(target_os = "macos")]
mod real {
    use du_platform_os::webview::{Asset, EventCallbacks, Events, WebApp, run_with_events};
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };
    use std::time::{Duration, Instant};

    const INDEX: &[u8] =
        br#"<!doctype html><title>Events fixture</title><script type="module" src="/check.js"></script>"#;
    const SCRIPT: &[u8] = br#"
        const report = (text) => window.webkit.messageHandlers.diskunfold.postMessage(text);
        const timer = setTimeout(() => report('result:timeout'), 8000);
        window.addEventListener('du://event', ({ detail }) => {
            clearTimeout(timer);
            const injected = window.__diskunfoldInjected === true;
            report('result:' + JSON.stringify({ text: detail.body.text, injected }));
        }, { once: true });
    "#;
    const EVENT: &str = r#"{"contractVersion":1,"topic":"fixture.event","occurredAt":"2026-11-20T00:00:00.000Z","seq":1,"body":{"text":"\"); window.__diskunfoldInjected = true; //"}}"#;
    const EXPECTED: &str =
        r#"result:{"text":"\"); window.__diskunfoldInjected = true; //","injected":false}"#;

    fn asset(path: &str) -> Option<Asset> {
        match path {
            "/index.html" => Some(Asset {
                mime: "text/html",
                bytes: INDEX,
            }),
            "/check.js" => Some(Asset {
                mime: "text/javascript",
                bytes: SCRIPT,
            }),
            _ => None,
        }
    }

    fn counter() -> (Arc<AtomicUsize>, impl Fn() + Send + Sync + 'static) {
        let count = Arc::new(AtomicUsize::new(0));
        let callback = Arc::clone(&count);
        (count, move || {
            callback.fetch_add(1, Ordering::SeqCst);
        })
    }

    pub(super) fn run() -> Result<(), Box<dyn std::error::Error>> {
        if std::env::var_os("DU_TEST_MAC_WEBVIEW").is_none() {
            eprintln!("skipping: set DU_TEST_MAC_WEBVIEW=1 in a logged-in GUI session");
            return Ok(());
        }
        let received = Arc::new(Mutex::new(None::<String>));
        let reply = Arc::clone(&received);
        let app = WebApp {
            asset,
            dispatch: Arc::new(move |data: &str| {
                if data.starts_with("result:")
                    && let Ok(mut slot) = reply.lock()
                {
                    *slot = Some(data.to_owned());
                }
                Some("ok".to_owned())
            }),
            locale_enabled: false,
            native_language: "en".to_owned(),
            icon: &[],
        };
        let pending = Arc::new(AtomicBool::new(true));
        let failures = Arc::new(AtomicUsize::new(0));
        let failed = Arc::clone(&failures);
        let (started, on_started) = counter();
        let (ready, on_ready) = counter();
        let (closed, on_closed) = counter();
        let done = Arc::clone(&received);
        let deadline = Instant::now() + Duration::from_secs(15);
        let events = Events::new(
            move || {
                pending
                    .swap(false, Ordering::SeqCst)
                    .then(|| EVENT.to_owned())
            },
            EventCallbacks::new(
                move |_| {
                    failed.fetch_add(1, Ordering::SeqCst);
                },
                on_started,
                on_ready,
                on_closed,
            ),
        )
        .with_shutdown_callbacks(
            || {},
            || {},
            move || Instant::now() > deadline || done.lock().map_or(true, |slot| slot.is_some()),
        );
        run_with_events(app, events)?;
        let received = received.lock().map_err(|_| "result lock")?.clone();
        let counts = (
            started.load(Ordering::SeqCst),
            ready.load(Ordering::SeqCst),
            closed.load(Ordering::SeqCst),
            failures.load(Ordering::SeqCst),
        );
        eprintln!(
            "macos_webview_events: received={received:?} started/ready/closed/failures={counts:?}"
        );
        if received.as_deref() != Some(EXPECTED) || counts != (1, 1, 1, 0) {
            return Err("the page did not receive the event as data exactly once".into());
        }
        eprintln!("macos_webview_events: passed");
        Ok(())
    }
}
