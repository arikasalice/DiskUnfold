//! Window shutdown admission and periodic coordinator progress.

use super::super::contract::Events;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum WindowPhase {
    Starting,
    CloseRequested,
    Running,
}

pub(super) fn request_close(events: Option<&Events>) -> bool {
    let Some(events) = events.filter(|events| events.has_shutdown_callbacks()) else {
        return true;
    };
    events.request_window_close();
    events.shutdown_is_complete()
}

pub(super) fn pump_shutdown(events: Option<&Events>) -> bool {
    let Some(events) = events else {
        return false;
    };
    events.pump_shutdown();
    events.shutdown_is_complete()
}
