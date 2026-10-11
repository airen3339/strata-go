// SPDX-License-Identifier: MIT

use std::{cell::OnceCell, cmp::Ordering, collections::HashSet};

use super::LocationSet;
use crate::model::{FileEntry, Location};

/// A column's selected locations and, once known, the listing positions they occupy.
/// Changing the selection drops the positions; the column drops them whenever its
/// listing changes.
#[derive(Clone, Debug, Default)]
pub(super) struct SelectedLocations {
    set: LocationSet,
    // Ascending. Covers the whole selection only when its length matches `set`.
    positions: OnceCell<Vec<usize>>,
}

impl From<HashSet<Location>> for SelectedLocations {
    fn from(locations: HashSet<Location>) -> Self {
        Self {
            set: LocationSet::Explicit(locations),
            positions: OnceCell::new(),
        }
    }
}

impl SelectedLocations {
    pub(super) fn all_visible(entries: &[FileEntry], show_hidden: bool) -> Self {
        let mut selection = Self {
            set: LocationSet::all_visible(entries, show_hidden),
            positions: OnceCell::new(),
        };
        selection.remember(
            entries
                .iter()
                .enumerate()
                .filter(|(_, entry)| show_hidden || !entry.is_hidden)
                .map(|(position, _)| position)
                .collect(),
        );
        selection
    }

    pub(super) fn is_all(&self) -> bool {
        matches!(self.set, LocationSet::All { .. })
    }

    pub(super) fn is_empty(&self) -> bool {
        self.set.is_empty()
    }

    pub(super) fn len(&self) -> usize {
        self.set.len()
    }

    pub(super) fn contains(&self, location: &Location) -> bool {
        self.set.contains(location)
    }

    pub(super) fn materialize(&self, entries: &[FileEntry]) -> HashSet<Location> {
        self.set.materialize(entries)
    }

    pub(super) fn equals_explicit(
        &self,
        locations: &HashSet<Location>,
        entries: &[FileEntry],
    ) -> bool {
        self.set.equals_explicit(locations, entries)
    }

    pub(super) fn clear(&mut self) {
        *self = Self::default();
    }

    pub(super) fn insert(&mut self, location: Location) -> bool {
        let inserted = self.set.insert(location);
        if inserted {
            self.forget_positions();
        }
        inserted
    }

    pub(super) fn remove(&mut self, location: &Location) -> bool {
        let removed = self.set.remove(location);
        if removed {
            self.forget_positions();
        }
        removed
    }

    pub(super) fn excludes_new(&mut self, location: Location) {
        self.forget_positions();
        self.set.excludes_new(location);
    }

    pub(super) fn relocate(&mut self, from: &Location, to: Location) {
        self.forget_positions();
        self.set.relocate(from, to);
    }

    pub(super) fn retain(&mut self, entries: &[FileEntry], hide_hidden: bool) {
        self.forget_positions();
        self.set.retain(entries, hide_hidden);
    }

    pub(super) fn forget_positions(&mut self) {
        self.positions.take();
    }

    /// Positions in `entries` that hold the selection, in listing order. A lone
    /// selected `cursor` avoids scanning the listing.
    pub(super) fn positions(&self, entries: &[FileEntry], cursor: Option<usize>) -> &[usize] {
        if let Some(positions) = self.positions.get() {
            debug_assert!(
                positions.iter().all(|position| entries
                    .get(*position)
                    .is_some_and(|entry| self.set.contains(&entry.location))),
                "selection positions outlived a listing change"
            );
            return positions;
        }
        self.positions.get_or_init(|| {
            if self.set.is_empty() {
                return Vec::new();
            }
            // A select-all's count can drift from its listing, so only an explicit
            // set may trust it.
            if let Some(cursor) = cursor.filter(|cursor| {
                matches!(self.set, LocationSet::Explicit(_))
                    && self.set.len() == 1
                    && entries
                        .get(*cursor)
                        .is_some_and(|entry| self.set.contains(&entry.location))
            }) {
                return vec![cursor];
            }
            entries
                .iter()
                .enumerate()
                .filter(|(_, entry)| self.set.contains(&entry.location))
                .map(|(position, _)| position)
                .collect()
        })
    }

    /// The selection's positions when they are known to cover all of it.
    pub(super) fn exact_positions(&self) -> Option<&[usize]> {
        self.positions
            .get()
            .filter(|positions| positions.len() == self.set.len())
            .map(Vec::as_slice)
    }

    /// Selects exactly the items at `positions`, which must be ascending, distinct and
    /// within `entries`. An explicit selection with known positions changes by
    /// difference, so a drag over a large selection rehashes only the items that
    /// changed. A select-all becomes explicit on its first change.
    pub(super) fn select_positions(&mut self, entries: &[FileEntry], positions: Vec<usize>) {
        debug_assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
        let previous = self
            .positions
            .take()
            .filter(|previous| previous.len() == self.set.len());
        let difference = previous
            .as_deref()
            .map(|previous| sorted_difference(previous, &positions))
            .filter(|(removed, added)| removed.len() + added.len() < positions.len());
        match (difference, &mut self.set) {
            (Some((removed, added)), LocationSet::Explicit(locations)) => {
                for position in removed {
                    locations.remove(&entries[position].location);
                }
                locations.reserve(added.len());
                for position in added {
                    locations.insert(entries[position].location.clone());
                }
            }
            _ => {
                self.set = LocationSet::Explicit(
                    positions
                        .iter()
                        .map(|position| entries[*position].location.clone())
                        .collect(),
                );
            }
        }
        self.remember(positions);
    }

    /// Records the ascending positions of exactly the selected items. They are kept
    /// only when they cover the whole selection; a location the listing repeats is
    /// left to a later scan.
    pub(super) fn remember(&mut self, positions: Vec<usize>) {
        self.positions = if positions.len() == self.set.len() {
            OnceCell::from(positions)
        } else {
            OnceCell::new()
        };
    }
}

/// Positions only in `previous`, then positions only in `next`; both ascending.
fn sorted_difference(previous: &[usize], next: &[usize]) -> (Vec<usize>, Vec<usize>) {
    let (mut removed, mut added) = (Vec::new(), Vec::new());
    let (mut old, mut new) = (0, 0);
    loop {
        match (previous.get(old), next.get(new)) {
            (Some(left), Some(right)) => match left.cmp(right) {
                Ordering::Equal => {
                    old += 1;
                    new += 1;
                }
                Ordering::Less => {
                    removed.push(*left);
                    old += 1;
                }
                Ordering::Greater => {
                    added.push(*right);
                    new += 1;
                }
            },
            (Some(_), None) => {
                removed.extend_from_slice(&previous[old..]);
                return (removed, added);
            }
            (None, _) => {
                added.extend_from_slice(&next[new..]);
                return (removed, added);
            }
        }
    }
}
