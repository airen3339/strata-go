#!/usr/bin/env python3
"""Tests for the optional Nextcloud file provider.

Run with:

    python3 scripts/test_nextcloud_provider.py

or, with the other script tests:

    python3 -m unittest discover -s scripts -p 'test_*.py'
"""

from __future__ import annotations

import importlib.util
import json
import os
import queue
import socket
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PROVIDER_PATH = ROOT / "providers" / "nextcloud" / "provider.py"
INSTALL_PATH = ROOT / "providers" / "nextcloud" / "install.py"


def load(name: str, path: Path):
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"could not load {path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


provider = load("strata_nextcloud_provider", PROVIDER_PATH)
install = load("strata_nextcloud_install", INSTALL_PATH)


class ParseTests(unittest.TestCase):
    def test_menu_item_enabled_and_disabled(self) -> None:
        enabled = provider.parse_menu_item("SHARE::Share options")
        self.assertIsNotNone(enabled)
        assert enabled is not None
        self.assertEqual(enabled.action, "SHARE")
        self.assertTrue(enabled.enabled)
        self.assertEqual(enabled.label, "Share options")

        disabled = provider.parse_menu_item("SHARE:d:Share options")
        self.assertIsNotNone(disabled)
        assert disabled is not None
        self.assertFalse(disabled.enabled)

    def test_menu_item_rejects_a_broken_row(self) -> None:
        self.assertIsNone(provider.parse_menu_item("SHARE:"))
        self.assertIsNone(provider.parse_menu_item("not a command::Label"))

    def test_sync_root_boundary(self) -> None:
        roots = {"/sync"}
        self.assertTrue(provider.in_sync("/sync", roots))
        self.assertTrue(provider.in_sync("/sync/folder", roots))
        self.assertFalse(provider.in_sync("/sync-extra", roots))
        self.assertFalse(provider.in_sync("/elsewhere", roots))

    def test_action_ids_are_slugs(self) -> None:
        used: set[str] = set()
        self.assertEqual(provider.action_id("COPY_PRIVATE_LINK", used), "copy-private-link")
        self.assertEqual(provider.action_id("SHARE", used), "share")
        self.assertEqual(provider.action_id("SHARE", used), "share-2")
        for slug in used:
            self.assertRegex(slug, r"^[a-z0-9-]{1,64}$")

    def test_badges_describe_shared_files(self) -> None:
        badge = provider.badge_for("OK+SWM")
        self.assertIsNotNone(badge)
        assert badge is not None
        self.assertEqual(badge["badge"], "shared")
        self.assertIn("Shared with you", badge["description"])
        self.assertIsNone(provider.badge_for("NOP"))
        self.assertEqual(provider.badge_for("ERROR")["priority"], 2)

    def test_label_is_clipped_to_the_protocol_byte_limit(self) -> None:
        self.assertLessEqual(len(provider.clip("é" * 100, 128).encode()), 128)


def reply_for(line: str) -> list[str]:
    if line.startswith("GET_MENU_ITEMS:"):
        argument = line.split(":", 1)[1]
        if argument == "/sync":
            return [
                "GET_MENU_ITEMS:BEGIN",
                "MENU_ITEM:SHARE:d:Share options",
                "GET_MENU_ITEMS:END",
            ]
        if argument.startswith("/sync/"):
            return [
                "GET_MENU_ITEMS:BEGIN",
                "MENU_ITEM:OPEN_PRIVATE_LINK::Open in browser",
                "MENU_ITEM:SHARE::Share options",
                "MENU_ITEM:COPY_PRIVATE_LINK::Copy internal link",
                "MENU_ITEM:FILE_ACTIONS::File actions",
                "MENU_ITEM:SHARE:d:Disabled share",
                "GET_MENU_ITEMS:END",
            ]
        return ["GET_MENU_ITEMS:BEGIN", "GET_MENU_ITEMS:END"]
    if line.startswith("RETRIEVE_FILE_STATUS:"):
        path = line.split(":", 1)[1]
        state = "OK+SWM" if path == "/sync/folder" else "OK"
        return [f"STATUS:{state}:{path}"]
    if line.startswith("SHARE:"):
        path = line.split(":", 1)[1]
        if path == "/sync/missing":
            return [f"SHARE:NOTSYNCED:{path}"]
        return [f"SHARE:OK:{path}"]
    return []


