# SPDX-License-Identifier: MIT
"""Keymap overrides from Settings → Keyboard and settings.toml reach dispatch."""

import tomllib

import pytest

from harness.keyboard_layout import RUSSIAN, RUSSIAN_ONLY

# Folders sort first, so todo.txt is the last entry.
TARGET = "readme.md"
LAST = "todo.txt"


def _saved_keymap(strata):
    return tomllib.loads(strata.environment.settings_path.read_text()).get("keymap", {})


def _open_keyboard_settings(strata):
    assert strata.window.find(role="button", name="Settings").activate()
    page = strata.wait(
        lambda: strata.window.find(role="button", name="Keyboard settings"), "Settings to open"
    )
    assert page.activate()
    strata.wait(
        lambda: strata.window.find(role="toggle button", name="10xer mode"),
        "the Keyboard page's table switch",
    )
    # The page lists every action; a search brings the Rename row on screen.
    search = strata.window.find(role="text", name="Search settings")
    strata.pointer.click(search)
    strata.keyboard.type_text("Rename")
    strata.wait(lambda: search.text == "Rename", "the settings search")
    strata.wait(
        lambda: (add := strata.window.find(role="button", name="Add a key for “Rename”"))
        and strata.on_screen(add),
        "the Rename row on the Keyboard page",
    )


def _close_settings(strata):
    assert strata.window.find(role="button", name="Close settings").activate()
    strata.wait(
        lambda: strata.window.find(role="button", name="Close settings") is None,
        "Settings to close",
    )


def _show_table(strata, name):
    switch = strata.window.find(role="toggle button", name=name)
    strata.pointer.click(switch)
    strata.wait(lambda: switch.has_state("pressed"), f"the {name} table")


def _start_capture(strata, action):
    strata.pointer.click(strata.reveal(role="button", name=f"Add a key for “{action}”"))
    return strata.wait(
        lambda: strata.window.find(
            role="button", name=f"Press the new key for “{action}”", states={"focused"}
        ),
        f"the capture for {action}",
    )


def _chip_remove(strata, keys, action):
    return strata.window.find(role="button", name=f"Remove {keys} from “{action}”", rendered=False)


def _editor(strata):
    return strata.window.find(role="text", states={"editable", "focused"})


def _assert_renames(strata, key):
    strata.select_entry_with_keyboard(TARGET)
    strata.keyboard.press(key)
    field = strata.editable_field()
    strata.wait(lambda: field.text == TARGET, "the rename to show the focused name")
    strata.keyboard.press("Escape")
    strata.wait(lambda: _editor(strata) is None, "Escape to cancel the rename")
    strata.wait_for_focused_entry(TARGET)


def _assert_does_not_rename(strata, key):
    strata.select_entry_with_keyboard(TARGET)
    strata.keyboard.press(key)
    # End reaches the list after the key before it, and only while no rename
    # field holds the focus.
    strata.keyboard.press("End")
    strata.wait_for_focused_entry(LAST)
    assert _editor(strata) is None, f"{key} started a rename"


@pytest.mark.preferences(type_to_search=False)
def test_remap_through_settings(strata):
    _open_keyboard_settings(strata)
    _start_capture(strata, "Rename")
    strata.keyboard.press("ctrl+e")
    strata.wait(lambda: _chip_remove(strata, "Ctrl+E", "Rename"), "the captured Ctrl+E chip")
    strata.pointer.click(strata.reveal(role="button", name="Remove F2 from “Rename”"))
    strata.wait(lambda: _chip_remove(strata, "F2", "Rename") is None, "F2 to leave Rename")
    strata.wait(
        lambda: _saved_keymap(strata) == {"default": {"rename": ["<Control>r", "<Control>e"]}},
        "the override to be saved",
    )
    _close_settings(strata)

    _assert_renames(strata, "ctrl+e")
    _assert_does_not_rename(strata, "F2")


@pytest.mark.preferences(
    type_to_search=False, keymap={"default": {"rename": ["<Control>e"]}}
)
def test_saved_override_applies_at_launch(strata):
    _assert_renames(strata, "ctrl+e")
    _assert_does_not_rename(strata, "F2")
    _assert_does_not_rename(strata, "ctrl+r")

    strata.keyboard.press("F1")
    strata.wait(
        lambda: strata.window.find(role="label", name="Keyboard shortcuts"),
        "the shortcut reference to open",
    )
    title = strata.wait(
        lambda: strata.window.find(role="label", name="Rename", rendered=False),
        "the reference's Rename row",
    )
    keys = [label.name for label in title.parent.find_all(role="label", rendered=False)]
    assert keys == ["Rename", "Ctrl+E"], "the row shows the saved key instead of F2 / Ctrl+R"


