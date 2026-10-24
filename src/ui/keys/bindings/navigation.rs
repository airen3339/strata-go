// SPDX-License-Identifier: MIT

use gtk::gdk::{Key, ModifierType as Modifiers};

use crate::ui::keys::{Binding, Command, SHORTCUT_MODIFIERS};

catalog! {
    listed {
        UP: Binding = Binding::plain(Key::Up);
        DOWN: Binding = Binding::plain(Key::Down);
        LEFT: Binding = Binding::plain(Key::Left);
        RIGHT: Binding = Binding::plain(Key::Right);
        VIM_UP: Binding = Binding::plain(Key::k);
        VIM_DOWN: Binding = Binding::plain(Key::j);
        VIM_LEFT: Binding = Binding::plain(Key::h);
        VIM_RIGHT: Binding = Binding::plain(Key::l);
        FIRST_ITEM: Command "first_item" = Binding::plain(Key::Home);
        LAST_ITEM: Command "last_item" = Binding::plain(Key::End);
        FIRST_ITEM_UP: Command "first_item_up" = Binding::ctrl(Key::Up);
        LAST_ITEM_DOWN: Command "last_item_down" = Binding::ctrl(Key::Down);
        PAGE_UP: Command "page_up" = Binding::plain(Key::Page_Up);
        PAGE_DOWN: Command "page_down" = Binding::plain(Key::Page_Down);

        ENTER: Binding = Binding::plain(Key::Return);
        ESCAPE: Binding = Binding::plain(Key::Escape);
        BACKSPACE: Binding = Binding::plain(Key::BackSpace);
        DELETE: Binding = Binding::plain(Key::Delete);
        DELETE_PERMANENTLY: Binding = Binding::shift(Key::Delete);
        FOCUS_NEXT: Binding = Binding::plain(Key::Tab);
        FOCUS_PREVIOUS: Binding = Binding::shift(Key::Tab);

        HISTORY_BACK: Command "history_back" = Binding::alt(Key::Left);
        HISTORY_FORWARD: Command "history_forward" = Binding::alt(Key::Right);
        PARENT_FOLDER: Command "parent_folder" = Binding::alt(Key::Up);
        HOME_FOLDER: Command "home_folder" = Binding::alt(Key::Home);
        OPEN_IN_NEW_TAB: Command "open_in_new_tab" = Binding::ctrl(Key::Return);
        OPEN_IN_NEW_WINDOW: Command "open_in_new_window" = Binding::shift(Key::Return);
    }
    default_only {
        EXTEND_UP: Binding = Binding::shift(Key::Up);
        EXTEND_DOWN: Binding = Binding::shift(Key::Down);
        EXTEND_PAGE_UP: Binding = Binding::shift(Key::Page_Up);
        EXTEND_PAGE_DOWN: Binding = Binding::shift(Key::Page_Down);
        /// GTK's list views handle it; the reference names it.
        SELECTION_TOGGLE: Binding = Binding::ctrl(Key::space);
        /// Previews the focused file or opens a folder. 10xer previews with
        /// **i**, but the file chooser and keys its listing leaves alone still
        /// preview with Space there.
        PREVIEW: Command "preview" = Binding::plain(Key::space), live in both;
    }
    tenxer_only {
        /// Space wherever it activates, selects, plays or types; [`PREVIEW`]
        /// is its quick-preview job, and all the default reference shows.
        SPACE: Binding = Binding::plain(Key::space);
        /// Tab where it completes a path rather than moving focus. Location bar
        /// completion reads it in either map.
        COMPLETE: Command "complete" = Binding::plain(Key::Tab), live in both;
    }
    unlisted {
        // Range selection that GTK performs once the dispatcher passes the key on.
        EXTEND_LEFT: Binding = Binding::shift(Key::Left);
        EXTEND_RIGHT: Binding = Binding::shift(Key::Right);
        EXTEND_TO_FIRST: Binding = Binding::shift(Key::Home);
        EXTEND_TO_LAST: Binding = Binding::shift(Key::End);

        // Home, End and the page keys where they move a caret, seek, scroll or
        // page a list other than the file listing, which the item commands own.
        HOME_KEY: Binding = Binding::plain(Key::Home);
        END_KEY: Binding = Binding::plain(Key::End);
        PAGE_UP_KEY: Binding = Binding::plain(Key::Page_Up);
        PAGE_DOWN_KEY: Binding = Binding::plain(Key::Page_Down);

        /// Escape with any modifiers, as dialogs and inline edits accept it.
        ESCAPE_ANY: Binding = ESCAPE.ignoring(SHORTCUT_MODIFIERS);
        /// Escape with or without Shift, as the listing, prompts and popovers accept it.
        ESCAPE_ANY_SHIFT: Binding = ESCAPE.ignoring(Modifiers::SHIFT_MASK);
        /// Enter with any modifiers, as confirmation dialogs accept it.
        ENTER_ANY: Binding = ENTER.ignoring(SHORTCUT_MODIFIERS);
        /// Enter with or without Shift, as the listing, prompts and popovers accept it.
        ENTER_ANY_SHIFT: Binding = ENTER.ignoring(Modifiers::SHIFT_MASK);
    }
}
