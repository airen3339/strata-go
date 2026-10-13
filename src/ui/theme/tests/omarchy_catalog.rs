// SPDX-License-Identifier: MIT

//! Discovery of installed Omarchy themes from fixture directories. These tests never
//! touch `$HOME`: every path is injected into `discover_omarchy_themes_in`.

use std::path::{Path, PathBuf};

use super::super::{
    Theme, ThemeTokens, append_omarchy_themes, background_is_light, discover_omarchy_themes_in,
    is_omarchy_theme_id, is_valid_omarchy_slug, merge_builtin_custom_and_omarchy,
    omarchy_bundled_themes_dir_with, omarchy_declared_light,
};
use crate::test_support::capture_logs;

const MINIMAL_DARK: &str =
    "mode = \"dark\"\nbackground = \"#101418\"\nforeground = \"#d8dee9\"\naccent = \"#88c0d0\"\n";

fn fixture_roots(_name: &str) -> (tempfile::TempDir, PathBuf, PathBuf) {
    let sandbox = tempfile::tempdir().expect("fixture sandbox");
    let user = sandbox.path().join("user");
    let bundled = sandbox.path().join("bundled");
    std::fs::create_dir_all(&user).expect("user fixture dir");
    std::fs::create_dir_all(&bundled).expect("bundled fixture dir");
    (sandbox, user, bundled)
}

fn write_colors(dir: &Path, slug: &str, contents: &str) {
    let theme_dir = dir.join(slug);
    std::fs::create_dir_all(&theme_dir).expect("fixture theme dir");
    std::fs::write(theme_dir.join("colors.toml"), contents).expect("fixture colors");
}

fn discovered_ids(user: &Path, bundled: &Path) -> Vec<String> {
    let mut ids: Vec<String> = discover_omarchy_themes_in(user, bundled)
        .into_iter()
        .map(|theme| theme.id)
        .collect();
    ids.sort();
    ids
}

fn theme_by_id(user: &Path, bundled: &Path, id: &str) -> Theme {
    discover_omarchy_themes_in(user, bundled)
        .into_iter()
        .find(|theme| theme.id == id)
        .unwrap_or_else(|| panic!("{id} should be discovered"))
}

#[test]
fn missing_directories_discover_nothing() {
    let sandbox = tempfile::tempdir().expect("fixture sandbox");
    let themes = discover_omarchy_themes_in(
        &sandbox.path().join("no-user"),
        &sandbox.path().join("no-bundled"),
    );
    assert!(themes.is_empty());
}

#[test]
fn single_user_theme_is_listed() {
    let (_sandbox, user, bundled) = fixture_roots("single");
    write_colors(&user, "harbor", MINIMAL_DARK);

    let theme = theme_by_id(&user, &bundled, "omarchy:harbor");
    assert!(!theme.custom);
    assert_eq!(theme.tokens.name, "Harbor");
    assert_eq!(theme.tokens.background, "#101418");
    assert_eq!(theme.tokens.accent, "#88c0d0");
}

#[test]
fn bundled_themes_are_listed() {
    let (_sandbox, user, bundled) = fixture_roots("bundled");
    write_colors(&bundled, "dune", MINIMAL_DARK);
    write_colors(&bundled, "reef", MINIMAL_DARK);

    assert_eq!(
        discovered_ids(&user, &bundled),
        ["omarchy:dune", "omarchy:reef"]
    );
}

#[test]
fn user_copy_replaces_bundled_copy_wholesale() {
    let (_sandbox, user, bundled) = fixture_roots("precedence");
    write_colors(
        &bundled,
        "coast",
        "mode = \"dark\"\nbackground = \"#101418\"\nforeground = \"#d8dee9\"\naccent = \"#ff0000\"\nmagenta = \"#333333\"\nyellow = \"#111111\"\n",
    );
    write_colors(
        &user,
        "coast",
        "mode = \"dark\"\nbackground = \"#101418\"\nforeground = \"#d8dee9\"\naccent = \"#00ff00\"\nblue = \"#222222\"\n",
    );

    let theme = theme_by_id(&user, &bundled, "omarchy:coast");
    assert_eq!(theme.tokens.accent, "#00ff00");
    // The bundled magenta/yellow must not leak through the wholesale replacement:
    // with no user magenta, keywords derive from the user accent, and the
    // preprocessor is derived rather than the bundled yellow.
    let palette = super::super::resolved_source_palette(&theme.tokens, None);
    assert_eq!(palette.statement, "#00ff00");
    assert_ne!(palette.preprocessor, "#111111");
}

