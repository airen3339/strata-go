// SPDX-License-Identifier: MIT

//! Every rebindable command, with its stable id and its default keys in each
//! map. The catalog declares each command beside its area's other keys; a
//! press matches a command through the active map's table.

use std::{
    cell::RefCell,
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
    /// Escape, Enter, Tab or an arrow. Its first default binding is the
    /// canonical key its other bindings are read as; see [`super::alias`].
    Structural,
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

    /// The key a structural command's alternates are read as.
    pub(crate) fn canonical(self) -> Option<Binding> {
        (self.0.kind == Kind::Structural)
            .then(|| self.0.default.first().copied())
            .flatten()
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
    pub(crate) const fn ignoring(self, modifiers: Modifiers) -> Matcher {
        Matcher::new(self).ignoring(modifiers)
    }

    /// Matches `map`'s bindings whichever map is active, for a stage that
    /// claims another map's keys.
    pub(crate) const fn in_map(self, map: Map) -> Matcher {
        Matcher::new(self).in_map(map)
    }

    /// A structural command's other keys as pressed, whether or not they were
    /// read as the canonical key: **h** but not ← for `left`.
    pub(crate) const fn alternates(self) -> Matcher {
        Matcher::new(self).alternates()
    }

    /// A structural command, or one of its other keys where no alias applied,
    /// as for **h** in a popover.
    pub(crate) const fn or_alternates(self) -> Matcher {
        Matcher::new(self).or_alternates()
    }

    /// The one binding at `index` in the shown map, 0 being the primary, for a
    /// label that shows one alternate.
    pub(crate) const fn alternate(self, index: usize) -> Alternate {
        Alternate {
            command: self,
            index,
        }
    }

    pub(crate) const fn shown(self) -> Shown {
        Shown::Command(self)
    }
}

impl From<Command> for Matcher {
    fn from(command: Command) -> Self {
        Self::new(command)
    }
}

impl Match for Command {
    fn matches(&self, press: &KeyPress) -> bool {
        Matcher::new(*self).matches(press)
    }
}

/// A command matched in a fixed map, or with extra modifiers tolerated.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Matcher {
    command: Command,
    map: Option<Map>,
    ignored: Modifiers,
    alternates: Alternates,
}

/// Whether a structural matcher also compares the alternates as pressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Alternates {
    No,
    Only,
    Also,
}

impl Matcher {
    const fn new(command: Command) -> Self {
        Self {
            command,
            map: None,
            ignored: Modifiers::empty(),
            alternates: Alternates::No,
        }
    }

    pub(crate) const fn ignoring(self, modifiers: Modifiers) -> Self {
        Self {
            ignored: self.ignored.union(modifiers),
            ..self
        }
    }

    pub(crate) const fn in_map(self, map: Map) -> Self {
        Self {
            map: Some(map),
            ..self
        }
    }

    pub(crate) const fn alternates(self) -> Self {
        Self {
            alternates: Alternates::Only,
            ..self
        }
    }

    pub(crate) const fn or_alternates(self) -> Self {
        Self {
            alternates: Alternates::Also,
            ..self
        }
    }

    /// Structural commands compare the key a press reads as. The canonical
    /// key matches while the table lists it, and Escape always does. Ignored
    /// modifiers may also be held with an alternate, beside its own, except
    /// with plain printable ones, which Shift or a shortcut modifier turns
    /// into another key.
    fn matches_structural(&self, press: &KeyPress, canonical: Binding, entry: &[Binding]) -> bool {
        let others = || entry.iter().filter(move |binding| **binding != canonical);
        if self.alternates != Alternates::No
            && others().any(|binding| binding.matches_pressed(press))
        {
            return true;
        }
        if self.alternates == Alternates::Only {
            return false;
        }
        let read = canonical.ignoring(self.ignored);
        if press.aliased() {
            return read.matches_read(press);
        }
        ((entry.contains(&canonical) || self.command == super::alias::FALLBACK)
            && read.matches_read(press))
            || (!self.ignored.is_empty()
                && press.aliasing.allows(false)
                && others()
                    .filter(|binding| !binding.is_typed())
                    .any(|binding| {
                        binding
                            .ignoring(self.ignored.difference(binding.modifiers()))
                            .matches(press)
                    }))
    }
}

impl Match for Matcher {
    fn matches(&self, press: &KeyPress) -> bool {
        let keymap = active_keymap();
        let entry = keymap.bindings(self.map.unwrap_or_else(Map::active), self.command);
        match self.command.canonical() {
            Some(canonical) => self.matches_structural(press, canonical, entry),
            None => entry
                .iter()
                .any(|binding| binding.ignoring(self.ignored).matches(press)),
        }
    }
}

/// One alternate of a command, by its position in a map's entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Alternate {
    command: Command,
    index: usize,
}

impl Alternate {
    pub(crate) fn binding_in(self, map: Map) -> Option<Binding> {
        active_keymap()
            .bindings(map, self.command)
            .get(self.index)
            .copied()
    }

    pub(crate) const fn shown(self) -> Shown {
        Shown::Alternate(self)
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

    /// The same tables with `map`'s entry for `command` replaced, as a saved
    /// override replaces it.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "overrides are applied once they are saved")
    )]
    pub(crate) fn with(mut self, map: Map, command: Command, bindings: Vec<Binding>) -> Self {
        let table = match map {
            Map::Default => &mut self.default,
            Map::Tenxer => &mut self.tenxer,
        };
        table.insert(command, bindings);
        self
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
    static ACTIVE_KEYMAP: RefCell<Rc<Keymap>> = RefCell::new(Rc::new(Keymap::defaults()));
}

/// The tables presses and labels resolve through.
pub(crate) fn active_keymap() -> Rc<Keymap> {
    ACTIVE_KEYMAP.with(|keymap| keymap.borrow().clone())
}

/// Resolves presses and labels through `keymap` until the guard drops.
#[cfg(test)]
pub(crate) fn use_keymap(keymap: Keymap) -> KeymapGuard {
    KeymapGuard(ACTIVE_KEYMAP.with(|active| active.replace(Rc::new(keymap))))
}

#[cfg(test)]
#[must_use = "the keymap reverts when the guard drops"]
pub(crate) struct KeymapGuard(Rc<Keymap>);

#[cfg(test)]
impl Drop for KeymapGuard {
    fn drop(&mut self) {
        let previous = self.0.clone();
        ACTIVE_KEYMAP.with(|active| active.replace(previous));
    }
}

#[cfg(test)]
mod tests;
