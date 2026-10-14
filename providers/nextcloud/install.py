#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Install the Nextcloud provider into Strata's provider directory.

Copies this adapter to ``$XDG_CONFIG_HOME/strata/providers/nextcloud`` and,
when the desktop client is installed, copies its status icons beside the
manifest. Those icons stay on this machine; they are not part of the repo.
Restart Strata after installing.
"""

from __future__ import annotations

import json
import os
import sys
from pathlib import Path

ICON_SOURCES = (
    Path("/usr/share/icons/hicolor/48x48/apps"),
    Path("/usr/share/icons/hicolor/32x32/apps"),
    Path("/usr/share/icons/hicolor/64x64/apps"),
)
ICON_FILES = {
    "logo": "Nextcloud.png",
    "ok": "Nextcloud_ok.png",
    "sync": "Nextcloud_sync.png",
    "warn": "Nextcloud_warn.png",
    "error": "Nextcloud_error.png",
    "shared": "Nextcloud_a_shared.png",
}


def config_dir() -> Path:
    base = os.environ.get("XDG_CONFIG_HOME")
    root = Path(base) if base else Path.home() / ".config"
    return root / "strata" / "providers" / "nextcloud"


def valid_png(data: bytes) -> bool:
    if len(data) < 24 or len(data) > 65536 or data[:8] != b"\x89PNG\r\n\x1a\n":
        return False
    width = int.from_bytes(data[16:20], "big")
    height = int.from_bytes(data[20:24], "big")
    return 0 < width <= 256 and 0 < height <= 256


def find_icon(filename: str) -> bytes | None:
    for directory in ICON_SOURCES:
        path = directory / filename
        if not path.is_file():
            continue
        try:
            data = path.read_bytes()
        except OSError:
            continue
        if valid_png(data):
            return data
    return None


def install(destination: Path | None = None) -> Path:
    here = Path(__file__).resolve().parent
    source = here / "provider.py"
    if not source.is_file():
        raise SystemExit(f"missing {source}")
    destination = (destination or config_dir()).resolve()
    if destination.is_symlink():
        raise SystemExit(f"{destination} is a symlink; refusing to install there")
    destination.mkdir(parents=True, exist_ok=True)
    if destination.is_symlink():
        raise SystemExit(f"{destination} is a symlink; refusing to install there")

    script = destination / "provider.py"
    script.write_bytes(source.read_bytes())
    os.chmod(script, 0o644)

    icons: dict[str, str] = {}
    for icon_id, filename in ICON_FILES.items():
        data = find_icon(filename)
        if data is None:
            continue
        target = destination / f"{icon_id}.png"
        target.write_bytes(data)
        os.chmod(target, 0o644)
        icons[icon_id] = target.name

    python = Path(sys.executable)
    if not python.is_absolute():
        python = Path("/usr/bin/python3")
    manifest: dict = {
        "version": 1,
        "id": "nextcloud",
        "name": "Nextcloud",
        "command": [str(python), str(script)],
    }
    if "logo" in icons:
        manifest["icon"] = "logo"
    if icons:
        manifest["icons"] = icons
    manifest_path = destination / "provider.json"
    manifest_path.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    os.chmod(manifest_path, 0o644)
    os.chmod(destination, 0o755)
    return destination


def main() -> None:
    destination = install()
    print(f"Installed the Nextcloud provider in {destination}")
    print("Restart Strata. Share actions appear on items inside a synced folder.")


if __name__ == "__main__":
    main()
