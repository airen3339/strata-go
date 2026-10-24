// SPDX-License-Identifier: MIT

use std::collections::HashSet;

use gtk::gdk::{Key, ModifierType as Modifiers};

use super::{
    super::{
        KeyPress, Shown,
        bindings::{self, Entry, Listed, Maps},
    },
    Command, Group, Kind, Map, registry,
};

const MAPS: [Map; 2] = [Map::Default, Map::Tenxer];

const STRUCTURAL: &str = "a structural key, which joins the registry as an alias command";
const TEXT_EDITING: &str = "a text-editing key the listing also reads, kept as a fixed binding";
const GENERIC: &str = "a key many surfaces read for their own job, kept as a fixed binding";
const GTK_OWNED: &str = "GTK acts on it once the dispatcher yields";
const EXTENDS_SELECTION: &str = "Shift with a motion key extends the selection and stays fixed";
const IGNORING: &str = "an .ignoring() matcher over another constant";
const REVERSED_SORT: &str = "Shift with a sort step reverses it";

/// Catalog bindings that are not commands, and why.
const NOT_COMMANDS: &[(&str, &str)] = &[
    ("UP", STRUCTURAL),
    ("DOWN", STRUCTURAL),
    ("LEFT", STRUCTURAL),
    ("RIGHT", STRUCTURAL),
    ("VIM_UP", STRUCTURAL),
    ("VIM_DOWN", STRUCTURAL),
    ("VIM_LEFT", STRUCTURAL),
    ("VIM_RIGHT", STRUCTURAL),
    ("ENTER", STRUCTURAL),
    ("ESCAPE", STRUCTURAL),
    ("FOCUS_NEXT", STRUCTURAL),
    ("FOCUS_PREVIOUS", STRUCTURAL),
    ("BACKSPACE", TEXT_EDITING),
    ("DELETE", TEXT_EDITING),
    ("DELETE_PERMANENTLY", TEXT_EDITING),
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
    ("ESCAPE_ANY", IGNORING),
    ("ESCAPE_ANY_SHIFT", IGNORING),
    ("ENTER_ANY", IGNORING),
    ("ENTER_ANY_SHIFT", IGNORING),
    ("SORT_BY_NAME_REVERSED", REVERSED_SORT),
    ("SORT_BY_MODIFIED_REVERSED", REVERSED_SORT),
    ("SORT_BY_SIZE_REVERSED", REVERSED_SORT),
    ("SORT_BY_TYPE_REVERSED", REVERSED_SORT),
];

const TIMELINE: &str = "the media timeline reads it in either map";
const FALLS_THROUGH: &str =
    "its handler runs in 10xer wherever 10xer's own keys leave the press, as in popovers";

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
    ("TENXER_LAST_ITEM", TIMELINE),
    (
        "PREVIEW",
        "the file chooser and keys the 10xer listing leaves alone still preview with Space",
    ),
    ("DUPLICATE", FALLS_THROUGH),
    ("RENAME_R", FALLS_THROUGH),
    ("JUMP_TO_FOLDER", FALLS_THROUGH),
    ("FILTER", FALLS_THROUGH),
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
            Kind::Single => word(id),
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
            Kind::Single => {}
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
                DUPLICATE, GO_PROMPT, HALF_PAGE_DOWN, OPEN_IN_NEW_TAB, SIDEBAR, TENXER_SIDEBAR,
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
                assert_eq!(press(Key::n, ctrl).is(TENXER_SIDEBAR), tenxer);
                assert_eq!(press(Key::d, ctrl).is(HALF_PAGE_DOWN), tenxer);
                assert!(press(Key::d, ctrl).is(DUPLICATE));
                let shift_space = press(Key::space, Modifiers::SHIFT_MASK);
                assert_eq!(
                    shift_space.is(GO_PROMPT.ignoring(Modifiers::SHIFT_MASK)),
                    tenxer
                );
                assert!(!shift_space.is(GO_PROMPT));
                let ctrl_shift_enter = press(Key::Return, ctrl | Modifiers::SHIFT_MASK);
                assert!(!ctrl_shift_enter.is(OPEN_IN_NEW_TAB));
                assert!(ctrl_shift_enter.is(OPEN_IN_NEW_TAB.ignoring(Modifiers::SHIFT_MASK)));
            }
            preferences.set_tenxer_mode(false);
        },
    );
}
