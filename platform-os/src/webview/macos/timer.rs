//! The main-thread ticker that drives the host shutdown and the event delivery.
use super::Handler;
use objc2::{
    class, msg_send,
    rc::Retained,
    runtime::{AnyObject, Bool},
    sel,
};
use objc2_app_kit::{NSApplication, NSEvent, NSEventModifierFlags, NSEventType};
use objc2_foundation::{NSPoint, NSString};

/// Interval of the ticker, matching the Windows and Linux tickers.
const TICK_SECONDS: f64 = 0.016;

#[link(name = "Foundation", kind = "framework")]
unsafe extern "C" {
    /// The run-loop mode set that covers tracking, modal and default modes.
    static NSRunLoopCommonModes: &'static NSString;
}

/// Invalidates the ticker on every exit path.
pub(super) struct TimerGuard(Retained<AnyObject>);

impl Drop for TimerGuard {
    fn drop(&mut self) {
        // SAFETY: `invalidate` is sent to the NSTimer this guard retains, on the main thread.
        unsafe {
            let () = msg_send![&*self.0, invalidate];
        }
    }
}

/// Schedules the ticker (`tick:` on `handler`) in the common run-loop modes.
///
/// A postponed quit (`TerminateLater`) runs the loop in a modal mode, and a window
/// resize or menu tracking uses an event-tracking mode. A timer in the default mode
/// alone would stall in both, and neither the shutdown nor the events would advance.
pub(super) fn schedule(handler: &Handler) -> TimerGuard {
    // SAFETY: Called on the main thread; the timer retains `handler` until invalidated,
    // and `tick:` is a method `Handler` defines with the NSTimer callback signature.
    unsafe {
        let timer: Retained<AnyObject> = msg_send![
            class!(NSTimer),
            timerWithTimeInterval: TICK_SECONDS,
            target: handler,
            selector: sel!(tick:),
            userInfo: Option::<&AnyObject>::None,
            repeats: Bool::YES
        ];
        let run_loop: Retained<AnyObject> = msg_send![class!(NSRunLoop), currentRunLoop];
        let () = msg_send![&*run_loop, addTimer: &*timer, forMode: NSRunLoopCommonModes];
        TimerGuard(timer)
    }
}

/// Ends `NSApplication::run` from outside an event handler.
///
/// `stop:` only takes effect after the next event, so a timer callback must post
/// one for the loop to observe the request.
pub(super) fn stop_application(application: &NSApplication) {
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