#[test]
fn empty_user_folder_still_lists_bundled_theme() {
    let (_sandbox, user, bundled) = fixture_roots("empty-user");
    std::fs::create_dir_all(user.join("aether")).expect("empty user theme folder");
    write_colors(&bundled, "aether", MINIMAL_DARK);

    let theme = theme_by_id(&user, &bundled, "omarchy:aether");
    assert_eq!(theme.tokens.accent, "#88c0d0");
}

#[test]
fn broken_user_copy_falls_back_to_bundled_with_a_warning() {
    let (_sandbox, user, bundled) = fixture_roots("broken-user");
    write_colors(&user, "coast", "not toml at all {{{");
    write_colors(&bundled, "coast", MINIMAL_DARK);

    let output = capture_logs(|| {
        let theme = theme_by_id(&user, &bundled, "omarchy:coast");
        assert_eq!(theme.tokens.accent, "#88c0d0");
    });
    assert!(
        output.contains("skipping invalid Omarchy user theme"),
        "broken user copy warns:\n{output}"
    );
}

#[test]
fn slug_broken_in_both_locations_is_skipped_with_warnings() {
    let (_sandbox, user, bundled) = fixture_roots("broken-both");
    write_colors(&user, "wreck", "background = [");
    write_colors(
        &bundled,
        "wreck",
        "background = \"#101418\"\naccent = \"#88c0d0\"\n",
    );

    let output = capture_logs(|| {
        assert!(!discovered_ids(&user, &bundled).contains(&"omarchy:wreck".to_owned()));
    });
    assert!(
        output.contains("omarchy:wreck") || output.contains("wreck"),
        "skipped slug is logged:\n{output}"
    );
}

#[test]
fn unreadable_copy_is_skipped_with_a_warning() {
    let (_sandbox, user, bundled) = fixture_roots("unreadable");
    // A directory where colors.toml should be cannot be read as a file.
    let theme_dir = user.join("murk");
    std::fs::create_dir_all(theme_dir.join("colors.toml")).expect("colors.toml directory");
    write_colors(&bundled, "murk", MINIMAL_DARK);

    let output = capture_logs(|| {
        let theme = theme_by_id(&user, &bundled, "omarchy:murk");
        assert_eq!(theme.tokens.accent, "#88c0d0");
    });
    assert!(
        output.contains("skipping unreadable Omarchy user theme"),
        "unreadable copy warns:\n{output}"
    );
}

#[test]
fn broken_symlink_is_skipped_with_a_warning() {
    let (_sandbox, user, bundled) = fixture_roots("dangling");
    let theme_dir = bundled.join("phantom");
    std::fs::create_dir_all(&theme_dir).expect("bundled theme dir");
    std::os::unix::fs::symlink(
        bundled.join("missing-target.toml"),
        theme_dir.join("colors.toml"),
    )
    .expect("dangling colors link");

    let output = capture_logs(|| {
        assert!(!discovered_ids(&user, &bundled).contains(&"omarchy:phantom".to_owned()));
    });
    assert!(
        output.contains("skipping unreadable bundled Omarchy theme"),
        "broken symlink warns:\n{output}"
    );
}

#[test]
fn symlinked_copy_is_used() {
    let (_sandbox, user, bundled) = fixture_roots("symlink");
    let dotfiles = _sandbox.path().join("dotfiles");
    std::fs::create_dir_all(&dotfiles).expect("dotfiles dir");
    std::fs::write(dotfiles.join("colors.toml"), MINIMAL_DARK).expect("dotfile colors");
    let theme_dir = user.join("linked");
    std::fs::create_dir_all(&theme_dir).expect("theme dir");
    std::os::unix::fs::symlink(dotfiles.join("colors.toml"), theme_dir.join("colors.toml"))
        .expect("colors link");

    assert!(discovered_ids(&user, &bundled).contains(&"omarchy:linked".to_owned()));
}

#[test]
fn stray_files_and_invalid_slugs_are_rejected() {
    for slug in ["", ".", "..", "a/b", "a\\b", "a\0b", "/lead", "trail/"] {
        assert!(!is_valid_omarchy_slug(slug), "{slug:?} must be rejected");
    }
    for slug in ["harbor", "tokyo-night", "coast_2", "UPPER", "a.b"] {
        assert!(is_valid_omarchy_slug(slug), "{slug:?} must be accepted");
    }

    let (_sandbox, user, bundled) = fixture_roots("stray");
    std::fs::write(user.join("lonely.toml"), MINIMAL_DARK).expect("stray file");
    std::fs::write(user.join("notes.txt"), "hello").expect("stray text");
    assert!(discover_omarchy_themes_in(&user, &bundled).is_empty());
}

