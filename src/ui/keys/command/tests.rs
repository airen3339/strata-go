// SPDX-License-Identifier: MIT

use std::collections::HashSet;

use gtk::gdk::{Key, ModifierType as Modifiers};

use super::{
    super::{
        Binding, KeyPress, Shown,
        bindings::{self, Entry, Listed, Maps},
    },
    Command, Group, Kind, Map, registry,
};

const MAPS: [Map; 2] = [Map::Default, Map::Tenxer];

const MANY_ROLES: &str =
    "it closes a pane, leaves the sidebar, clears a filter and edits text; kept as a fixed binding";
const GENERIC: &str = "a key many surfaces read for their own job, kept as a fixed binding";
const GTK_OWNED: &str = "GTK acts on it once the dispatcher yields";
const EXTENDS_SELECTION: &str = "Shift with a motion key extends the selection and stays fixed";
const REVERSED_SORT: &str = "Shift with a sort step reverses it";

/// Catalog bindings that are not commands, and why.
const NOT_COMMANDS: &[(&str, &str)] = &[
    ("BACKSPACE", MANY_ROLES),
    ("SPACE", GENERIC),
    ("HOME_KEY", GENERIC),
    ("END_KEY", GENERIC),
    ("PAGE_UP_KEY", GENERIC),
    ("PAGE_DOWN_KEY", GENERIC),
    ("EXTEND_UP", EXTENDS_SELECTION),
    ("EXTEND_DOWN", EXTENDS_SELECTION),
    ("EXTEND_PAGE_UP", EXTENDS_SELECTION),
    ("EXTEND_PAGE_DOWN", EXTENDS_SELECTION),
    ("EXTEND_LEFT", EXTENDS_SELECTION),
    ("EXTEND_RIGHT", EXTENDS_SELECTION),
    ("EXTEND_TO_FIRST", EXTENDS_SELECTION),
    ("EXTEND_TO_LAST", EXTENDS_SELECTION),
    ("SELECTION_TOGGLE", GTK_OWNED),
    ("UNSELECT_ALL", GTK_OWNED),
    ("DOCUMENT_START", GTK_OWNED),
    ("DOCUMENT_END", GTK_OWNED),
    ("SORT_BY_NAME_REVERSED", REVERSED_SORT),
    ("SORT_BY_MODIFIED_REVERSED", REVERSED_SORT),
    ("SORT_BY_SIZE_REVERSED", REVERSED_SORT),
    ("SORT_BY_TYPE_REVERSED", REVERSED_SORT),
];

const TIMELINE: &str = "the media timeline reads it in either map";

/// Listed commands bound in both maps although the reference shows them in
/// one, and why.
const LIVE_IN_BOTH_MAPS: &[(&str, &str)] = &[
    ("COMPLETE", "location bar path completion"),
    ("CHOOSE_FOLDER", "folder requests in the file chooser"),
    (
        "TERMINAL",
        "the listing and sidebar open a terminal; only text fields leave it unbound in 10xer",
    ),
    ("MEDIA_SEEK_BACK_PAGE", TIMELINE),
    ("MEDIA_SEEK_FORWARD_PAGE", TIMELINE),
    ("MEDIA_SEEK_START", TIMELINE),
    ("MEDIA_VOLUME_STEP_UP", TIMELINE),
    ("MEDIA_VOLUME_STEP_DOWN", TIMELINE),
    ("MEDIA_SEEK_END", TIMELINE),
    (
        "FIND_IN_PREVIEW",
        "a focused preview opens find with Ctrl+F; the default reference lists no preview keys",
    ),
];

fn bound_maps(command: Command) -> Vec<Map> {
    MAPS.into_iter()
        .filter(|map| !command.defaults(*map).is_empty())
        .collect()
}

fn trigger(group: Group) -> Option<Command> {
    registry().find(|command| command.kind() == Kind::Trigger(group))
}

fn is_trigger_key(prefix: Shown, map: Map) -> bool {
    let Shown::Command(prefix) = prefix else {
        return false;
    };
    matches!(prefix.kind(), Kind::Trigger(_)) && !prefix.defaults(map).is_empty()
}

