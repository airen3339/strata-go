// SPDX-License-Identifier: MIT
//! The "Run Script" modal: the Nautilus Scripts folder as a native dialog.

use std::{
    collections::HashMap,
    ffi::OsString,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    rc::Rc,
};

use gtk::{glib, prelude::*};

use super::{
    blur::BlurBin,
    browser::{dismiss_modal_layer, modal_layer, show_error_dialog},
    controls::modal_layout,
    keys::{
        self, SHORTCUT_MODIFIERS,
        bindings::{BACKSPACE, DOWN, ESCAPE_ANY, UP},
    },
    open_with::install_list_tab_navigation,
    terminal::spawn_reaped,
};

/// `errno` for a program without a usable shebang; those run through `/bin/sh`.
const ENOEXEC: i32 = 8;
const GROUP_SEPARATOR: &str = " ▸ ";

pub(crate) struct ScriptEntry {
    pub name: String,
    pub path: PathBuf,
    /// Relative directory from the scripts root, joined with ` ▸ `; empty for
    /// scripts placed directly in the root.
    pub group: String,
    haystack: String,
}

pub(crate) fn scripts_root() -> PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|base| base.is_absolute())
        .unwrap_or_else(|| glib::home_dir().join(".local/share"));
    base.join("nautilus/scripts")
}

fn executable_file(metadata: &std::fs::Metadata) -> bool {
    metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
}

fn group_label(root: &Path, directory: &Path) -> String {
    directory
        .strip_prefix(root)
        .ok()
        .filter(|relative| !relative.as_os_str().is_empty())
        .map(|relative| {
            relative
                .components()
                .map(|component| component.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join(GROUP_SEPARATOR)
        })
        .unwrap_or_default()
}

/// Hidden entries are skipped, directory symlinks are not descended (cycle
/// safety), and symlinked scripts are followed. Only executable regular
/// files are offered, mirroring what can actually run. Loose root scripts
/// come first, then grouped categories, each alphabetical.
pub(crate) fn enumerate_scripts(root: &Path) -> Vec<ScriptEntry> {
    let mut entries = Vec::new();
    walk_scripts(root, root, &mut entries);
    entries.sort_by(|left, right| {
        (
            left.group.is_empty(),
            left.group.to_lowercase(),
            left.name.to_lowercase(),
        )
            .cmp(&(
                right.group.is_empty(),
                right.group.to_lowercase(),
                right.name.to_lowercase(),
            ))
    });
    entries
}

fn walk_scripts(root: &Path, directory: &Path, out: &mut Vec<ScriptEntry>) {
    let Ok(children) = std::fs::read_dir(directory) else {
        return;
    };
    let mut children: Vec<(OsString, PathBuf)> = children
        .filter_map(|child| child.ok().map(|child| (child.file_name(), child.path())))
        .collect();
    children.sort_by(|left, right| left.0.cmp(&right.0));
    for (name, path) in children {
        let display = name.to_string_lossy().to_string();
        if display.starts_with('.') {
            continue;
        }
        let Ok(link) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if link.is_dir() {
            walk_scripts(root, &path, out);
            continue;
        }
        let Ok(followed) = std::fs::metadata(&path) else {
            continue;
        };
        if !executable_file(&followed) {
            continue;
        }
        let group = group_label(root, directory);
        let haystack = format!("{group} {display}").to_lowercase();
        out.push(ScriptEntry {
            name: display,
            path,
            group,
            haystack,
        });
    }
}

/// The environment Nautilus sets when a script starts. Paths stay byte-exact
/// in `OsString`s; URIs are percent-encoded by GLib. Selection lists carry a
/// trailing newline, like Nautilus.
pub(crate) fn nautilus_environment(
    selection: &[PathBuf],
    folder: &Path,
) -> Result<Vec<(&'static str, OsString)>, glib::Error> {
    let mut paths = OsString::new();
    let mut uris = OsString::new();
    for path in selection {
        paths.push(path.as_os_str());
        paths.push("\n");
        uris.push(glib::filename_to_uri(path, None)?.as_str());
        uris.push("\n");
    }
    let current = glib::filename_to_uri(folder, None)?.to_string();
    Ok(vec![
        ("NAUTILUS_SCRIPT_SELECTED_FILE_PATHS", paths),
        ("NAUTILUS_SCRIPT_SELECTED_URIS", uris),
        ("NAUTILUS_SCRIPT_CURRENT_URI", OsString::from(current)),
        ("NAUTILUS_SCRIPT_WINDOW_GEOMETRY", OsString::new()),
    ])
}

/// Starts the script in `folder` with no arguments, like Nautilus. Standard
/// streams are detached so a long-running script never blocks Strata.
pub(crate) fn spawn_script(
    path: &Path,
    folder: &Path,
    selection: &[PathBuf],
) -> Result<Child, String> {
    let environment = nautilus_environment(selection, folder)
        .map_err(|error| format!("Unable to build the script environment: {error}"))?;
    let build = |shell: bool| {
        let mut command = if shell {
            let mut command = Command::new("/bin/sh");
            command.arg(path);
            command
        } else {
            Command::new(path)
        };
        command
            .current_dir(folder)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        for (key, value) in &environment {
            command.env(key, value);
        }
        crate::child_environment::restore_command(&mut command);
        command.spawn()
    };
    match build(false) {
        Ok(child) => Ok(child),
        Err(error) if error.raw_os_error() == Some(ENOEXEC) => {
            build(true).map_err(|error| format!("Unable to run script: {error}"))
        }
        Err(error) => Err(format!("Unable to run script: {error}")),
    }
}

pub(in crate::ui) fn has_scripts() -> bool {
    !enumerate_scripts(&scripts_root()).is_empty()
}

struct ScriptRow {
    entry: ScriptEntry,
    row: gtk::ListBoxRow,
}

fn create_script_row(entry: &ScriptEntry) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.add_css_class("open-with-row");
    super::accessibility::set_label(&row, &entry.name);
    super::accessibility::set_description(&row, Some(&entry.group));
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    let icon = gtk::Image::new();
    crate::assets::set_primary_icon(&icon, crate::assets::icons::TERMINAL);
    icon.set_pixel_size(22);
    icon.add_css_class("open-with-icon");
    let labels = gtk::Box::new(gtk::Orientation::Vertical, 0);
    labels.set_valign(gtk::Align::Center);
    let name = gtk::Label::new(Some(&entry.name));
    name.add_css_class("open-with-name");
    name.set_xalign(0.0);
    name.set_ellipsize(gtk::pango::EllipsizeMode::End);
    let description = gtk::Label::new(Some(&entry.group));
    description.add_css_class("open-with-description");
    description.set_xalign(0.0);
    description.set_ellipsize(gtk::pango::EllipsizeMode::End);
    labels.append(&name);
    if !entry.group.is_empty() {
        labels.append(&description);
    }
    content.append(&icon);
    content.append(&labels);
    row.set_child(Some(&content));
    row
}

