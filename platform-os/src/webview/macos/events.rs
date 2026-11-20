//! Application event delivery to the trusted top-level page (#228, F06 FR8).
//!
//! The ticker takes a bounded batch from the host's event source and posts it one
//! envelope at a time, in order, through `callAsyncJavaScript`. The envelope is passed as
//! the function argument `payload`, never as script text. The navigation delegate opens
//! and readies the document generation that accepts events ([`DocumentGate`]).
use super::super::contract::entry;
use super::super::events_common::{
    DocumentGate, EVENT_DELIVERY_SCRIPT, fail_event_batch, take_event_batch,
};
use super::{Handler, State};
use block2::RcBlock;
use objc2::{DefinedClass, MainThreadMarker, Message, rc::Retained, runtime::AnyObject};
use objc2_foundation::{NSDictionary, NSError, NSNumber, NSString};
use objc2_web_kit::{WKContentWorld, WKWebView};
use std::rc::Rc;

/// Whether the view shows the application's own entry document.
fn trusted(view: &WKWebView) -> bool {
    // SAFETY: Called on the main thread with a live view; `URL` only reads its state.
    let url = unsafe { view.URL() };
    url.and_then(|url| url.absoluteString())
        .is_some_and(|url| url.to_string() == entry())
}

/// A trusted document committed its navigation: a new generation starts.
pub(super) fn document_committed(state: &State, view: &WKWebView) {
    if let Some(events) = &state.events
        && trusted(view)
        && state.document.borrow_mut().start()
    {
        events.document_started();
    }
}

/// The committed document finished loading and now accepts events.
pub(super) fn document_finished(state: &State, view: &WKWebView) {
    if let Some(events) = &state.events
        && trusted(view)
        && state.document.borrow_mut().mark_ready()
    {
        events.document_ready();
    }
}

/// The page's content process ended; nothing is posted until a new document starts.
pub(super) fn document_lost(state: &State) {
    state.document.borrow_mut().retire(false);
}

/// The window closed: delivery ends for good and the host is told once.
pub(super) fn close(state: &State) {
    if let Some(events) = &state.events {
        state.document.borrow_mut().retire(true);
        events.document_closed();
    }
}

/// One ticker step: posts the next batch when the document accepts events.
pub(super) fn poll(handler: &Handler, mtm: MainThreadMarker) {
    let state = handler.ivars();
    let Some(events) = &state.events else {
        return;
    };
    let Some(generation) = state.document.borrow().ready_to_send() else {
        return;
    };
    let Some(view) = state.view.borrow().clone() else {
        return;
    };
    if !trusted(&view) {
        return;
    }
    let batch = take_event_batch(|| events.next());
    if batch.is_empty() {
        return;
    }
    state.document.borrow_mut().sending = true;
    post(handler.retain(), view, mtm, generation, Rc::new(batch), 0);
}

/// Posts envelope `index` of `batch`; its completion posts the next one.
fn post(
    handler: Retained<Handler>,
    view: Retained<WKWebView>,
    mtm: MainThreadMarker,
    generation: u64,
    batch: Rc<Vec<String>>,
    index: usize,
) {
    let state = handler.ivars();
    let Some(events) = &state.events else {
        return;
    };
    let document: &std::cell::RefCell<DocumentGate> = &state.document;
    if index == batch.len() {
        document.borrow_mut().finish(generation);
        return;
    }
    if !document.borrow().is_current(generation) {
        return;
    }
    if !trusted(&view) {
        fail_event_batch(events, &batch, index);
        document.borrow_mut().finish(generation);
        return;
    }
    let key = NSString::from_str("payload");
    let payload = NSString::from_str(&batch[index]);
    let value: &AnyObject = &payload;
    let arguments = NSDictionary::<NSString, AnyObject>::from_slices(&[&*key], &[value]);
    let script = NSString::from_str(EVENT_DELIVERY_SCRIPT);
    let target = view.clone();
    let completion = RcBlock::new(move |result: *mut AnyObject, error: *mut NSError| {
        let state = handler.ivars();
        if !state.document.borrow().is_current(generation) {
            state.document.borrow_mut().finish(generation);
            return;
        }
        // SAFETY: WebKit passes null or an object that stays valid during this callback.
        let returned = unsafe { result.as_ref() };
        let delivered = error.is_null()
            && returned
                .and_then(|value| value.downcast_ref::<NSNumber>())
                .is_some_and(|value| value.boolValue());
        if delivered {
            post(
                handler.clone(),
                target.clone(),
                mtm,
                generation,
                Rc::clone(&batch),
                index + 1,
            );
        } else {
            if let Some(events) = &state.events {
                fail_event_batch(events, &batch, index);
            }
            state.document.borrow_mut().finish(generation);
        }
    });
    // SAFETY: Called on the main thread. The script, the arguments and the block are
    // retained by WebKit for the call, and WebKit runs the block on the main thread,
    // where every `State` borrow above has already ended.
    unsafe {
        view.callAsyncJavaScript_arguments_inFrame_inContentWorld_completionHandler(
            &script,
            Some(&arguments),
            None,
            &WKContentWorld::pageWorld(mtm),
            Some(&completion),
        );
    }
}
