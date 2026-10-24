// SPDX-License-Identifier: MIT

use std::collections::HashSet;

use super::{
    super::{
        Binding,
        bindings::{self, Entry, Listed, Maps},
    },
    CONSTANTS, Command, Kind, Map, REGISTRY,
};

const MAPS: [Map; 2] = [Map::Default, Map::Tenxer];

const STRUCTURAL: &str = "a structural key, which joins the registry as an alias command";
const TEXT_EDITING: &str = "a text-editing key the listing also reads, kept as a fixed binding";
const GTK_OWNED: &str = "GTK acts on it once the dispatcher yields";
const EXTENDS_SELECTION: &str = "Shift with a motion key extends the selection and stays fixed";
const IGNORING: &str = "an .ignoring() matcher over another constant";
const REVERSED_SORT: &str = "Shift with a sort step reverses it";

/// Catalog constants that are not commands, and why.
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

/// Listed constants that work in both maps although the reference shows them
/// in one, and where.
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
];

fn constants(command: Command, map: Map) -> &'static [&'static str] {
    CONSTANTS[command as usize][map as usize]
}

fn specs_naming(name: &str) -> Vec<Command> {
    REGISTRY
        .iter()
        .map(|spec| spec.command)
        .filter(|command| {
            MAPS.iter()
                .any(|map| constants(*command, *map).contains(&name))
        })
        .collect()
}

fn excluded(name: &str) -> bool {
    NOT_COMMANDS.iter().any(|(excluded, _)| *excluded == name)
}

fn is_trigger_key(prefix: Binding, map: Map) -> bool {
    REGISTRY.iter().any(|spec| {
        matches!(spec.kind, Kind::Trigger(_)) && spec.command.defaults(map).contains(&prefix)
    })
}

#[test]
fn registry_covers_every_listed_constant() {
    let mut checked = 0;
    for constant in bindings::listed() {
        let name = constant.name;
        let live_in_both = LIVE_IN_BOTH_MAPS.iter().any(|(live, _)| *live == name);
        assert!(
            !(live_in_both && constant.maps == Maps::Both),
            "{name} is listed in both maps, so it needs no LIVE_IN_BOTH_MAPS entry"
        );
        let maps: Vec<Map> = MAPS
            .into_iter()
            .filter(|map| live_in_both || constant.maps.include(*map == Map::Tenxer))
            .collect();
        if let Entry::Digits(digits) = constant.entry {
            // Digit ranges stay fixed; a chord's range belongs to its trigger's group.
            if let Some(prefix) = digits.prefix() {
                assert!(
                    maps.iter().all(|map| is_trigger_key(prefix, *map)),
                    "{name} follows no chord trigger"
                );
            }
            continue;
        }
        let specs = specs_naming(name);
        if excluded(name) {
            assert!(specs.is_empty(), "{name} is excluded but in {specs:?}");
            continue;
        }
        let [command] = specs[..] else {
            panic!("{name} must be in exactly one spec, not {specs:?}");
        };
        for map in MAPS {
            assert_eq!(
                constants(command, map).contains(&name),
                maps.contains(&map),
                "{name} in the {map:?} map of {}",
                command.id()
            );
        }
        checked += 1;
    }
    assert!(checked > 0);
    for (name, _) in LIVE_IN_BOTH_MAPS {
        assert!(
            bindings::listed().any(|constant| constant.name == *name),
            "{name} is not a listed constant"
        );
    }
    for spec in REGISTRY {
        assert!(
            MAPS.iter()
                .any(|map| !spec.command.defaults(*map).is_empty()),
            "{} has no binding in either map",
            spec.id
        );
    }
}

#[test]
fn unlisted_constants_are_registered_or_excluded() {
    let mut checked = 0;
    for (name, _) in bindings::unlisted() {
        let specs = specs_naming(name);
        match (specs.len(), excluded(name)) {
            (0, true) | (1, false) => {}
            (0, false) => panic!("{name} needs a spec or a reason on the exclusion list"),
            _ => panic!("{name} must be in one spec or excluded, not both: {specs:?}"),
        }
        checked += 1;
    }
    assert!(checked > 0);
    let catalog: HashSet<&str> = bindings::listed()
        .map(|constant| constant.name)
        .chain(bindings::unlisted().map(|(name, _)| name))
        .collect();
    for (name, reason) in NOT_COMMANDS {
        assert!(
            catalog.contains(name),
            "{name} is excluded as {reason:?} but is not a catalog constant"
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
    for spec in REGISTRY {
        let command = spec.command;
        let id = command.id();
        assert_eq!(id, spec.id);
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
    let listed: Vec<Listed> = bindings::listed().collect();
    let after = |name: &str| {
        listed
            .iter()
            .find(|constant| constant.name == name)
            .and_then(|constant| constant.after)
    };
    let mut steps = 0;
    for spec in REGISTRY {
        let command = spec.command;
        let trigger = match command.kind() {
            Kind::Step(group) => {
                let Some(trigger) = Command::parse(group.id()) else {
                    panic!("{} has no {group:?} trigger", spec.id);
                };
                assert_eq!(trigger.kind(), Kind::Trigger(group), "{}", spec.id);
                steps += 1;
                Some(trigger)
            }
            Kind::Single | Kind::Trigger(_) => None,
        };
        for map in MAPS {
            for name in constants(command, map) {
                match trigger {
                    Some(trigger) => assert!(
                        after(name).is_some_and(|prefix| trigger.defaults(map).contains(&prefix)),
                        "{name} must follow {} in the {map:?} map",
                        trigger.id()
                    ),
                    None => assert!(
                        !after(name).is_some_and(|prefix| is_trigger_key(prefix, map)),
                        "{name} follows a chord trigger, so {} must be a step",
                        spec.id
                    ),
                }
            }
        }
    }
    assert!(steps > 0);
}
