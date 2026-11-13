//! Application menu for the macOS adapter (F15 FR9.HD1: dmg row).
//!
//! The menu carries the command line tool link action and an orderly Quit item.
use super::Handler;
use crate::cli_link;
use objc2::{
    MainThreadMarker, MainThreadOnly,
    rc::Retained,
    runtime::{AnyObject, Sel},
    sel,
};
use objc2_app_kit::{NSAlert, NSApplication, NSMenu, NSMenuItem};
use objc2_foundation::NSString;
use std::path::PathBuf;

/// Installs the main menu: the command line tool item, and Quit when the host owns shutdown.
///
/// Quit is offered only with host shutdown hooks because `terminate:` otherwise ends the
/// process without the host's cleanup.
pub(super) fn install_main_menu(
    mtm: MainThreadMarker,
    application: &NSApplication,
    handler: &Handler,
    with_quit: bool,
) {
    let main_menu = NSMenu::new(mtm);
    let app_item = NSMenuItem::new(mtm);
    let app_menu = NSMenu::new(mtm);
    let install = item(
        mtm,
        "Install Command Line Tool\u{2026}",
        sel!(installCommandLineTool:),
        "",
    );
    // SAFETY: `handler` outlives the menu: the caller keeps it alive until the
    // application loop ends, and the menu is released with the application.
    unsafe { install.setTarget(Some(AsRef::<AnyObject>::as_ref(handler))) };
    app_menu.addItem(&install);
    if with_quit {
        app_menu.addItem(&NSMenuItem::separatorItem(mtm));
        // No target: `terminate:` goes through the responder chain to the application,
        // whose delegate runs the host's orderly shutdown.
        app_menu.addItem(&item(mtm, "Quit DiskUnfold", sel!(terminate:), "q"));
    }
    app_item.setSubmenu(Some(&app_menu));
    main_menu.addItem(&app_item);
    application.setMainMenu(Some(&main_menu));
}

fn item(mtm: MainThreadMarker, title: &str, action: Sel, key: &str) -> Retained<NSMenuItem> {
    // SAFETY: Both strings are valid for the call, and the selectors name methods of the
    // handler or of NSApplication.
    unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(title),
            Some(action),
            &NSString::from_str(key),
        )
    }
}

/// Links the running bundle's executable into the user's `~/.local/bin` and reports the result.
pub(super) fn install_command_line_tool(mtm: MainThreadMarker) {
    let directory = std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|home| home.is_absolute())
        .map(|home| cli_link::default_directory(&home));
    let result = directory.as_deref().map(|directory| {
        std::env::current_exe()
            .and_then(std::fs::canonicalize)
            .map_err(cli_link::LinkError::Io)
            .and_then(|executable| cli_link::install_link(&executable, directory))
    });
    let (title, detail) = match (&result, &directory) {
        (Some(result), Some(directory)) => cli_link::describe(result, directory),
        _ => (
            "The command line tool could not be installed".to_owned(),
            "The home folder could not be found.".to_owned(),
        ),
    };
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&NSString::from_str(&title));
    alert.setInformativeText(&NSString::from_str(&detail));
    alert.runModal();
}
