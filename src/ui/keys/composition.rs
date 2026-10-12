// SPDX-License-Identifier: MIT

//! Input methods compose text (the preedit) in the focused field before they
//! commit it. GTK filters keys through the input method after every
//! capture-phase handler, so while a composition is pending, key handlers yield
//! every key to it.

use std::cell::{Cell, RefCell};

use gtk::{gio, glib, prelude::*};

thread_local! {
    static FOLLOWING: Cell<bool> = const { Cell::new(false) };
    /// Fields whose preedit handler is connected; it stays for their lifetime.
    static FOLLOWED: RefCell<Vec<glib::WeakRef<gtk::Widget>>> = const { RefCell::new(Vec::new()) };
    /// Fields that show an unfinished composition.
    static COMPOSING: RefCell<Vec<glib::WeakRef<gtk::Widget>>> = const { RefCell::new(Vec::new()) };
}

/// Whether the focused text field of the controller's window is composing.
pub(super) fn pending(controller: &gtk::EventControllerKey) -> bool {
    COMPOSING.with_borrow(|composing| {
        !composing.is_empty()
            && controller
                .widget()
                .and_then(|widget| widget.root())
                .and_then(|root| root.focus())
                .is_some_and(|focus| {
                    composing
                        .iter()
                        .any(|field| field.upgrade().as_ref() == Some(&focus))
                })
    })
}

/// Follows the focused text field of every window, now and later. Runs once.
pub(super) fn follow() {
    if FOLLOWING.replace(true) {
        return;
    }
    let windows = gtk::Window::toplevels();
    follow_windows(&windows, 0, windows.n_items());
    windows.connect_items_changed(|windows, position, _, added| {
        follow_windows(windows, position, added);
    });
}

fn follow_windows(windows: &gio::ListModel, position: u32, count: u32) {
    for window in (position..position + count)
        .filter_map(|index| windows.item(index).and_downcast::<gtk::Window>())
    {
        follow_focus(&window);
        window.connect_focus_widget_notify(follow_focus);
    }
}

fn follow_focus(window: &gtk::Window) {
    let Some(focus) = GtkWindowExt::focus(window) else {
        return;
    };
    let text = focus.downcast_ref::<gtk::Text>();
    let view = focus.downcast_ref::<gtk::TextView>();
    if text.is_none() && view.is_none() {
        return;
    }
    FOLLOWED.with_borrow_mut(|followed| {
        followed.retain(|field| field.upgrade().is_some());
        if followed
            .iter()
            .any(|field| field.upgrade().as_ref() == Some(&focus))
        {
            return;
        }
        if let Some(text) = text {
            text.connect_preedit_changed(|text, preedit| {
                composition_changed(text.upcast_ref(), preedit);
            });
        }
        if let Some(view) = view {
            view.connect_preedit_changed(|view, preedit| {
                composition_changed(view.upcast_ref(), preedit);
            });
        }
        followed.push(focus.downgrade());
    });
}

fn composition_changed(field: &gtk::Widget, preedit: &str) {
    COMPOSING.with_borrow_mut(|composing| {
        composing.retain(|other| other.upgrade().is_some_and(|other| &other != field));
        if !preedit.is_empty() {
            composing.push(field.downgrade());
        }
    });
}