class FakeClient(threading.Thread):
    def __init__(self, path: str) -> None:
        super().__init__(daemon=True)
        self.path = path
        self.ready = threading.Event()
        self.commands: list[str] = []
        self._socket = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)

    def run(self) -> None:
        if os.path.exists(self.path):
            os.unlink(self.path)
        self._socket.bind(self.path)
        self._socket.listen(1)
        self._socket.settimeout(5)
        self.ready.set()
        try:
            connection, _address = self._socket.accept()
        except socket.timeout:
            return
        connection.settimeout(1)
        connection.sendall(b"REGISTER_PATH:/sync\n")
        buffer = b""
        while True:
            try:
                chunk = connection.recv(65536)
            except socket.timeout:
                continue
            if not chunk:
                break
            buffer += chunk
            while b"\n" in buffer:
                raw, buffer = buffer.split(b"\n", 1)
                line = raw.decode().strip()
                if not line:
                    continue
                self.commands.append(line)
                for reply in reply_for(line):
                    connection.sendall((reply + "\n").encode())
        connection.close()

    def close(self) -> None:
        self._socket.close()


class ProviderProcess:
    def __init__(self, socket_path: str) -> None:
        env = os.environ.copy()
        env["STRATA_NEXTCLOUD_SOCKET"] = socket_path
        self.proc = subprocess.Popen(
            [sys.executable, str(PROVIDER_PATH)],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            env=env,
            text=True,
        )
        self.lines: queue.Queue[str] = queue.Queue()
        threading.Thread(target=self._read, daemon=True).start()

    def _read(self) -> None:
        assert self.proc.stdout is not None
        for line in self.proc.stdout:
            self.lines.put(line)

    def request(self, payload: dict) -> dict:
        assert self.proc.stdin is not None
        self.proc.stdin.write(json.dumps(payload) + "\n")
        self.proc.stdin.flush()
        deadline = time.monotonic() + 5
        seen: list[str] = []
        while time.monotonic() < deadline:
            try:
                line = self.lines.get(timeout=0.2)
            except queue.Empty:
                continue
            seen.append(line)
            data = json.loads(line)
            if data.get("id") == payload["id"]:
                return data
        raise AssertionError(f"no reply for {payload['id']}: {seen}")

    def close(self) -> str:
        if self.proc.stdin is not None and not self.proc.stdin.closed:
            self.proc.stdin.close()
        try:
            self.proc.wait(timeout=3)
        except subprocess.TimeoutExpired:
            self.proc.kill()
            self.proc.wait(timeout=3)
        stderr = ""
        if self.proc.stderr is not None:
            stderr = self.proc.stderr.read()
            self.proc.stderr.close()
        if self.proc.stdout is not None:
            self.proc.stdout.close()
        return stderr