#[test]
fn listed_commands_are_bound_in_the_maps_that_list_them() {
    let mut checked = 0;
    for constant in bindings::listed() {
        let name = constant.name;
        let block: Vec<Map> = MAPS
            .into_iter()
            .filter(|map| constant.maps.include(*map == Map::Tenxer))
            .collect();
        let live_in_both = LIVE_IN_BOTH_MAPS.iter().any(|(live, _)| *live == name);
        match constant.entry {
            Entry::Command(command) => {
                let expected = if live_in_both { MAPS.to_vec() } else { block };
                assert_eq!(bound_maps(command), expected, "{name} ({})", command.id());
                assert!(
                    !(live_in_both && constant.maps == Maps::Both),
                    "{name} is listed in both maps, so it needs no LIVE_IN_BOTH_MAPS entry"
                );
                checked += 1;
            }
            // Digit ranges stay fixed; a chord's range belongs to its trigger's group.
            Entry::Digits(digits) => {
                if let Some(prefix) = digits.prefix() {
                    assert!(
                        block.iter().all(|map| is_trigger_key(prefix, *map)),
                        "{name} follows no chord trigger"
                    );
                }
            }
            Entry::Binding(_) => assert!(
                NOT_COMMANDS.iter().any(|(excluded, _)| *excluded == name),
                "{name} is a fixed binding; make it a command or give it a reason"
            ),
        }
    }
    assert!(checked > 0);
    for (name, _) in LIVE_IN_BOTH_MAPS {
        assert!(
            bindings::listed()
                .any(|constant| constant.name == *name
                    && matches!(constant.entry, Entry::Command(_))),
            "{name} is not a listed command"
        );
    }
}

#[test]
fn unlisted_constants_are_commands_or_excluded() {
    let mut checked = 0;
    for (name, entry) in bindings::unlisted() {
        match entry {
            Entry::Command(command) => assert!(
                !bound_maps(command).is_empty(),
                "{} has no binding in either map",
                command.id()
            ),
            Entry::Binding(_) => assert!(
                NOT_COMMANDS.iter().any(|(excluded, _)| *excluded == name),
                "{name} needs to be a command or have a reason on the exclusion list"
            ),
            Entry::Digits(_) => panic!("{name} is an unlisted digit range"),
        }
        checked += 1;
    }
    assert!(checked > 0);
    let fixed: HashSet<&str> = bindings::listed()
        .map(|constant| (constant.name, constant.entry))
        .chain(bindings::unlisted())
        .filter(|(_, entry)| matches!(entry, Entry::Binding(_)))
        .map(|(name, _)| name)
        .collect();
    for (name, reason) in NOT_COMMANDS {
        assert!(
            fixed.contains(name),
            "{name} is excluded as {reason:?} but is not a fixed catalog binding"
        );
    }
}

#[test]
fn ids_are_unique_and_round_trip() {
    fn word(text: &str) -> bool {
        text.starts_with(|character: char| character.is_ascii_lowercase())
            && text.chars().all(|character| {
                character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_'
            })
    }
    let mut ids = HashSet::new();
    for command in registry() {
        let id = command.id();
        assert!(ids.insert(id), "{id} is used twice");
        assert_eq!(Command::parse(id), Some(command));
        let well_formed = match command.kind() {
            Kind::Single | Kind::Structural => word(id),
            Kind::Trigger(group) => word(id) && id == group.id(),
            Kind::Step(group) => id
                .split_once('.')
                .is_some_and(|(prefix, step)| prefix == group.id() && word(step)),
        };
        assert!(well_formed, "{id} does not fit {:?}", command.kind());
    }
    assert!(!ids.is_empty());
    for unknown in ["", "Copy", "go.", ".home", "go.home.", "future_command"] {
        assert_eq!(Command::parse(unknown), None, "{unknown:?}");
    }
}

#[test]
fn chord_steps_name_their_trigger() {
    let mut steps = 0;
    for command in registry() {
        match command.kind() {
            Kind::Step(group) => {
                let Some(trigger) = trigger(group) else {
                    panic!("{} has no {group:?} trigger", command.id());
                };
                for map in bound_maps(command) {
                    assert!(
                        !trigger.defaults(map).is_empty(),
                        "{} is bound in the {map:?} map, where {} is not",
                        command.id(),
                        trigger.id()
                    );
                }
                steps += 1;
            }
            Kind::Trigger(group) => assert_eq!(
                registry()
                    .filter(|other| other.kind() == Kind::Trigger(group))
                    .count(),
                1,
                "{group:?} has more than one trigger"
            ),
            Kind::Single | Kind::Structural => {}
        }
    }
    assert!(steps > 0);
    let listed: Vec<Listed> = bindings::listed().collect();
    for constant in &listed {
        if let Some(after) = constant.after {
            assert!(
                matches!(after.kind(), Kind::Single),
                "{} follows a chord trigger, so it must be a step",
                constant.name
            );
        }
    }
}

