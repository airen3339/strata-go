// SPDX-License-Identifier: MIT

use gtk::gdk::{Key, ModifierType as Modifiers};

use crate::ui::keys::{Binding, Command, Digits};

catalog! {
    listed {
        COPY: Command "copy" = Binding::ctrl(Key::c);
        CUT: Command "cut" = Binding::ctrl(Key::x);
        PASTE: Command "paste" = Binding::ctrl(Key::v);
        SELECT_ALL: Command "select_all" = Binding::ctrl(Key::a);
        UNDO: Command "undo" = Binding::ctrl(Key::z);
        REDO: Command "redo" = Binding::ctrl_shift(Key::z);
        REDO_Y: Command "redo_y" = Binding::ctrl(Key::y);

        NEW_FOLDER: Command "new_folder" = Binding::ctrl_shift(Key::n);
        NEW_FOLDER_WITH_SELECTION: Command "new_folder_with_selection" =
            Binding::ctrl_alt(Key::n);
        RENAME: Command "rename" = Binding::plain(Key::F2);
        PROPERTIES: Command "properties" = Binding::alt(Key::Return);
        CONTEXT_MENU: Command "context_menu" = Binding::plain(Key::Menu);
        CONTEXT_MENU_F10: Command "context_menu_f10" = Binding::shift(Key::F10);

        SEARCH: Command "search" = Binding::ctrl(Key::k);
        LOCATION: Command "location" = Binding::ctrl(Key::l);
        REFRESH: Command "refresh" = Binding::plain(Key::F5);
        HIDDEN_FILES: Command "toggle_hidden_files" = Binding::ctrl(Key::h);
        HIDDEN_FILES_PERIOD: Command "hidden_files_period" = Binding::ctrl(Key::period);

        VIEW_COLUMNS: Command "view_columns" = Binding::ctrl(Key::_1);
        VIEW_ICONS: Command "view_icons" = Binding::ctrl(Key::_2);
        VIEW_LIST: Command "view_list" = Binding::ctrl(Key::_3);
        TEXT_LARGER: Command "text_larger" = Binding::ctrl(Key::plus);
        TEXT_SMALLER: Command "text_smaller" = Binding::ctrl(Key::minus);
        TEXT_RESET: Command "text_reset" = Binding::ctrl(Key::_0);
        THUMBNAILS_LARGER: Command "thumbnails_larger" = Binding::ctrl_alt(Key::plus);
        THUMBNAILS_SMALLER: Command "thumbnails_smaller" = Binding::ctrl_alt(Key::minus);
        THUMBNAILS_RESET: Command "thumbnails_reset" = Binding::ctrl_alt(Key::_0);
        SIDEBAR_FOCUS: Command "focus_sidebar" = Binding::ctrl_shift(Key::b);

        SETTINGS: Command "settings" = Binding::ctrl(Key::comma);
        REFERENCE: Command "shortcut_reference" = Binding::plain(Key::F1);
        TENXER_MODE: Command "tenxer_mode" = Binding::ctrl_shift(Key::m);

        TAB_NEW: Command "tab_new" = Binding::ctrl(Key::t);
        TAB_CLOSE: Command "tab_close" = Binding::ctrl(Key::w);
        TAB_NEXT: Command "tab_next" = Binding::ctrl(Key::Tab);
        TAB_PREVIOUS: Command "tab_previous" = Binding::ctrl_shift(Key::Tab);
        TAB_PREVIOUS_PAGE_UP: Command "tab_previous_page_up" = Binding::ctrl(Key::Page_Up);
        TAB_NEXT_PAGE_DOWN: Command "tab_next_page_down" = Binding::ctrl(Key::Page_Down);
        TAB_MOVE_LEFT: Command "tab_move_left" = Binding::ctrl_shift(Key::Page_Up);
        TAB_MOVE_RIGHT: Command "tab_move_right" = Binding::ctrl_shift(Key::Page_Down);
        TAB_NUMBERS: Digits = Digits::new(
            Modifiers::CONTROL_MASK.union(Modifiers::SHIFT_MASK),
            9,
            true,
        );
    }
    default_only {
        // The 10xer map gives these keys to other commands or leaves them out.
        // Those marked live in both still reach their handlers in 10xer where
        // its own keys do not claim them first, as in popovers.
        DUPLICATE: Command "duplicate" = Binding::ctrl(Key::d), live in both;
        COPY_PATH: Command "copy_path" = Binding::plain(Key::y);
        PIN: Command "pin" = Binding::plain(Key::p);
        RENAME_R: Command "rename_r" = Binding::ctrl(Key::r), live in both;
        JUMP_TO_FOLDER: Command "jump_to_folder" = Binding::ctrl_shift(Key::k), live in both;
        FILTER: Command "filter" = Binding::ctrl(Key::f), live in both;
        TERMINAL: Command "terminal" = Binding::ctrl_alt(Key::t), live in both;
        ARROW_SCOPE: Command "arrow_scope" = Binding::ctrl(Key::backslash);
        SIDEBAR: Command "toggle_sidebar" = Binding::ctrl(Key::b);
    }
    unlisted {
        /// While type-to-search is on, **/** opens the filter without typing.
        FILTER_SLASH: Command "filter_slash" = Binding::plain(Key::slash), live in default;
        /// US keyboards type **+** with Shift+=; the reference shows only Ctrl++.
        TEXT_LARGER_EQUAL: Command "text_larger_equal" = Binding::ctrl(Key::equal),
            live in both;
        THUMBNAILS_LARGER_EQUAL: Command "thumbnails_larger_equal" =
            Binding::ctrl_alt(Key::equal), live in both;
    }
}
