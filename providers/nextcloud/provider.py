#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Strata file provider for the Nextcloud desktop client.

Speaks Strata provider protocol 1 on stdin and stdout. Menu items and sync
badges come from the desktop client's shell-integration socket, the same
protocol the Nautilus extension uses. Choosing an action sends that command
back, so share options and the other entries open the client's own windows.
"""

from __future__ import annotations

import json
import os
import queue
import re
import socket
import sys
import threading
import time
from dataclasses import dataclass
from pathlib import Path

PROTOCOL = 1
RECORD = "\x1e"
COMMAND_RE = re.compile(r"^[A-Z][A-Z0-9_]{0,63}$")
TOKEN_RE = re.compile(r"[A-Z0-9_]{1,32}")
NON_COMMANDS = {
    "DISABLED",
    "CURRENT_PIN",
    "LOCKED_FILE_OWNER",
    "LOCKED_FILE_DATE",
}
ERROR_TOKENS = {
    "NOP": "Nextcloud cannot do that with this item.",
    "NOTCONNECTED": "Nextcloud is not connected to the server.",
    "NOTSYNCED": "This item is not on the server yet. Wait until it finishes syncing, then try again.",
    "CANNOTSHAREROOT": "The top Nextcloud folder cannot be shared. Share a folder inside it.",
    "ERROR": "Nextcloud could not do that.",
}
NOTICES = {
    "COPY_PRIVATE_LINK": "Nextcloud is copying the internal link.",
    "COPY_PUBLIC_LINK": "Nextcloud is copying the public link.",
    "COPY_SECUREFILEDROP_LINK": "Nextcloud is copying the file-drop link.",
    "COPYASPATH": "Nextcloud copied the path.",
    "LOCK_FILE": "Nextcloud is locking the file.",
    "UNLOCK_FILE": "Nextcloud is unlocking the file.",
    "MAKE_AVAILABLE_LOCALLY": "Nextcloud will keep this available offline.",
    "MAKE_ONLINE_ONLY": "Nextcloud will make this online-only.",
    "DELETE_ITEM": "Nextcloud is removing the local changes.",
}
WINDOWS = {
    "SHARE": "Share options opened in Nextcloud.",
    "LEAVESHARE": "Leave share opened in Nextcloud.",
    "ACTIVITY": "Activity opened in Nextcloud.",
    "FILE_ACTIONS": "File actions opened in Nextcloud.",
    "FILES_GOVERNANCE_LABELS": "Labels opened in Nextcloud.",
    "OPEN_PRIVATE_LINK": "Opening in the browser.",
    "EDIT": "Opening in the browser.",
    "EMAIL_PRIVATE_LINK": "Opening the email composer.",
    "RESOLVE_CONFLICT": "Conflict resolver opened in Nextcloud.",
    "MOVE_ITEM": "Move dialog opened in Nextcloud.",
    "ENCRYPT": "Encryption opened in Nextcloud.",
}
ACTION_ICONS = {
    "SHARE": "shared",
    "LEAVESHARE": "shared",
    "LOCK_FILE": "warn",
    "UNLOCK_FILE": "ok",
    "DELETE_ITEM": "error",
    "MAKE_ONLINE_ONLY": "sync",
    "MAKE_AVAILABLE_LOCALLY": "ok",
    "ENCRYPT": "warn",
}


@dataclass
class MenuItem:
    action: str
    enabled: bool
    label: str


def socket_path() -> str:
    override = os.environ.get("STRATA_NEXTCLOUD_SOCKET")
    if override:
        return override
    runtime = os.environ.get("XDG_RUNTIME_DIR")
    if not runtime:
        runtime = os.path.join(
            os.environ.get("TMPDIR", "/tmp"),
            "runtime-" + os.environ.get("USER", "user"),
        )
    return os.path.join(runtime, "Nextcloud", "socket")


def declared_icons() -> set[str]:
    path = Path(__file__).with_name("provider.json")
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return set()
    icons = data.get("icons")
    if not isinstance(icons, dict):
        return set()
    return {key for key in icons if isinstance(key, str)}


def clip(text: str, max_bytes: int, *, allow_newline: bool = False) -> str:
    kept = []
    for char in text:
        if char in "\n\r\t":
            if allow_newline:
                kept.append(char)
            continue
        if char < " " or char == "\x7f":
            continue
        kept.append(char)
    raw = "".join(kept).strip().encode("utf-8")
    if len(raw) <= max_bytes:
        return raw.decode("utf-8")
    raw = raw[:max_bytes]
    while raw:
        try:
            return raw.decode("utf-8").strip()
        except UnicodeDecodeError:
            raw = raw[:-1]
    return ""


def safe_path(path: object) -> str | None:
    if not isinstance(path, str) or not path.startswith("/") or "\n" in path or "\x00" in path:
        return None
    if len(path.encode("utf-8")) > 16384 or RECORD in path:
        return None
    return path


def in_sync(path: str, roots: set[str]) -> bool:
    norm = os.path.normpath(path)
    for root in roots:
        root_norm = os.path.normpath(root)
        if norm == root_norm or norm.startswith(root_norm + os.sep):
            return True
    return False


def parse_menu_item(rest: str) -> MenuItem | None:
    action, found, tail = rest.partition(":")
    if not found:
        return None
    flags, found, label = tail.partition(":")
    if not found:
        return None
    label = clip(label, 128)
    if not label or not COMMAND_RE.fullmatch(action):
        return None
    return MenuItem(action=action, enabled="d" not in flags, label=label)


def badge_for(state: str) -> dict | None:
    shared = state.endswith("+SWM")
    base = state[: -len("+SWM")] if shared else state
    if base in {"", "NOP"}:
        return None
    table = {
        "OK": (0, "ok", "Synced"),
        "SYNC": (1, "sync", "Syncing"),
        "NEW": (1, "sync", "Waiting to sync"),
        "IGNORE": (1, "warn", "Excluded from sync"),
        "ERROR": (2, "error", "Sync error"),
    }
    if base not in table:
        if not TOKEN_RE.fullmatch(base):
            return {"priority": 1, "badge": "warn", "description": "Needs attention"}
        priority, icon, text = 1, "warn", "Nextcloud status " + base
    else:
        priority, icon, text = table[base]
    if shared and base == "OK":
        icon = "shared"
    if shared:
        text += ". Shared with you"
    return {"priority": priority, "badge": icon, "description": text}


def action_id(command: str, used: set[str]) -> str | None:
    slug = re.sub(r"[^a-z0-9-]", "", command.lower().replace("_", "-")).strip("-")
    if not slug:
        return None
    base = slug[:64]
    candidate = base
    number = 2
    while candidate in used:
        suffix = f"-{number}"
        candidate = (base[: 64 - len(suffix)] + suffix).strip("-")
        number += 1
        if number > 70:
            return None
    used.add(candidate)
    return candidate


class Nextcloud:
    def __init__(self) -> None:
        self.state_lock = threading.RLock()
        self.cv = threading.Condition(self.state_lock)
        self.outq: queue.Queue = queue.Queue()
        self.stop = False
        self.sock: socket.socket | None = None
        self.connected = False
        self.registration_seen = False
        self.roots: set[str] = set()
        self.revision = 0
        self.waiting: set[str] = set()
        self.wait_results: dict[str, str] = {}
        self.collecting_menu = False
        self.menu_items: list[MenuItem] = []
        self.menu_done = False
        self.expect_command = ""
        self.command_token = ""
        self.icons = declared_icons()
        self._buffer = b""
        self._writer: threading.Thread | None = None
        self._reader: threading.Thread | None = None

    def start(self) -> None:
        self._writer = threading.Thread(target=self._write_loop, name="strata-nextcloud-out")
        self._reader = threading.Thread(
            target=self._read_loop, name="strata-nextcloud-socket", daemon=True
        )
        self._writer.start()
        self._reader.start()

    def close(self) -> None:
        with self.state_lock:
            self.stop = True
            self.cv.notify_all()
        self._drop_socket()
        self.outq.put(None)
        if self._writer is not None:
            self._writer.join(timeout=2)

    def _write_loop(self) -> None:
        while True:
            item = self.outq.get()
            if item is None:
                return
            try:
                sys.stdout.write(json.dumps(item, ensure_ascii=False) + "\n")
                sys.stdout.flush()
            except (BrokenPipeError, OSError):
                return

    def _emit(self, payload: dict, *, revision: bool) -> None:
        with self.state_lock:
            body = {"version": PROTOCOL, **payload}
            if revision:
                self.revision += 1
                body["revision"] = self.revision
            self.outq.put(body)

    def _invalidate(self, paths: list[str] | None) -> None:
        payload: dict = {"event": "invalidate"}
        if paths is not None:
            payload["paths"] = paths[:200]
        self._emit(payload, revision=True)

    def _drop_socket(self) -> None:
        with self.state_lock:
            sock = self.sock
            self.sock = None
            had_roots = bool(self.roots)
            self.connected = False
            self.roots.clear()
            self.registration_seen = False
            self.menu_done = True
            self.collecting_menu = False
            self.expect_command = ""
            self.cv.notify_all()
        if sock is not None:
            try:
                sock.close()
            except OSError:
                pass
        if had_roots:
            self._invalidate(None)

    def _read_loop(self) -> None:
        backoff = 1.0
        while not self.stop:
            try:
                sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
                sock.settimeout(1.0)
                sock.connect(socket_path())
            except OSError:
                time.sleep(backoff)
                backoff = min(backoff * 1.5, 5.0)
                continue
            backoff = 1.0
            with self.state_lock:
                if self.stop:
                    sock.close()
                    return
                self.sock = sock
                self.connected = True
                self._buffer = b""
                self.cv.notify_all()
            try:
                sock.sendall(b"VERSION:\nGET_STRINGS:\n")
                self._read_socket(sock, time.monotonic())
            except OSError:
                pass
            self._drop_socket()

    def _read_socket(self, sock: socket.socket, connected_at: float) -> None:
        while not self.stop:
            if not self.registration_seen and time.monotonic() - connected_at > 0.5:
                with self.state_lock:
                    self.registration_seen = True
                    self.cv.notify_all()
            try:
                chunk = sock.recv(65536)
            except socket.timeout:
                continue
            if not chunk:
                return
            self._buffer += chunk
            while b"\n" in self._buffer:
                raw, self._buffer = self._buffer.split(b"\n", 1)
                line = raw.decode("utf-8", "replace").strip("\r")
                if line:
                    self._handle_line(line)

    def _handle_line(self, line: str) -> None:
        command, found, rest = line.partition(":")
        if not found:
            return
        if command == "REGISTER_PATH":
            root = os.path.normpath(rest)
            with self.state_lock:
                self.roots.add(root)
                self.registration_seen = True
                self.cv.notify_all()
            self._invalidate([root])
            return
        if command == "UNREGISTER_PATH":
            root = os.path.normpath(rest)
            with self.state_lock:
                self.roots.discard(root)
                self.cv.notify_all()
            self._invalidate([root])
            return
        if command == "UPDATE_VIEW":
            self._invalidate([os.path.normpath(rest)])
            return
        if command == "STATUS":
            state, found, path = rest.partition(":")
            if not found or not path:
                return
            with self.state_lock:
                if path in self.waiting:
                    self.wait_results[path] = state
                    self.cv.notify_all()
                    return
            self._invalidate([path])
            return
        if command == "GET_MENU_ITEMS" and rest in {"BEGIN", "END"}:
            with self.state_lock:
                if rest == "BEGIN" and self.collecting_menu:
                    self.menu_items = []
                elif rest == "END":
                    self.menu_done = True
                    self.cv.notify_all()
            return
        if command == "MENU_ITEM":
            item = parse_menu_item(rest)
            if item is None:
                return
            with self.state_lock:
                if self.collecting_menu:
                    self.menu_items.append(item)
            return
        if command in {"VERSION", "STRING", "GET_STRINGS"}:
            return
        token, _found, _tail = rest.partition(":")
        with self.state_lock:
            if command == self.expect_command:
                self.command_token = token
                self.expect_command = ""
                self.cv.notify_all()

    def _wait_until_registered(self) -> bool:
        deadline = time.monotonic() + 2.0
        with self.state_lock:
            while not self.registration_seen and not self.stop and time.monotonic() < deadline:
                self.cv.wait(timeout=max(0.05, deadline - time.monotonic()))
            return self.connected

    def _send(self, line: str) -> bool:
        data = (line if line.endswith("\n") else line + "\n").encode("utf-8")
        with self.state_lock:
            sock = self.sock
            connected = self.connected
        if not connected or sock is None:
            return False
        try:
            sock.sendall(data)
            return True
        except OSError:
            self._drop_socket()
            return False

    def _roots(self) -> set[str]:
        if not self._wait_until_registered():
            return set()
        with self.state_lock:
            return set(self.roots)

    def _paths(self, paths: list[str]) -> list[str] | None:
        safe = []
        for path in paths:
            cleaned = safe_path(path)
            if cleaned is None:
                return None
            safe.append(cleaned)
        return safe

    def query(self, request_id: int, paths: list[str]) -> None:
        safe = self._paths(paths)
        if safe is None:
            self._emit(
                {"id": request_id, "error": "unsupported-method", "message": "Could not read those paths."},
                revision=False,
            )
            return
        roots = self._roots()
        scoped = [path for path in safe if in_sync(path, roots)]
        results: dict[str, str] = {}
        if scoped:
            with self.state_lock:
                self.waiting = set(scoped)
                self.wait_results = {}
            sent = self._send("".join(f"RETRIEVE_FILE_STATUS:{path}\n" for path in scoped))
            deadline = time.monotonic() + 2.0
            with self.state_lock:
                while sent and self.waiting - set(self.wait_results) and time.monotonic() < deadline:
                    self.cv.wait(timeout=max(0.05, deadline - time.monotonic()))
                results = dict(self.wait_results)
                self.waiting.clear()
        decorations = []
        for path in scoped:
            badge = badge_for(results.get(path, ""))
            if badge is None:
                continue
            if badge["badge"] not in self.icons:
                badge = {"description": badge["description"], "priority": badge["priority"]}
            decorations.append({"path": path, **badge})
        self._emit({"id": request_id, "decorations": decorations}, revision=True)

    def _fetch_menu(self, paths: list[str]) -> list[MenuItem] | None:
        with self.state_lock:
            self.menu_items = []
            self.menu_done = False
            self.collecting_menu = True
        sent = self._send("GET_MENU_ITEMS:" + RECORD.join(paths))
        deadline = time.monotonic() + 2.0
        with self.state_lock:
            while sent and not self.menu_done and time.monotonic() < deadline:
                self.cv.wait(timeout=max(0.05, deadline - time.monotonic()))
            self.collecting_menu = False
            if not sent or not self.menu_done:
                return None
            return list(self.menu_items)

    def _actions(self, items: list[MenuItem]) -> list[dict]:
        actions = []
        used: set[str] = set()
        for item in items:
            if not item.enabled or item.action in NON_COMMANDS:
                continue
            leaf_id = action_id(item.action, used)
            if leaf_id is None:
                continue
            leaf: dict = {"id": leaf_id, "label": item.label, "context": item.action}
            icon = ACTION_ICONS.get(item.action)
            if icon in self.icons:
                leaf["icon"] = icon
            actions.append(leaf)
            if len(actions) >= 64:
                break
        return actions

    def menu(self, request_id: int, paths: list[str]) -> None:
        safe = self._paths(paths)
        actions: list[dict] = []
        if safe is not None:
            roots = self._roots()
            if safe and all(in_sync(path, roots) for path in safe):
                items = self._fetch_menu(safe)
                if items:
                    actions = self._actions(items)
        self._emit({"id": request_id, "actions": actions}, revision=True)

    def activate(self, request_id: int, paths: list[str], context: object) -> None:
        total = len(paths)

        def finish(message: str, status: str, accepted: int, feedback: str | None = None) -> None:
            if total == 0:
                self._emit(
                    {
                        "id": request_id,
                        "error": "unsupported-method",
                        "message": "Nothing was selected.",
                    },
                    revision=False,
                )
                return
            payload: dict = {
                "id": request_id,
                "message": clip(message, 4096, allow_newline=True) or "Done.",
                "outcome": {"status": status, "accepted": accepted, "total": total},
            }
            if feedback and status == "accepted":
                payload["feedback"] = feedback
            self._emit(payload, revision=False)

        command = context if isinstance(context, str) else ""
        safe = self._paths(paths)
        roots = self._roots()
        if safe is None or not safe or any(not in_sync(path, roots) for path in safe):
            finish("Nextcloud controls are only available inside a synced folder.", "rejected", 0)
            return
        if not COMMAND_RE.fullmatch(command) or command in NON_COMMANDS:
            finish("That Nextcloud action is no longer available.", "rejected", 0)
            return
        items = self._fetch_menu(safe)
        if items is None:
            finish("Nextcloud did not answer. Open the menu and try again.", "unknown", 0)
            return
        if command not in {item.action for item in items if item.enabled and item.action not in NON_COMMANDS}:
            finish("Nextcloud cannot do that with this selection anymore.", "rejected", 0)
            return
        with self.state_lock:
            self.command_token = ""
            self.expect_command = command
        if not self._send(f"{command}:{RECORD.join(safe)}"):
            with self.state_lock:
                self.expect_command = ""
            finish("Could not reach the Nextcloud client.", "rejected", 0)
            return
        wait = 0.4 if command in NOTICES else 1.0
        deadline = time.monotonic() + wait
        with self.state_lock:
            while self.expect_command and time.monotonic() < deadline:
                self.cv.wait(timeout=max(0.05, deadline - time.monotonic()))
            token = self.command_token
            self.expect_command = ""
            self.command_token = ""
        if token in ERROR_TOKENS:
            finish(ERROR_TOKENS[token], "rejected", 0)
            return
        if command in NOTICES:
            finish(NOTICES[command], "accepted", total, "notice")
            return
        finish(WINDOWS.get(command, "Opened in Nextcloud."), "accepted", total, "none")


def request_paths(request: dict) -> list[str] | None:
    paths = request.get("paths")
    if not isinstance(paths, list) or len(paths) > 200:
        return None
    if not all(isinstance(path, str) for path in paths):
        return None
    return paths


def handle(client: Nextcloud, request: dict) -> None:
    request_id = request.get("id")
    method = request.get("method")
    if not isinstance(request_id, int) or isinstance(request_id, bool):
        return
    if request.get("version") != PROTOCOL or not isinstance(method, str):
        client._emit(
            {"id": request_id, "error": "unsupported-method", "message": "Unsupported request."},
            revision=False,
        )
        return
    paths = request_paths(request)
    if paths is None:
        client._emit(
            {"id": request_id, "error": "unsupported-method", "message": "Could not read that selection."},
            revision=False,
        )
        return
    if method == "query":
        client.query(request_id, paths)
    elif method == "menu":
        client.menu(request_id, paths)
    elif method == "activate":
        action = request.get("action")
        if not isinstance(action, str):
            client._emit(
                {"id": request_id, "error": "unsupported-method", "message": "Missing action."},
                revision=False,
            )
            return
        client.activate(request_id, paths, request.get("context"))
    else:
        client._emit(
            {"id": request_id, "error": "unsupported-method", "message": "Unsupported request."},
            revision=False,
        )


def main() -> None:
    try:
        sys.stdin.reconfigure(encoding="utf-8")
        sys.stdout.reconfigure(encoding="utf-8", newline="\n")
    except (AttributeError, OSError):
        pass
    client = Nextcloud()
    client.start()
    try:
        for line in sys.stdin:
            text = line.strip()
            if not text:
                continue
            try:
                request = json.loads(text)
            except json.JSONDecodeError:
                continue
            if not isinstance(request, dict):
                continue
            try:
                handle(client, request)
            except Exception:
                request_id = request.get("id")
                if isinstance(request_id, int) and not isinstance(request_id, bool):
                    client._emit(
                        {
                            "id": request_id,
                            "error": "unsupported-method",
                            "message": "Nextcloud provider failed on that request.",
                        },
                        revision=False,
                    )
    finally:
        client.close()


if __name__ == "__main__":
    main()