#[test]
fn partial_files_are_skipped() {
    let (_sandbox, user, bundled) = fixture_roots("partial");
    write_colors(
        &user,
        "half",
        "background = \"#101418\"\nforeground = \"#d8dee9\"\n",
    );
    assert!(!discovered_ids(&user, &bundled).contains(&"omarchy:half".to_owned()));
}

#[test]
fn declared_mode_disagreement_keeps_luminance_and_logs() {
    let (_sandbox, user, bundled) = fixture_roots("disagree");
    write_colors(
        &user,
        "contrary",
        "mode = \"light\"\nbackground = \"#101418\"\nforeground = \"#d8dee9\"\naccent = \"#88c0d0\"\n",
    );

    let output = capture_logs(|| {
        let theme = theme_by_id(&user, &bundled, "omarchy:contrary");
        assert!(!background_is_light(&theme.tokens.background));
    });
    assert!(
        output.contains("disagrees with its luminance"),
        "disagreement is noted:\n{output}"
    );
    assert_eq!(omarchy_declared_light("mode = \"light\"\n"), Some(true));
    assert_eq!(omarchy_declared_light("mode = \"dark\"\n"), Some(false));
    assert_eq!(omarchy_declared_light("background = \"#fff\"\n"), None);
}

#[test]
fn identical_palettes_are_both_retained() {
    let (_sandbox, user, bundled) = fixture_roots("identical");
    write_colors(&user, "first", MINIMAL_DARK);
    write_colors(&bundled, "second", MINIMAL_DARK);

    assert_eq!(
        discovered_ids(&user, &bundled),
        ["omarchy:first", "omarchy:second"]
    );
}

#[test]
fn qualified_ids_never_collide_with_native_ids() {
    let native = Theme {
        id: "tokyo-night".to_owned(),
        tokens: fixture_tokens("Native"),
        custom: false,
    };
    let detected = discover_like("tokyo-night");
    let themes = merge_builtin_custom_and_omarchy(vec![native], Vec::new(), vec![detected]);
    let mut ids: Vec<&str> = themes.iter().map(|theme| theme.id.as_str()).collect();
    ids.sort();
    assert_eq!(ids, ["omarchy:tokyo-night", "tokyo-night"]);
}

#[test]
fn custom_files_win_over_detected_copies() {
    let custom = Theme {
        id: "omarchy:coast".to_owned(),
        tokens: fixture_tokens("Handmade"),
        custom: true,
    };
    // A hand-made custom file with the same id keeps the detected copy out.
    let mut themes = vec![custom];
    append_omarchy_themes(&mut themes, vec![discover_like("coast")]);
    assert_eq!(themes.len(), 1);
    assert!(themes[0].custom);
    assert!(is_omarchy_theme_id("omarchy:coast"));
    assert!(!is_omarchy_theme_id("coast"));
}

fn fixture_tokens(name: &str) -> ThemeTokens {
    let (mut tokens, _) =
        super::super::adapt_omarchy_copy("fixture", MINIMAL_DARK).expect("valid fixture tokens");
    tokens.name = name.to_owned();
    tokens
}

fn discover_like(slug: &str) -> Theme {
    let sandbox = tempfile::tempdir().expect("id fixture");
    let user = sandbox.path().join("user");
    let bundled = sandbox.path().join("bundled");
    std::fs::create_dir_all(&user).expect("user dir");
    std::fs::create_dir_all(&bundled).expect("bundled dir");
    write_colors(&bundled, slug, MINIMAL_DARK);
    discover_omarchy_themes_in(&user, &bundled)
        .into_iter()
        .find(|theme| theme.id == format!("omarchy:{slug}"))
        .unwrap_or_else(|| panic!("{slug} should be discovered"))
}

#[test]
fn bundled_dir_prefers_omarchy_path_with_documented_fallback() {
    assert_eq!(
        omarchy_bundled_themes_dir_with(Some("/opt/omarchy".into())),
        PathBuf::from("/opt/omarchy/themes")
    );
    assert_eq!(
        omarchy_bundled_themes_dir_with(None),
        PathBuf::from("/usr/share/omarchy/themes")
    );
}
