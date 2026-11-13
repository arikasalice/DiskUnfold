//! Ownership of GLib timeout sources whose callbacks can end themselves.
use gtk4::glib;
use std::{cell::RefCell, rc::Rc, time::Duration};

/// Handle to a local timeout source that is removed at most once.
///
/// A callback that returns `ControlFlow::Break` destroys its source, so removing
/// the stored id afterwards is an error. The handle forgets the id when the
/// callback breaks, and `remove` only removes a source that is still scheduled.
pub(super) struct TimerSlot(Rc<RefCell<Option<glib::SourceId>>>);

impl TimerSlot {
    /// Schedules `callback` on the thread-default main context.
    ///
    /// Must be called on the thread that runs that context; the callback runs there.
    pub(super) fn every(
        interval: Duration,
        mut callback: impl FnMut() -> glib::ControlFlow + 'static,
    ) -> Self {
        let slot = Rc::new(RefCell::new(None));
        let owner = Rc::clone(&slot);
        let id = glib::timeout_add_local(interval, move || {
            let flow = callback();
            if matches!(flow, glib::ControlFlow::Break) {
                // GLib destroys the source on Break; forget the id so it is never removed.
                drop(owner.borrow_mut().take());
            }
            flow
        });
        *slot.borrow_mut() = Some(id);
        Self(slot)
    }

    /// Removes the source if it is still scheduled; does nothing after it broke or was removed.
    pub(super) fn remove(&self) {
        let id = self.0.borrow_mut().take();
        if let Some(id) = id {
            id.remove();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    /// One test owns the default main context, which only one thread may acquire.
    #[test]
    fn removing_twice_or_after_break_never_panics() {
        let context = glib::MainContext::default();
        let runs = Rc::new(Cell::new(0));
        let counter = Rc::clone(&runs);
        let broke = TimerSlot::every(Duration::from_millis(1), move || {
            counter.set(counter.get() + 1);
            glib::ControlFlow::Break
        });
        while runs.get() == 0 {
            context.iteration(true);
        }
        broke.remove();
        broke.remove();
        assert_eq!(runs.get(), 1);

        let counter = Rc::clone(&runs);
        let live = TimerSlot::every(Duration::from_millis(1), move || {
            counter.set(counter.get() + 1);
            glib::ControlFlow::Continue
        });
        live.remove();
        while context.iteration(false) {}
        assert_eq!(runs.get(), 1);
    }
}
