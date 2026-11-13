use super::StartupDialogOutcome;
use gtk4::{AlertDialog, Window, gio, glib};
use std::{cell::Cell, rc::Rc};

pub(super) fn show_pre_window_error_dialog(title: &str, detail: &str) -> StartupDialogOutcome {
    if gtk4::is_initialized() {
        if !gtk4::is_initialized_main_thread() {
            return StartupDialogOutcome::Unsupported;
        }
    } else if gtk4::init().is_err() || !gtk4::is_initialized_main_thread() {
        return StartupDialogOutcome::Unsupported;
    }

    let dialog = AlertDialog::builder().message(title).detail(detail).build();
    dialog.set_buttons(&["OK"]);
    dialog.set_default_button(0);
    dialog.set_cancel_button(0);
    dialog.set_modal(true);

    let main_loop = glib::MainLoop::new(None, false);
    let outcome = Rc::new(Cell::new(StartupDialogOutcome::Failed));
    let callback_outcome = Rc::clone(&outcome);
    let callback_loop = main_loop.clone();
    dialog.choose(None::<&Window>, None::<&gio::Cancellable>, move |result| {
        callback_outcome.set(if result.is_ok() {
            StartupDialogOutcome::Acknowledged
        } else {
            StartupDialogOutcome::Failed
        });
        callback_loop.quit();
    });
    main_loop.run();
    outcome.get()
}
