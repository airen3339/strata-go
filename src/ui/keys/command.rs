// SPDX-License-Identifier: MIT

//! Every rebindable command, with its stable id and its default keys in each
//! map. The catalog declares each command beside its area's other keys; a
//! press matches a command through the active map's table.

use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
    rc::Rc,
};

use gtk::gdk::ModifierType as Modifiers;

use super::{Binding, KeyPress, Match, Shown, bindings};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Map {
    Default,
    Tenxer,
}

impl Map {
    /// The map the preferences select now, read at each press as handlers
    /// read 10xer mode. Without a live preference manager it is the default.
    pub(crate) fn active() -> Self {
        let tenxer = crate::ui::preferences::PreferenceManager::existing()
            .is_some_and(|preferences| preferences.tenxer_mode());
        if tenxer { Self::Tenxer } else { Self::Default }
    }
}

/// A fixed chord group. Its trigger and steps rebind; the group does not.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Group {
    Go,
    CopyAs,
    Sort,
    Actions,
    Tabs,
}

impl Group {
    /// The trigger's id and the prefix of each step's id.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "ids are read once overrides are saved")
    )]
    pub(crate) const fn id(self) -> &'static str {
        match self {
            Self::Go => "go",
            Self::CopyAs => "copy_as",
            Self::Sort => "sort",
            Self::Actions => "actions",
            Self::Tabs => "tabs",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Single,
    Trigger(Group),
    /// Matches only while its group's trigger is pending.
    Step(Group),
}

#[derive(Debug)]
pub(crate) struct Spec {
    pub(crate) id: &'static str,
    pub(crate) kind: Kind,
    /// Ordered bindings; the first is the one menus show.
    pub(crate) default: &'static [Binding],
    pub(crate) tenxer: &'static [Binding],
}

/// A rebindable action. Two commands are the same when their ids are.
#[derive(Clone, Copy)]
pub(crate) struct Command(&'static Spec);

impl PartialEq for Command {
    fn eq(&self, other: &Self) -> bool {
        self.0.id == other.0.id
    }
}

impl Eq for Command {}

impl Hash for Command {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.id.hash(state);
    }
}

impl std::fmt::Debug for Command {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "Command({})", self.0.id)
    }
}

impl Command {
    pub(crate) const fn new(spec: &'static Spec) -> Self {
        Self(spec)
    }

    /// The stable name settings files store.
    pub(crate) fn id(self) -> &'static str {
        self.0.id
    }

    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "settings files parse ids once overrides are saved"
        )
    )]
    pub(crate) fn parse(id: &str) -> Option<Self> {
        registry().find(|command| command.id() == id)
    }

    /// The shipped bindings in `map`, primary first; empty where the map
    /// leaves the command unbound.
    pub(crate) fn defaults(self, map: Map) -> &'static [Binding] {
        match map {
            Map::Default => self.0.default,
            Map::Tenxer => self.0.tenxer,
        }
    }

    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "only the registry tests read kinds so far")
    )]
    pub(crate) fn kind(self) -> Kind {
        self.0.kind
    }

    /// The bindings the active map's table holds, primary first.
    pub(crate) fn bindings(self) -> Vec<Binding> {
        self.bindings_in(Map::active())
    }

    pub(crate) fn bindings_in(self, map: Map) -> Vec<Binding> {
        active_keymap().bindings(map, self).to_vec()
    }

    /// The binding a menu shows, from the active map.
    pub(crate) fn primary(self) -> Option<Binding> {
        self.bindings().first().copied()
    }

    /// Also matches while any of `modifiers` is held, for each binding.
    pub(crate) const fn ignoring(self, modifiers: Modifiers) -> Ignoring {
        Ignoring {
            command: self,
            ignored: modifiers,
        }
    }

    /// Matches `map`'s bindings whichever map is active, for a stage that
    /// claims another map's keys.
    pub(crate) const fn in_map(self, map: Map) -> InMap {
        InMap { command: self, map }
    }

    pub(crate) const fn shown(self) -> Shown {
        Shown::Command(self)
    }
}

impl Match for Command {
    fn matches(&self, press: &KeyPress) -> bool {
        self.in_map(Map::active()).matches(press)
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Ignoring {
    command: Command,
    ignored: Modifiers,
}

impl Match for Ignoring {
    fn matches(&self, press: &KeyPress) -> bool {
        active_keymap()
            .bindings(Map::active(), self.command)
            .iter()
            .any(|binding| binding.ignoring(self.ignored).matches(press))
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct InMap {
    command: Command,
    map: Map,
}

impl Match for InMap {
    fn matches(&self, press: &KeyPress) -> bool {
        active_keymap()
            .bindings(self.map, self.command)
            .iter()
            .any(|binding| binding.matches(press))
    }
}

/// Every command the catalog declares.
pub(crate) fn registry() -> impl Iterator<Item = Command> {
    bindings::entries().filter_map(|entry| match entry {
        bindings::Entry::Command(command) => Some(command),
        bindings::Entry::Binding(_) | bindings::Entry::Digits(_) => None,
    })
}

/// Both maps' tables: each command's bindings, primary first.
#[derive(Debug)]
pub(crate) struct Keymap {
    default: HashMap<Command, Vec<Binding>>,
    tenxer: HashMap<Command, Vec<Binding>>,
}

impl Keymap {
    pub(crate) fn defaults() -> Self {
        let table = |map| {
            registry()
                .map(|command| (command, command.defaults(map).to_vec()))
                .collect()
        };
        Self {
            default: table(Map::Default),
            tenxer: table(Map::Tenxer),
        }
    }

    pub(crate) fn bindings(&self, map: Map, command: Command) -> &[Binding] {
        let table = match map {
            Map::Default => &self.default,
            Map::Tenxer => &self.tenxer,
        };
        table.get(&command).map_or(&[], Vec::as_slice)
    }
}

thread_local! {
    static ACTIVE_KEYMAP: Rc<Keymap> = Rc::new(Keymap::defaults());
}

/// The tables presses and labels resolve through.
pub(crate) fn active_keymap() -> Rc<Keymap> {
    ACTIVE_KEYMAP.with(Rc::clone)
}

#[cfg(test)]
mod tests;