class ProtocolTests(unittest.TestCase):
    def setUp(self) -> None:
        self.directory = tempfile.TemporaryDirectory()
        self.socket_path = os.path.join(self.directory.name, "nextcloud.sock")
        self.server = FakeClient(self.socket_path)
        self.server.start()
        self.assertTrue(self.server.ready.wait(2))
        self.provider = ProviderProcess(self.socket_path)

    def tearDown(self) -> None:
        stderr = self.provider.close()
        self.server.close()
        self.server.join(timeout=2)
        self.directory.cleanup()
        if self.provider.proc.returncode not in (0, -15):
            self.assertEqual(stderr, "", stderr)

    def test_query_decorates_only_synced_paths(self) -> None:
        reply = self.provider.request(
            {
                "version": 1,
                "id": 1,
                "method": "query",
                "paths": ["/sync/folder", "/elsewhere"],
                "background": False,
            }
        )
        self.assertEqual([item["path"] for item in reply["decorations"]], ["/sync/folder"])
        self.assertIn("Shared with you", reply["decorations"][0]["description"])
        self.assertNotIn("badge", reply["decorations"][0])
        self.assertIn("revision", reply)

    def test_menu_uses_the_client_labels_and_drops_disabled_rows(self) -> None:
        reply = self.provider.request(
            {
                "version": 1,
                "id": 2,
                "method": "menu",
                "paths": ["/sync/folder"],
                "background": False,
            }
        )
        labels = [item["label"] for item in reply["actions"]]
        self.assertEqual(
            labels,
            ["Open in browser", "Share options", "Copy internal link", "File actions"],
        )
        share = next(item for item in reply["actions"] if item["label"] == "Share options")
        self.assertEqual(share["id"], "share")
        self.assertEqual(share["context"], "SHARE")
        self.assertNotIn("icon", share)

    def test_sync_root_and_outside_paths_offer_nothing(self) -> None:
        root = self.provider.request(
            {"version": 1, "id": 3, "method": "menu", "paths": ["/sync"], "background": True}
        )
        outside = self.provider.request(
            {
                "version": 1,
                "id": 4,
                "method": "menu",
                "paths": ["/elsewhere"],
                "background": False,
            }
        )
        self.assertEqual(root["actions"], [])
        self.assertEqual(outside["actions"], [])
        self.assertFalse(any(line.startswith("GET_MENU_ITEMS:/elsewhere") for line in self.server.commands))

    def test_share_opens_in_the_client_without_a_strata_dialog(self) -> None:
        reply = self.provider.request(
            {
                "version": 1,
                "id": 5,
                "method": "activate",
                "paths": ["/sync/folder"],
                "background": False,
                "action": "share",
                "context": "SHARE",
            }
        )
        self.assertEqual(reply["feedback"], "none")
        self.assertEqual(reply["outcome"]["status"], "accepted")
        self.assertTrue(any(line == "SHARE:/sync/folder" for line in self.server.commands))

    def test_unsynced_share_is_rejected(self) -> None:
        reply = self.provider.request(
            {
                "version": 1,
                "id": 6,
                "method": "activate",
                "paths": ["/sync/missing"],
                "background": False,
                "action": "share",
                "context": "SHARE",
            }
        )
        self.assertEqual(reply["outcome"]["status"], "rejected")
        self.assertIn("not on the server", reply["message"])
        self.assertNotIn("feedback", reply)

    def test_copy_link_uses_a_notice(self) -> None:
        reply = self.provider.request(
            {
                "version": 1,
                "id": 7,
                "method": "activate",
                "paths": ["/sync/folder"],
                "background": False,
                "action": "copy-private-link",
                "context": "COPY_PRIVATE_LINK",
            }
        )
        self.assertEqual(reply["feedback"], "notice")
        self.assertEqual(reply["outcome"]["status"], "accepted")


class InstallTests(unittest.TestCase):
    def test_install_writes_an_absolute_unlinked_command(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            destination = install.install(Path(tmp) / "nextcloud")
            manifest = json.loads((destination / "provider.json").read_text(encoding="utf-8"))
            command = manifest["command"]
            script = Path(command[1])
            self.assertEqual(manifest["id"], "nextcloud")
            self.assertTrue(Path(command[0]).is_absolute())
            self.assertEqual(script, destination / "provider.py")
            self.assertTrue(script.is_file())
            self.assertFalse(script.is_symlink())
            self.assertEqual(script.stat().st_mode & 0o022, 0)
            self.assertEqual((destination / "provider.json").stat().st_mode & 0o022, 0)
            for filename in manifest.get("icons", {}).values():
                data = (destination / filename).read_bytes()
                self.assertTrue(install.valid_png(data))
                self.assertFalse((destination / filename).is_symlink())


if __name__ == "__main__":
    unittest.main()
