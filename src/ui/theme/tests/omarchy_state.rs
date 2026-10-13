// SPDX-License-Identifier: MIT

//! Follow ON/OFF state model: the stored `theme` id is the independent selection,
//! `mode` is only the follow switch, and an unresolvable Omarchy theme degrades
//! with a single retry instead of switching modes.

use super::super::ThemeManager;
use crate::test_support::{gtk_test, gtk_test_with_env};
use crate::ui::preferences::fixtures::{seed_omarchy_colors_for_test, seed_omarchy_for_test};

const HARBOR_COLORS: &str =
    "mode = \"dark\"\nbackground = \"#0d1b2a\"\nforeground = \"#c9deed\"\naccent = \"#4fd6ff\"\n";
const HARBOR_RETURN_COLORS: &str =
    "mode = \"dark\"\nbackground = \"#0d1b2a\"\nforeground = \"#c9deed\"\naccent = \"#123456\"\n";

fn seed_catalog_theme(slug: &str, colors: &str) {
    let dir = gtk::glib::user_config_dir()
        .join("omarchy")
        .join("themes")
        .join(slug);
    std::fs::create_dir_all(&dir).expect("catalog fixture dir");
    std::fs::write(dir.join("colors.toml"), colors).expect("catalog fixture colors");
}

fn saved_mode_and_theme() -> (String, String) {
    let path = crate::ui::preferences::config_directory().join("settings.toml");
    let contents = std::fs::read_to_string(&path).expect("saved settings");
    let table: toml::Table = toml::from_str(&contents).expect("settings TOML");
    (
        table
            .get("mode")
            .and_then(toml::Value::as_str)
            .expect("saved mode")
            .to_owned(),
        table
            .get("theme")
            .and_then(toml::Value::as_str)
            .expect("saved theme")
            .to_owned(),
    )
}

fn remove_omarchy_state() {
    std::fs::remove_dir_all(super::super::omarchy_state_dir()).expect("remove Omarchy state");
}

#[test]
fn follow_toggle_preserves_the_independent_selection() {
    gtk_test(
        "ui::theme::tests::omarchy_state::follow_toggle_preserves_the_independent_selection",
        || {
            seed_omarchy_for_test();
            let themes = ThemeManager::shared();
            assert!(themes.follows_omarchy());
            themes.set_follow_omarchy(false);
            themes.select_theme("nord");
            themes.set_follow_omarchy(true);
            assert_eq!(themes.selected_id(), "nord");

            // Desktop theme changes do not affect the stored selection.
            seed_omarchy_colors_for_test(
                "background = '#221122'\nforeground = '#eeddee'\naccent = '#aabbcc'\n",
            );
            themes.refresh_omarchy_state();
            assert!(themes.follows_omarchy());
            assert_eq!(themes.selected_id(), "nord");
            assert_eq!(
                saved_mode_and_theme(),
                ("omarchy".to_owned(), "nord".to_owned())
            );

            // Toggling off restores the last independent selection.
            themes.set_follow_omarchy(false);
            assert_eq!(themes.selected_id(), "nord");
            assert!(themes.current_tokens().is_some());
        },
    );
}

#[test]
fn selecting_while_following_changes_nothing() {
    gtk_test(
        "ui::theme::tests::omarchy_state::selecting_while_following_changes_nothing",
        || {
            crate::ui::preferences::fixtures::seed_saved_preferences_for_test();
            seed_omarchy_for_test();
            let themes = ThemeManager::shared();
            assert!(!themes.follows_omarchy());
            themes.set_follow_omarchy(true);
            assert!(themes.follows_omarchy());
            let saved = saved_mode_and_theme();

            themes.select_theme("azure-glow");

            assert!(themes.follows_omarchy());
            assert_eq!(saved_mode_and_theme(), saved);
        },
    );
}

#[test]
fn removed_independent_omarchy_theme_falls_back_but_keeps_its_id() {
    let isolated = tempfile::tempdir().expect("isolated bundled themes");
    let bundled = isolated.path().to_string_lossy().into_owned();
    gtk_test_with_env(
        "ui::theme::tests::omarchy_state::removed_independent_omarchy_theme_falls_back_but_keeps_its_id",
        [("OMARCHY_PATH", bundled)],
        || {
            seed_catalog_theme("harbor", HARBOR_COLORS);
            let themes = ThemeManager::shared();
            assert!(
                themes
                    .themes()
                    .iter()
                    .any(|theme| theme.id == "omarchy:harbor")
            );
            themes.select_theme("omarchy:harbor");
            assert_eq!(themes.appearance_tokens().accent, "#4fd6ff");

            std::fs::remove_dir_all(gtk::glib::user_config_dir().join("omarchy"))
                .expect("remove installed themes");
            assert!(themes.refresh_omarchy_catalog());
            assert_eq!(themes.selected_id(), "omarchy:harbor");
            assert!(
                !themes
                    .themes()
                    .iter()
                    .any(|theme| theme.id == "omarchy:harbor")
            );
            // Last-known-good stays applied with the stored id kept.
            assert_eq!(themes.appearance_tokens().accent, "#4fd6ff");

            seed_catalog_theme("harbor", HARBOR_RETURN_COLORS);
            assert!(themes.refresh_omarchy_catalog());
            assert_eq!(themes.selected_id(), "omarchy:harbor");
            assert_eq!(themes.appearance_tokens().accent, "#123456");
        },
    );
}