@pytest.mark.preferences(type_to_search=False)
def test_tables_are_independent(strata):
    _open_keyboard_settings(strata)
    _show_table(strata, "10xer mode")
    strata.pointer.click(strata.reveal(role="button", name="Remove F2 from “Rename”"))
    strata.wait(lambda: _chip_remove(strata, "F2", "Rename") is None, "F2 to leave 10xer Rename")
    _show_table(strata, "Default")
    strata.wait(lambda: _chip_remove(strata, "F2", "Rename"), "the default table to keep F2")
    _start_capture(strata, "Rename")
    strata.keyboard.press("ctrl+e")
    strata.wait(lambda: _chip_remove(strata, "Ctrl+E", "Rename"), "Ctrl+E on the default Rename")
    _show_table(strata, "10xer mode")
    strata.wait(lambda: _chip_remove(strata, "r", "Rename"), "the 10xer table")
    assert _chip_remove(strata, "Ctrl+E", "Rename") is None
    assert _chip_remove(strata, "F2", "Rename") is None
    strata.wait(
        lambda: _saved_keymap(strata)
        == {
            "default": {"rename": ["F2", "<Control>r", "<Control>e"]},
            "tenxer": {"rename": ["r"]},
        },
        "each table's override to be saved apart",
    )
    _close_settings(strata)

    _assert_renames(strata, "F2")
    _assert_renames(strata, "ctrl+e")
    strata.keyboard.press("ctrl+shift+m")
    strata.wait(
        lambda: strata.environment.read_preferences().get("tenxer_mode") == "true",
        "10xer mode to turn on",
    )
    _assert_does_not_rename(strata, "F2")
    _assert_does_not_rename(strata, "ctrl+e")
    _assert_renames(strata, "r")


@pytest.mark.preferences(
    browser_mode="list",
    type_to_search=False,
    keymap={
        "default": {
            "activate": ["Return", "o"],
            "escape": ["Escape", "<Control>bracketleft"],
        }
    },
)
def test_structural_alias(strata):
    root = strata.fixture.root.name
    strata.select_entry_with_keyboard("documents")
    strata.keyboard.press("o")
    strata.wait_for_directory("documents")
    strata.keyboard.press("alt+Up")
    strata.wait_for_directory(root)

    strata.select_entry_with_keyboard(TARGET)
    strata.keyboard.press("F2")
    field = strata.editable_field()
    strata.keyboard.type_text("renamed")
    strata.wait(lambda: field.text == "renamed.md", "typing to replace the stem")
    strata.keyboard.press("ctrl+[")
    strata.wait(lambda: _editor(strata) is None, "Ctrl+[ to cancel the rename")
    assert strata.fixture.path(TARGET).exists()
    assert not strata.fixture.path("renamed.md").exists()

    strata.keyboard.press("ctrl+f")
    field = strata.editable_field()
    strata.keyboard.type_text("o")
    strata.wait(lambda: field.text == "o", "o to type into the filter field")
    assert strata.current_directory() == root


@pytest.mark.parametrize("layout", [RUSSIAN, RUSSIAN_ONLY], ids=lambda layout: layout.name)
@pytest.mark.preferences(type_to_search=False)
def test_capture_records_latin_key_on_russian_layout(strata, keyboard_layout, layout):
    _open_keyboard_settings(strata)
    _start_capture(strata, "Rename")
    # The key types у here; the row records the Latin key the labels show.
    with keyboard_layout(layout):
        strata.keyboard.press("ctrl+e")
        strata.wait(lambda: _chip_remove(strata, "Ctrl+E", "Rename"), "the captured Ctrl+E chip")
    strata.wait(
        lambda: _saved_keymap(strata)
        == {"default": {"rename": ["F2", "<Control>r", "<Control>e"]}},
        "the Latin key to be saved",
    )
    _close_settings(strata)

    with keyboard_layout(layout):
        _assert_renames(strata, "ctrl+e")
    _assert_renames(strata, "ctrl+e")


def test_conflict_blocks_commit(strata):
    before = strata.environment.settings_path.read_text()
    _open_keyboard_settings(strata)
    capture = _start_capture(strata, "Rename")
    strata.keyboard.press("ctrl+c")
    strata.wait(
        lambda: strata.window.find(role="label", name_matches=r"Ctrl\+C.*“Copy”|“Copy”.*Ctrl\+C"),
        "the duplicate key error",
    )
    assert "Copy" in capture.description
    assert capture.has_state("focused"), "the capture keeps waiting for another key"
    assert _chip_remove(strata, "Ctrl+C", "Rename") is None
    assert strata.environment.settings_path.read_text() == before

    strata.keyboard.press("Escape")
    strata.wait(
        lambda: strata.window.find(
            role="button", name="Add a key for “Rename”", states={"focused"}
        ),
        "Escape to end the capture",
    )
    assert _chip_remove(strata, "F2", "Rename") is not None
    assert _chip_remove(strata, "Ctrl+R", "Rename") is not None
    assert strata.environment.settings_path.read_text() == before