/// A command matches the active map's bindings, which keeps both tables apart
/// even where a key means different things in each.
#[test]
fn commands_match_through_the_active_map() {
    crate::test_support::gtk_test(
        "ui::keys::command::tests::commands_match_through_the_active_map",
        || {
            use super::super::bindings::{
                DUPLICATE, GO_PROMPT, HALF_PAGE_DOWN, MOVE_TO_TRASH, OPEN_IN_NEW_TAB, RENAME,
                SIDEBAR,
            };
            let press = |key, modifiers| KeyPress::from_signal(key, 0, modifiers);
            let ctrl = Modifiers::CONTROL_MASK;
            let preferences = crate::ui::preferences::PreferenceManager::shared();
            for tenxer in [false, true] {
                preferences.set_tenxer_mode(tenxer);
                assert_eq!(
                    Map::active(),
                    if tenxer { Map::Tenxer } else { Map::Default }
                );
                let ctrl_b = press(Key::b, ctrl);
                assert_eq!(ctrl_b.is(SIDEBAR), !tenxer);
                assert!(ctrl_b.is(SIDEBAR.in_map(Map::Default)));
                assert_eq!(press(Key::n, ctrl).is(SIDEBAR), tenxer);
                assert_eq!(press(Key::d, ctrl).is(HALF_PAGE_DOWN), tenxer);
                assert_eq!(press(Key::d, ctrl).is(DUPLICATE), !tenxer);
                assert!(press(Key::d, ctrl).is(DUPLICATE.in_map(Map::Default)));
                assert!(press(Key::F2, Modifiers::empty()).is(RENAME));
                assert_eq!(press(Key::r, Modifiers::empty()).is(RENAME), tenxer);
                assert_eq!(press(Key::r, ctrl).is(RENAME), !tenxer);
                assert!(press(Key::r, ctrl).is(RENAME.in_map(Map::Default)));
                let shift_space = press(Key::space, Modifiers::SHIFT_MASK);
                assert_eq!(
                    shift_space.is(GO_PROMPT.ignoring(Modifiers::SHIFT_MASK)),
                    tenxer
                );
                assert!(!shift_space.is(GO_PROMPT));
                let ctrl_shift_enter = press(Key::Return, ctrl | Modifiers::SHIFT_MASK);
                assert!(!ctrl_shift_enter.is(OPEN_IN_NEW_TAB));
                assert!(ctrl_shift_enter.is(OPEN_IN_NEW_TAB.ignoring(Modifiers::SHIFT_MASK)));
                let trash = MOVE_TO_TRASH
                    .in_map(Map::Default)
                    .ignoring(Modifiers::SHIFT_MASK);
                assert!(press(Key::Delete, Modifiers::SHIFT_MASK).is(trash));
                assert!(!press(Key::d, Modifiers::empty()).is(trash));
            }
            preferences.set_tenxer_mode(false);
        },
    );
}

