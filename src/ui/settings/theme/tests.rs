// SPDX-License-Identifier: MIT

use super::*;
use crate::{
    test_support::{gtk_test, gtk_test_with_env},
    ui::preferences::fixtures::seed_omarchy_for_test,
};

fn packaged_card_names(packaged: &gtk::FlowBox) -> Vec<String> {
    let mut names = Vec::new();
    let mut child = packaged.first_child();
    while let Some(current) = child {
        child = current.next_sibling();
        if let Some(card) = current
            .downcast_ref::<gtk::FlowBoxChild>()
            .and_then(gtk::FlowBoxChild::child)
        {
            names.push(card.widget_name().to_string());
        }
    }
    names
}

fn card_has_source_label(packaged: &gtk::FlowBox, name: &str) -> bool {
    let mut child = packaged.first_child();
    while let Some(current) = child {
        child = current.next_sibling();
        let card = current
            .downcast_ref::<gtk::FlowBoxChild>()
            .and_then(gtk::FlowBoxChild::child);
        if card.as_ref().is_some_and(|card| card.widget_name() == name) {
            return descendants(card.as_ref().expect("named card"))
                .into_iter()
                .filter_map(|widget| widget.downcast::<gtk::Label>().ok())
                .any(|label| label.text() == "Omarchy" && label.has_css_class("theme-source"));
        }
    }
    false
}

fn descendants(root: &gtk::Widget) -> Vec<gtk::Widget> {
    let mut widgets = vec![root.clone()];
    let mut child = root.first_child();
    while let Some(current) = child {
        child = current.next_sibling();
        widgets.extend(descendants(&current));
    }
    widgets
}

#[test]
fn installed_omarchy_themes_appear_in_the_library_with_a_source_label() {
    let isolated = tempfile::tempdir().expect("isolated bundled themes");
    let bundled = isolated.path().to_string_lossy().into_owned();
    gtk_test_with_env(
        "ui::settings::theme::tests::installed_omarchy_themes_appear_in_the_library_with_a_source_label",
        [("OMARCHY_PATH", bundled)],
        || {
            let dir = gtk::glib::user_config_dir()
                .join("omarchy")
                .join("themes")
                .join("harbor");
            std::fs::create_dir_all(&dir).expect("catalog fixture dir");
            std::fs::write(
                dir.join("colors.toml"),
                "mode = \"dark\"\nbackground = \"#0d1b2a\"\nforeground = \"#c9deed\"\naccent = \"#4fd6ff\"\n",
            )
            .expect("catalog fixture colors");

            let preferences = PreferenceManager::shared();
            let themes = ThemeManager::shared();
            let page = theme_page(preferences, themes.clone());
            let packaged = &page.flows[0].0;
            assert!(packaged_card_names(packaged).contains(&"Harbor".to_owned()));
            assert!(card_has_source_label(packaged, "Harbor"));

            std::fs::remove_dir_all(gtk::glib::user_config_dir().join("omarchy"))
                .expect("remove installed themes");
            assert!(themes.refresh_omarchy_catalog());
            assert!(!packaged_card_names(packaged).contains(&"Harbor".to_owned()));
            assert!(
                !themes.refresh_omarchy_catalog(),
                "an unchanged catalog refreshes nothing"
            );
        },
    );
}

#[test]
fn variant_choices_sync_across_windows_without_initialization_writes() {
    gtk_test(
        "ui::settings::theme::tests::variant_choices_sync_across_windows_without_initialization_writes",
        || {
            seed_omarchy_for_test();
            let preferences = PreferenceManager::shared();
            let themes = ThemeManager::shared();
            preferences.set_omarchy_variant(OmarchyVariant::Darker);
            let settings = crate::ui::preferences::config_directory().join("settings.toml");
            let before = std::fs::read(&settings).expect("saved settings");
            let windows = [gtk::Window::new(), gtk::Window::new()];
            let controls: Vec<_> = windows
                .iter()
                .map(|window| {
                    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
                    append_omarchy_variant_option(&content, &preferences, &themes);
                    window.set_child(Some(&content));
                    window.present();
                    let row = content.first_child().expect("variant row");
                    let button = row
                        .last_child()
                        .and_downcast::<gtk::MenuButton>()
                        .expect("variant choice");
                    (row, button)
                })
                .collect();
            assert_eq!(
                std::fs::read(&settings).expect("unchanged settings"),
                before
            );
            for (row, button) in &controls {
                assert!(row.is_visible() && row.is_sensitive());
                assert_eq!(button.label().as_deref(), Some("Darker"));
            }
            let menu = controls[0]
                .1
                .popover()
                .expect("choice popover")
                .child()
                .expect("variant menu");
            menu.last_child()
                .and_downcast::<gtk::Button>()
                .expect("high contrast option")
                .emit_clicked();
            assert_eq!(preferences.omarchy_variant(), OmarchyVariant::HighContrast);
            for (_, button) in &controls {
                assert_eq!(button.label().as_deref(), Some("High contrast"));
            }
            themes.set_follow_omarchy(false);
            for (row, _) in &controls {
                assert!(!row.is_sensitive());
            }
            themes.set_follow_omarchy(true);
            menu.first_child()
                .and_downcast::<gtk::Button>()
                .expect("original option")
                .emit_clicked();
            assert_eq!(preferences.omarchy_variant(), OmarchyVariant::Original);
            for (row, button) in &controls {
                assert!(row.is_sensitive());
                assert_eq!(button.label().as_deref(), Some("Original"));
            }
            for window in windows {
                window.close();
            }
        },
    );
}

#[test]
fn variant_is_unavailable_without_omarchy() {
    gtk_test(
        "ui::settings::theme::tests::variant_is_unavailable_without_omarchy",
        || {
            let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
            append_omarchy_variant_option(
                &content,
                &PreferenceManager::shared(),
                &ThemeManager::shared(),
            );
            let row = content.first_child().expect("unavailable variant row");
            assert!(!row.is_visible());
            super::super::search::apply(&content, &super::super::search::State::default());
            assert!(!row.is_visible());
        },
    );
}