#[test]
fn follow_unavailable_degrades_and_recovers_without_a_toggle() {
    gtk_test(
        "ui::theme::tests::omarchy_state::follow_unavailable_degrades_and_recovers_without_a_toggle",
        || {
            seed_omarchy_for_test();
            let themes = ThemeManager::shared();
            themes.set_follow_omarchy(false);
            themes.select_theme("nord");
            themes.set_follow_omarchy(true);

            remove_omarchy_state();
            themes.refresh_omarchy_state();
            assert!(themes.follows_omarchy());
            assert!(themes.omarchy_unavailable());
            assert!(themes.omarchy_retry_armed());
            assert_eq!(themes.selected_id(), "nord");
            assert_eq!(
                themes.appearance_tokens(),
                themes.current_tokens().expect("stored fallback")
            );
            assert_eq!(
                saved_mode_and_theme(),
                ("omarchy".to_owned(), "nord".to_owned())
            );

            // Repeated failures never stack timers.
            themes.refresh_omarchy_state();
            themes.refresh_omarchy_state();
            assert!(themes.omarchy_retry_armed());
            assert_eq!(themes.retry_armed_total(), 1);

            // The next tick resumes following with no toggle needed.
            seed_omarchy_for_test();
            themes.refresh_omarchy_state();
            assert!(themes.follows_omarchy());
            assert!(!themes.omarchy_unavailable());
            assert!(!themes.omarchy_retry_armed());
            assert_eq!(themes.appearance_tokens().accent, "#445566");
            assert_eq!(
                saved_mode_and_theme(),
                ("omarchy".to_owned(), "nord".to_owned())
            );
        },
    );
}

#[test]
fn retry_stops_when_follow_turns_off() {
    gtk_test(
        "ui::theme::tests::omarchy_state::retry_stops_when_follow_turns_off",
        || {
            seed_omarchy_for_test();
            let themes = ThemeManager::shared();
            themes.set_follow_omarchy(false);
            themes.select_theme("nord");
            themes.set_follow_omarchy(true);
            remove_omarchy_state();
            themes.refresh_omarchy_state();
            assert!(themes.omarchy_retry_armed());

            themes.set_follow_omarchy(false);

            assert!(!themes.follows_omarchy());
            assert!(!themes.omarchy_unavailable());
            assert!(!themes.omarchy_retry_armed());
            assert_eq!(themes.selected_id(), "nord");
        },
    );
}

#[test]
fn active_theme_watcher_is_gated_by_follow() {
    gtk_test(
        "ui::theme::tests::omarchy_state::active_theme_watcher_is_gated_by_follow",
        || {
            let themes = ThemeManager::shared();
            assert!(!themes.follows_omarchy());
            assert_eq!(themes.omarchy_monitor_count(), 0);

            // State changes while manual are ignored: no monitors, no switches.
            seed_omarchy_for_test();
            themes.refresh_omarchy_state();
            assert!(!themes.follows_omarchy());
            assert_eq!(themes.omarchy_monitor_count(), 0);
            assert_eq!(themes.selected_id(), "tokyo-night");

            themes.set_follow_omarchy(true);
            assert!(themes.omarchy_monitor_count() > 0);
            themes.set_follow_omarchy(false);
            assert_eq!(themes.omarchy_monitor_count(), 0);
        },
    );
}

#[test]
fn dropping_the_manager_cancels_its_retry() {
    gtk_test(
        "ui::theme::tests::omarchy_state::dropping_the_manager_cancels_its_retry",
        || {
            let settings = crate::ui::preferences::config_directory().join("settings.toml");
            std::fs::create_dir_all(settings.parent().expect("settings parent"))
                .expect("settings dir");
            std::fs::write(&settings, "mode = \"omarchy\"\ntheme = \"nord\"\n")
                .expect("follow-mode settings");
            seed_omarchy_colors_for_test(
                "background = '#112233'\nforeground = '#ddeeff'\naccent = '0x7aa2f7'\n",
            );
            let manager = ThemeManager::load();
            assert!(manager.follows_omarchy());
            assert!(manager.omarchy_unavailable());
            assert!(manager.omarchy_retry_armed());
            drop(manager);
            for _ in 0..8 {
                gtk::glib::MainContext::default().iteration(false);
            }
        },
    );
}

#[test]
fn legacy_configs_load_unchanged() {
    gtk_test(
        "ui::theme::tests::omarchy_state::legacy_configs_load_unchanged",
        || {
            let settings = crate::ui::preferences::config_directory().join("settings.toml");
            std::fs::create_dir_all(settings.parent().expect("settings parent"))
                .expect("settings dir");
            std::fs::write(&settings, "theme = \"nord\"\n").expect("legacy settings");
            let themes = ThemeManager::shared();
            assert!(!themes.follows_omarchy());
            assert_eq!(themes.selected_id(), "nord");
        },
    );
}