/// Each command that replaced per-map or alternate constants, with every key
/// each map held before, primary first. Written out rather than derived, so an
/// alternate cannot leak into the other map: Ctrl+R inverts the selection in
/// 10xer, and the chooser and popovers that rename with it there read the
/// default map.
#[test]
fn merged_commands_keep_each_maps_key_set() {
    use super::super::bindings::{
        CONTEXT_MENU, DELETE_PERMANENTLY, FILTER, FIND_IN_PREVIEW, HIDDEN_FILES, HISTORY_BACK,
        HISTORY_FORWARD, LAST_ITEM, MEDIA_VOLUME_STEP_UP, MOVE_TO_TRASH, PAGE_DOWN, PAGE_UP,
        PREVIEW, REDO, REFERENCE, RENAME, SIDEBAR, TAB_NEXT, TAB_PREVIOUS, TEXT_LARGER,
        THUMBNAILS_LARGER, THUMBNAILS_SMALLER, UNYANK,
    };
    const fn plain(key: Key) -> Binding {
        Binding::plain(key)
    }
    const fn shift(key: Key) -> Binding {
        Binding::shift(key)
    }
    const fn ctrl(key: Key) -> Binding {
        Binding::ctrl(key)
    }
    const fn ctrl_alt(key: Key) -> Binding {
        Binding::ctrl_alt(key)
    }
    const REDO_KEYS: &[Binding] = &[Binding::ctrl_shift(Key::z), ctrl(Key::y)];
    const CONTEXT_MENU_KEYS: &[Binding] = &[plain(Key::Menu), shift(Key::F10)];
    const TEXT_LARGER_KEYS: &[Binding] = &[ctrl(Key::plus), ctrl(Key::equal)];
    const TAB_NEXT_KEYS: &[Binding] = &[ctrl(Key::Tab), ctrl(Key::Page_Down)];
    const TAB_PREVIOUS_KEYS: &[Binding] = &[Binding::ctrl_shift(Key::Tab), ctrl(Key::Page_Up)];
    const VOLUME_UP_KEYS: &[Binding] = &[plain(Key::plus), plain(Key::equal)];
    let cases: [(Command, &[Binding], &[Binding]); 23] = [
        (REDO, REDO_KEYS, REDO_KEYS),
        (
            RENAME,
            &[plain(Key::F2), ctrl(Key::r)],
            &[plain(Key::r), plain(Key::F2)],
        ),
        (CONTEXT_MENU, CONTEXT_MENU_KEYS, CONTEXT_MENU_KEYS),
        (
            MOVE_TO_TRASH,
            &[plain(Key::Delete)],
            &[plain(Key::d), plain(Key::Delete)],
        ),
        (
            DELETE_PERMANENTLY,
            &[shift(Key::Delete)],
            &[shift(Key::d), shift(Key::Delete)],
        ),
        (
            HIDDEN_FILES,
            &[ctrl(Key::h), ctrl(Key::period)],
            &[ctrl(Key::h), ctrl(Key::period), plain(Key::period)],
        ),
        (FILTER, &[ctrl(Key::f)], &[plain(Key::f)]),
        (TEXT_LARGER, TEXT_LARGER_KEYS, TEXT_LARGER_KEYS),
        (
            THUMBNAILS_LARGER,
            &[ctrl_alt(Key::plus), ctrl_alt(Key::equal)],
            &[
                ctrl_alt(Key::plus),
                ctrl_alt(Key::equal),
                plain(Key::plus),
                plain(Key::equal),
            ],
        ),
        (
            THUMBNAILS_SMALLER,
            &[ctrl_alt(Key::minus)],
            &[ctrl_alt(Key::minus), plain(Key::minus)],
        ),
        (SIDEBAR, &[ctrl(Key::b)], &[ctrl(Key::n)]),
        (
            REFERENCE,
            &[plain(Key::F1)],
            &[plain(Key::F1), plain(Key::asciitilde)],
        ),
        (TAB_NEXT, TAB_NEXT_KEYS, TAB_NEXT_KEYS),
        (TAB_PREVIOUS, TAB_PREVIOUS_KEYS, TAB_PREVIOUS_KEYS),
        (
            LAST_ITEM,
            &[plain(Key::End)],
            &[shift(Key::g), plain(Key::End)],
        ),
        (
            PAGE_UP,
            &[plain(Key::Page_Up)],
            &[plain(Key::Page_Up), ctrl(Key::b), ctrl(Key::Page_Up)],
        ),
        (
            PAGE_DOWN,
            &[plain(Key::Page_Down)],
            &[plain(Key::Page_Down), ctrl(Key::f), ctrl(Key::Page_Down)],
        ),
        (
            HISTORY_BACK,
            &[Binding::alt(Key::Left)],
            &[Binding::alt(Key::Left), shift(Key::h)],
        ),
        (
            HISTORY_FORWARD,
            &[Binding::alt(Key::Right)],
            &[Binding::alt(Key::Right), shift(Key::l)],
        ),
        (PREVIEW, &[plain(Key::space)], &[plain(Key::i)]),
        (UNYANK, &[], &[shift(Key::y), shift(Key::x)]),
        (FIND_IN_PREVIEW, &[ctrl(Key::f)], &[plain(Key::slash)]),
        (MEDIA_VOLUME_STEP_UP, VOLUME_UP_KEYS, VOLUME_UP_KEYS),
    ];
    for (command, default, tenxer) in cases {
        assert_eq!(command.defaults(Map::Default), default, "{}", command.id());
        assert_eq!(command.defaults(Map::Tenxer), tenxer, "{}", command.id());
    }
}

#[test]
fn no_command_repeats_a_binding() {
    let mut checked = 0;
    for command in registry() {
        for map in MAPS {
            let bindings = command.defaults(map);
            for (index, binding) in bindings.iter().enumerate() {
                assert!(
                    !bindings[..index].contains(binding),
                    "{} lists {binding:?} twice in the {map:?} map",
                    command.id()
                );
                checked += 1;
            }
        }
    }
    assert!(checked > 0);
}