fn create_group_heading(title: &str) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.add_css_class("open-with-heading-row");
    row.set_activatable(false);
    row.set_selectable(false);
    row.set_focusable(false);
    row.set_can_focus(false);
    let label = gtk::Label::new(Some(title));
    label.add_css_class("open-with-heading");
    label.set_xalign(0.0);
    row.set_child(Some(&label));
    row
}

pub(in crate::ui) fn show(
    parent: &impl IsA<gtk::Widget>,
    selection: Vec<PathBuf>,
    folder: PathBuf,
    on_close: Rc<dyn Fn()>,
) {
    let Some(window_overlay) = parent
        .root()
        .and_downcast::<gtk::Window>()
        .and_then(|window| window.child())
        .and_downcast::<gtk::Overlay>()
    else {
        return;
    };
    let blurred_root = window_overlay.child().and_downcast::<BlurBin>();
    if let Some(root) = blurred_root.as_ref() {
        root.set_blurred(true);
    }

    let subtitle = crate::i18n::tr(if selection.is_empty() {
        "Choose a script to run in this folder"
    } else {
        "Choose a script to run on the selection"
    });
    let layout = modal_layout(
        crate::assets::icons::TERMINAL,
        &crate::i18n::tr("Run Script"),
        &subtitle,
        &crate::i18n::tr("Run"),
    );
    layout.content.add_css_class("open-with-dialog");
    layout.content.set_size_request(480, 460);
    layout.body.set_vexpand(true);

    let search_entry = gtk::SearchEntry::new();
    search_entry.add_css_class("open-with-search");
    search_entry.set_placeholder_text(Some(&crate::i18n::tr("Search scripts…")));
    layout.body.append(&search_entry);

    let list = gtk::ListBox::new();
    list.add_css_class("open-with-list");
    list.set_selection_mode(gtk::SelectionMode::Single);
    list.set_activate_on_single_click(false);
    super::accessibility::set_label(&list, &crate::i18n::tr("Scripts"));

    // Hidden; keeps the shared tab-navigation helper's widget set complete.
    let always_use = gtk::CheckButton::new();
    always_use.set_visible(false);

    install_list_tab_navigation(
        &layout.content,
        &search_entry,
        &list,
        &always_use,
        &layout.close,
        &layout.cancel,
    );

    let entries = enumerate_scripts(&scripts_root());
    let mut rows: Vec<ScriptRow> = Vec::new();
    let mut headings: Vec<(gtk::ListBoxRow, String)> = Vec::new();
    let mut last_group: Option<String> = None;
    for entry in entries {
        if last_group.as_deref() != Some(entry.group.as_str()) {
            if !entry.group.is_empty() {
                let heading = create_group_heading(&entry.group);
                list.append(&heading);
                headings.push((heading, entry.group.clone()));
            }
            last_group = Some(entry.group.clone());
        }
        let row = create_script_row(&entry);
        list.append(&row);
        rows.push(ScriptRow { entry, row });
    }

    if let Some(first) = rows.first() {
        list.select_row(Some(&first.row));
    }

    let list_scroll = gtk::ScrolledWindow::new();
    list_scroll.add_css_class("open-with-scroll");
    list_scroll.set_child(Some(&list));
    list_scroll.set_propagate_natural_height(false);
    list_scroll.set_vexpand(true);
    list_scroll.set_hexpand(true);
    list_scroll.set_hscrollbar_policy(gtk::PolicyType::Never);
    layout.body.append(&list_scroll);

    let empty_search = gtk::Label::new(Some(&crate::i18n::tr("No matching scripts were found.")));
    empty_search.add_css_class("open-with-empty");
    empty_search.set_wrap(true);
    empty_search.set_xalign(0.5);
    empty_search.set_halign(gtk::Align::Center);
    empty_search.set_valign(gtk::Align::Center);
    empty_search.set_vexpand(true);
    empty_search.set_visible(false);
    layout.body.append(&empty_search);

    let has_scripts = !rows.is_empty();
    if !has_scripts {
        search_entry.set_visible(false);
        list_scroll.set_visible(false);
        let empty = gtk::Label::new(Some(&crate::i18n::tr("No scripts were found")));
        empty.add_css_class("open-with-empty");
        empty.set_wrap(true);
        empty.set_xalign(0.5);
        empty.set_halign(gtk::Align::Center);
        empty.set_valign(gtk::Align::Center);
        empty.set_vexpand(true);
        layout.body.append(&empty);
        layout.confirm.set_sensitive(false);
    }

    let layer = modal_layer(&layout.content, &window_overlay, blurred_root.clone(), None);
    layer.connect_unrealize(move |_| {
        let on_close = on_close.clone();
        glib::idle_add_local_once(move || on_close());
    });
    window_overlay.add_overlay(&layer);
    let dismissed = Rc::new(std::cell::Cell::new(false));
    let dismiss_layer = layer.downgrade();
    let dismiss_overlay = window_overlay.downgrade();
    let dismiss_root = blurred_root.as_ref().map(|root| root.downgrade());
    let dismiss = Rc::new(move || {
        if dismissed.replace(true) {
            return;
        }
        let (Some(layer), Some(overlay)) = (dismiss_layer.upgrade(), dismiss_overlay.upgrade())
        else {
            return;
        };
        let root = dismiss_root.as_ref().and_then(|root| root.upgrade());
        dismiss_modal_layer(&layer, &overlay, root.as_ref());
    });

    let cancel_dismiss = dismiss.clone();
    layout.cancel.connect_clicked(move |_| cancel_dismiss());
    let close_dismiss = dismiss.clone();
    layout.close.connect_clicked(move |_| close_dismiss());

    let run_dismiss = dismiss.clone();
    let rows_rc = Rc::new(rows);
    let confirm_rows = rows_rc.clone();
    let run_list = list.downgrade();
    let run_parent = parent.as_ref().downgrade();
    let run_selection = selection.clone();
    let run_folder = folder.clone();
    layout.confirm.connect_clicked(move |_| {
        run_dismiss();
        let (Some(list), Some(rows)) = (run_list.upgrade(), Some(confirm_rows.clone())) else {
            return;
        };
        let Some(selected) = list.selected_row() else {
            return;
        };
        let Some(entry) = rows.iter().find(|row| row.row == selected) else {
            return;
        };
        let path = entry.entry.path.clone();
        let folder = run_folder.clone();
        let selection = run_selection.clone();
        let launched = spawn_reaped(move || spawn_script(&path, &folder, &selection));
        let parent = run_parent.clone();
        glib::MainContext::default().spawn_local(async move {
            if let (Err(detail), Some(parent)) = (launched.await, parent.upgrade()) {
                show_error_dialog(&parent, &crate::i18n::tr("Unable to run script"), &detail);
            }
        });
    });

    let activate_confirm = layout.confirm.downgrade();
    list.connect_row_activated(move |_, _| {
        if let Some(confirm) = activate_confirm.upgrade() {
            confirm.emit_clicked();
        }
    });

    let escape_dismiss = dismiss;
    let escape_search = search_entry.downgrade();
    let escape = gtk::EventControllerKey::new();
    escape.set_propagation_phase(gtk::PropagationPhase::Capture);
    keys::on_press(&escape, move |_, press| {
        if press.is(ESCAPE_ANY) {
            if let Some(search) = escape_search.upgrade()
                && !search.text().is_empty()
            {
                search.set_text("");
                search.grab_focus();
                return glib::Propagation::Stop;
            }
            escape_dismiss();
            glib::Propagation::Stop
        } else {
            glib::Propagation::Proceed
        }
    });
    layer.add_controller(escape);

    let search_keys = gtk::EventControllerKey::new();
    search_keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    let list_for_search = list.downgrade();
    let scroll_for_search = list_scroll.downgrade();
    keys::on_press(&search_keys, move |_, press| {
        let down = press.is(DOWN);
        if !down && !press.is(UP) {
            return glib::Propagation::Proceed;
        }
        if let (Some(list), Some(scroll)) = (list_for_search.upgrade(), scroll_for_search.upgrade())
            && let Some(selected) = list.selected_row()
        {
            let mut row = selected.clone();
            let mut sibling = if down {
                selected.next_sibling()
            } else {
                selected.prev_sibling()
            };
            while let Some(candidate) = sibling {
                if candidate.is_visible()
                    && let Some(candidate_row) = candidate.downcast_ref::<gtk::ListBoxRow>()
                    && candidate_row.is_selectable()
                {
                    row = candidate_row.clone();
                    break;
                }
                sibling = if down {
                    candidate.next_sibling()
                } else {
                    candidate.prev_sibling()
                };
            }
            list.select_row(Some(&row));
            if let Some(bounds) = row.compute_bounds(&list) {
                let adjustment = scroll.vadjustment();
                let top = f64::from(bounds.y());
                let bottom = top + f64::from(bounds.height());
                if top < adjustment.value() {
                    adjustment.set_value(top);
                } else if bottom > adjustment.value() + adjustment.page_size() {
                    adjustment.set_value(bottom - adjustment.page_size());
                }
            }
        }
        glib::Propagation::Stop
    });
    search_entry.add_controller(search_keys);

    let list_keys = gtk::EventControllerKey::new();
    let search_for_list = search_entry.downgrade();
    let list_weak = list.downgrade();
    keys::on_press(&list_keys, move |_, press| {
        let (Some(list), Some(search)) = (list_weak.upgrade(), search_for_list.upgrade()) else {
            return glib::Propagation::Proceed;
        };

        if press.is(UP.ignoring(SHORTCUT_MODIFIERS))
            && let Some(selected) = list.selected_row()
        {
            let mut prev = selected.prev_sibling();
            let mut has_prev_selectable = false;
            while let Some(widget) = prev {
                if widget.is_visible() && widget.can_focus() {
                    has_prev_selectable = true;
                    break;
                }
                prev = widget.prev_sibling();
            }
            if !has_prev_selectable && search.is_visible() {
                search.grab_focus();
                return glib::Propagation::Stop;
            }
        }

        if search.is_visible() {
            if press.is(BACKSPACE.ignoring(gtk::gdk::ModifierType::SHIFT_MASK)) {
                search.grab_focus();
                let mut text = search.text().to_string();
                text.pop();
                search.set_text(&text);
                search.set_position(-1);
                return glib::Propagation::Stop;
            }
            if let Some(ch) = press.text() {
                search.grab_focus();
                let mut text = search.text().to_string();
                text.push(ch);
                search.set_text(&text);
                search.set_position(-1);
                return glib::Propagation::Stop;
            }
        }

        glib::Propagation::Proceed
    });
    list.add_controller(list_keys);

    let confirm_for_search = layout.confirm.downgrade();
    search_entry.connect_activate(move |_| {
        if let Some(confirm) = confirm_for_search.upgrade()
            && confirm.is_sensitive()
        {
            confirm.emit_clicked();
        }
    });

    let list_for_filter = list.downgrade();
    let list_scroll_for_filter = list_scroll.downgrade();
    let empty_search_for_filter = empty_search.downgrade();
    let confirm_for_filter = layout.confirm.downgrade();
    search_entry.connect_changed(move |search| {
        let (Some(list), Some(list_scroll), Some(empty_label), Some(confirm)) = (
            list_for_filter.upgrade(),
            list_scroll_for_filter.upgrade(),
            empty_search_for_filter.upgrade(),
            confirm_for_filter.upgrade(),
        ) else {
            return;
        };
        let query = search.text().trim().to_lowercase();
        let mut group_counts: HashMap<String, usize> = HashMap::new();
        let mut first_visible_row: Option<gtk::ListBoxRow> = None;
        let current_selected = list.selected_row();
        let mut selected_is_visible = false;

        for row in rows_rc.iter() {
            let matches = query.is_empty() || row.entry.haystack.contains(&query);
            row.row.set_visible(matches);
            if matches {
                if !row.entry.group.is_empty() {
                    *group_counts.entry(row.entry.group.clone()).or_default() += 1;
                }
                if first_visible_row.is_none() {
                    first_visible_row = Some(row.row.clone());
                }
                if let Some(selected) = &current_selected
                    && &row.row == selected
                {
                    selected_is_visible = true;
                }
            }
        }

        for (heading, group) in &headings {
            heading.set_visible(group_counts.get(group).is_some_and(|count| *count > 0));
        }

        let total_matches: usize = group_counts.values().sum::<usize>()
            + rows_rc
                .iter()
                .filter(|row| row.entry.group.is_empty() && row.row.is_visible())
                .count();
        if total_matches == 0 {
            empty_label.set_visible(true);
            list_scroll.set_visible(false);
            confirm.set_sensitive(false);
            list.select_row(None::<&gtk::ListBoxRow>);
        } else {
            empty_label.set_visible(false);
            list_scroll.set_visible(true);
            confirm.set_sensitive(true);
            if !selected_is_visible && let Some(row) = &first_visible_row {
                list.select_row(Some(row));
            }
        }
    });

    if !has_scripts {
        layout.cancel.grab_focus();
    } else {
        search_entry.grab_focus();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn scratch(name: &str) -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix(name)
            .tempdir()
            .expect("scratch directory")
    }

    fn write_executable(path: &Path, contents: &str) {
        std::fs::write(path, contents).expect("write script");
        set_executable(path);
    }

    fn set_executable(path: &Path) {
        let mut permissions = std::fs::metadata(path)
            .expect("script metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions).expect("chmod");
    }

    #[test]
    fn enumeration_offers_only_runnable_entries() {
        let root = scratch("strata-scripts-enumerate");
        let base = root.path();
        std::fs::create_dir_all(base.join("Image/Batch")).expect("create nested category");
        std::fs::create_dir_all(base.join("Plain text")).expect("create category");
        write_executable(
            &base.join("Image/resize.sh"),
            "#!/bin/sh\n:
",
        );
        // A symlinked script outside the tree is offered through its tree path.
        let real = root
            .path()
            .parent()
            .expect("scratch parent")
            .join("real.sh");
        write_executable(&real, "#!/bin/sh\n");
        std::os::unix::fs::symlink(&real, base.join("Image/Batch/count 冬.sh"))
            .expect("symlink script");
        std::fs::write(base.join("Plain text/not-executable.sh"), "#!/bin/sh\n")
            .expect("write script");
        set_executable(&base.join("Plain text/not-executable.sh"));
        let mut permissions = std::fs::metadata(base.join("Plain text/not-executable.sh"))
            .expect("script metadata")
            .permissions();
        permissions.set_mode(0o644);
        std::fs::set_permissions(base.join("Plain text/not-executable.sh"), permissions)
            .expect("drop execute bit");
        std::fs::write(base.join(".hidden.sh"), "#!/bin/sh\n").expect("write hidden script");
        set_executable(&base.join(".hidden.sh"));
        std::os::unix::fs::symlink(base, base.join("loop")).expect("symlink loop");

        let entries = enumerate_scripts(base);
        let described: Vec<(String, String)> = entries
            .iter()
            .map(|entry| (entry.group.clone(), entry.name.clone()))
            .collect();
        assert_eq!(
            described,
            vec![
                ("Image".to_string(), "resize.sh".to_string()),
                ("Image ▸ Batch".to_string(), "count 冬.sh".to_string()),
            ]
        );
        assert!(entries[1].path.ends_with("Image/Batch/count 冬.sh"));
    }

    #[test]
    fn environment_matches_nautilus() {
        let root = scratch("strata-scripts-env");
        let folder = root.path().to_path_buf();
        let first = folder.join("report final.txt");
        let second = folder.join("冬 workload.pdf");
        let environment =
            nautilus_environment(&[first.clone(), second.clone()], &folder).expect("environment");
        let value = |key: &str| {
            environment
                .iter()
                .find(|(name, _)| *name == key)
                .map(|(_, value)| value.clone())
                .expect("environment variable")
        };
        assert_eq!(
            value("NAUTILUS_SCRIPT_SELECTED_FILE_PATHS").to_string_lossy(),
            format!(
                "{}\n{}\n",
                first.to_string_lossy(),
                second.to_string_lossy()
            )
        );
        assert_eq!(
            value("NAUTILUS_SCRIPT_SELECTED_URIS").to_string_lossy(),
            format!(
                "file://{}/report%20final.txt\nfile://{}/%E5%86%AC%20workload.pdf\n",
                folder.to_string_lossy().replace(' ', "%20"),
                folder.to_string_lossy().replace(' ', "%20")
            )
        );
        assert_eq!(
            value("NAUTILUS_SCRIPT_CURRENT_URI").to_string_lossy(),
            format!("file://{}", folder.to_string_lossy().replace(' ', "%20"))
        );
        assert_eq!(value("NAUTILUS_SCRIPT_WINDOW_GEOMETRY"), "");
        let empty = nautilus_environment(&[], &folder).expect("environment");
        assert!(
            empty
                .iter()
                .find(|(name, _)| *name == "NAUTILUS_SCRIPT_SELECTED_FILE_PATHS")
                .expect("selection variable")
                .1
                .is_empty()
        );
    }

    #[test]
    fn spawn_runs_shebangless_scripts_through_the_shell() {
        let root = scratch("strata-scripts-spawn");
        let folder = root.path().to_path_buf();
        let marker = folder.join("marker");
        let script = folder.join("no-shebang");
        std::fs::write(
            &script,
            format!(
                "# marker written without a shebang\necho ran > {:?}\n",
                marker
            ),
        )
        .expect("write script");
        set_executable(&script);
        let mut child =
            spawn_script(&script, &folder, &[folder.join("input.txt")]).expect("script spawns");
        let status = child.wait().expect("script exits");
        assert!(status.success());
        assert!(marker.exists());
    }

    #[test]
    fn spawn_sets_the_nautilus_environment_and_working_directory() {
        let root = scratch("strata-scripts-spawn-env");
        let folder = root.path().to_path_buf();
        let script = folder.join("dump-env.sh");
        write_executable(
            &script,
            "#!/bin/sh\nprintf '%s' \"$PWD\" > pwd\n\
             printf '%s' \"$NAUTILUS_SCRIPT_SELECTED_FILE_PATHS\" > sel\n\
             printf '%s' \"$NAUTILUS_SCRIPT_CURRENT_URI\" > uri\n",
        );
        let selection = vec![folder.join("a b.txt")];
        let mut child = spawn_script(&script, &folder, &selection).expect("script spawns");
        assert!(child.wait().expect("script exits").success());
        let read = |name: &str| std::fs::read_to_string(folder.join(name)).expect("dump file");
        assert_eq!(read("pwd"), folder.to_string_lossy());
        assert_eq!(read("sel"), format!("{}\n", selection[0].to_string_lossy()));
        assert_eq!(
            read("uri"),
            format!("file://{}", folder.to_string_lossy().replace(' ', "%20"))
        );
    }
}
