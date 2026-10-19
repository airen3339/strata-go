// SPDX-License-Identifier: MIT

use super::bitset_positions;
use crate::services::SearchItem;
use gtk::prelude::*;
use std::{
    cell::{Cell, RefCell},
    collections::HashSet,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SelectionPolicy {
    /// Keep selected rows that still appear in the new result set.
    Retain,
    /// Select the first hit, used when the query itself changed.
    First,
}

pub(super) fn update_results(
    model: &gtk::StringList,
    results: &RefCell<Vec<SearchItem>>,
    selection: &gtk::MultiSelection,
    syncing: &Cell<bool>,
    items: Vec<SearchItem>,
    policy: SelectionPolicy,
) -> bool {
    let selected_paths: HashSet<_> = bitset_positions(&selection.selection())
        .into_iter()
        .filter_map(|position| {
            results
                .borrow()
                .get(position as usize)
                .map(|item| item.path.clone())
        })
        .collect();
    let was_syncing = syncing.replace(true);
    for (position, item) in items.iter().enumerate() {
        let existing = results.borrow()[position..].iter().position(|old| {
            old.path == item.path && old.name == item.name && old.is_directory == item.is_directory
        });
        match existing {
            Some(0) => {}
            Some(offset) => {
                // Row binding reads results synchronously during each model notification.
                results.borrow_mut().drain(position..position + offset);
                model.splice(position as u32, offset as u32, &[]);
            }
            None => {
                results.borrow_mut().insert(position, item.clone());
                model.splice(position as u32, 0, &[item.name.as_str()]);
            }
        }
    }
    let removed = results.borrow().len() - items.len();
    if removed > 0 {
        results.borrow_mut().truncate(items.len());
        model.splice(items.len() as u32, removed as u32, &[]);
    }
    let selected = gtk::Bitset::new_empty();
    match policy {
        SelectionPolicy::First if !items.is_empty() => {
            selected.add(0);
        }
        SelectionPolicy::First => {}
        SelectionPolicy::Retain => {
            for (position, item) in items.iter().enumerate() {
                if selected_paths.contains(&item.path) {
                    selected.add(position as u32);
                }
            }
        }
    }
    selection.set_selection(&selected, &gtk::Bitset::new_range(0, items.len() as u32));
    syncing.set(was_syncing);
    match policy {
        SelectionPolicy::First => !selected_paths.is_empty() || !items.is_empty(),
        SelectionPolicy::Retain => {
            let retained: HashSet<_> = items
                .iter()
                .filter(|item| selected_paths.contains(&item.path))
                .map(|item| item.path.clone())
                .collect();
            retained != selected_paths
        }
    }
}
