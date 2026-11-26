#!/usr/bin/env python3
"""Install this project's debug APK, preserving a verified save across signer changes.

Python standard library + adb only. No device is modified at import time.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import re
import shlex
import shutil
import struct
import subprocess
import sys
from dataclasses import dataclass
from datetime import datetime, timezone
from typing import Callable
import uuid
import xml.etree.ElementTree as ET
import zipfile

PACKAGE = "tw.lijiabao.defendtaipei"
SAVE_NAME = "defendtaipei-native-v1.json"
REMOTE_SAVE = "files/" + SAVE_NAME
MAX_SAVE_BYTES = 4 * 1024 * 1024
MAX_MANIFEST_BYTES = 1024 * 1024


class UpdateError(Exception):
    """A safe, user-facing failure; never includes private save contents."""


@dataclass(frozen=True)
class Backup:
    path: Path
    digest: str
    size: int


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def _string_pool(data: bytes, start: int, size: int, header: int) -> list[str]:
    if header < 28 or start + header > len(data):
        raise ValueError("invalid string pool")
    count, _, flags, strings_at, _ = struct.unpack_from("<IIIII", data, start + 8)
    end = start + size
    if count > 100_000 or header + count * 4 > size or not header <= strings_at < size:
        raise ValueError("invalid string offsets")

    def length8(at: int) -> tuple[int, int]:
        if at >= end:
            raise ValueError("truncated string")
        first = data[at]
        if first & 0x80:
            if at + 1 >= end:
                raise ValueError("truncated string length")
            return ((first & 0x7F) << 8) | data[at + 1], at + 2
        return first, at + 1

    def length16(at: int) -> tuple[int, int]:
        if at + 2 > end:
            raise ValueError("truncated string")
        first = struct.unpack_from("<H", data, at)[0]
        if first & 0x8000:
            if at + 4 > end:
                raise ValueError("truncated string length")
            return ((first & 0x7FFF) << 16) | struct.unpack_from("<H", data, at + 2)[0], at + 4
        return first, at + 2

    strings = []
    for i in range(count):
        offset = struct.unpack_from("<I", data, start + header + i * 4)[0]
        at = start + strings_at + offset
        if flags & 0x100:
            _, at = length8(at)
            length, at = length8(at)
            if at + length >= end or data[at + length] != 0:
                raise ValueError("truncated UTF-8 string")
            strings.append(data[at:at + length].decode("utf-8"))
        else:
            length, at = length16(at)
            length *= 2
            if at + length + 2 > end or data[at + length:at + length + 2] != b"\0\0":
                raise ValueError("truncated UTF-16 string")
            strings.append(data[at:at + length].decode("utf-16-le"))
    return strings


def manifest_details(data: bytes) -> tuple[str, bool]:
    """Read package/debuggable without requiring an Android SDK/aapt install.

    A private-data restore requires an explicitly debuggable application. A
    missing or unresolved flag is never treated as permission to reinstall.
    """
    android_namespace = "http://schemas.android.com/apk/res/android"
    try:
        if data.lstrip().startswith(b"<"):
            root = ET.fromstring(data)
            if root.tag != "manifest":
                raise ValueError("not a manifest")
            applications = root.findall("application")
            if len(applications) > 1:
                raise ValueError("multiple application elements")
            raw_debug = applications[0].get("{" + android_namespace + "}debuggable") if applications else None
            if raw_debug not in (None, "true", "false"):
                raise ValueError("unresolved debuggable flag")
            return root.attrib["package"], raw_debug == "true"
        kind, header, size = struct.unpack_from("<HHI", data)
        if kind != 3 or header != 8 or size != len(data):
            raise ValueError("invalid binary XML")
        strings: list[str] = []
        package = None
        debuggable = False
        application_seen = False
        root_seen = False
        depth = 0
        at = header
        while at + 8 <= size:
            chunk, chunk_header, chunk_size = struct.unpack_from("<HHI", data, at)
            if chunk_header < 8 or chunk_size < chunk_header or at + chunk_size > size:
                raise ValueError("invalid XML chunk")
            if chunk == 1:
                strings = _string_pool(data, at, chunk_size, chunk_header)
            elif chunk == 0x102:
                if chunk_header < 16 or chunk_size < chunk_header + 20:
                    raise ValueError("invalid XML element")
                ext = at + chunk_header
                name = struct.unpack_from("<I", data, ext + 4)[0]
                element = strings[name]
                if depth == 0:
                    if root_seen or element != "manifest":
                        raise ValueError("invalid manifest root")
                    root_seen = True
                is_manifest = depth == 0 and element == "manifest"
                is_application = depth == 1 and element == "application"
                if is_application:
                    if application_seen:
                        raise ValueError("multiple application elements")
                    application_seen = True
                if is_manifest or is_application:
                    attr_start, attr_size, count = struct.unpack_from("<HHH", data, ext + 8)
                    if attr_start < 20 or attr_size < 20 or ext + attr_start + count * attr_size > at + chunk_size:
                        raise ValueError("invalid attributes")
                    debug_seen = False
                    for i in range(count):
                        attr = ext + attr_start + i * attr_size
                        namespace, name, raw = struct.unpack_from("<III", data, attr)
                        attribute = strings[name]
                        value_type = data[attr + 15]
                        value = struct.unpack_from("<I", data, attr + 16)[0]
                        if is_manifest and namespace == 0xFFFFFFFF and attribute == "package":
                            if package is not None:
                                raise ValueError("duplicate package")
                            if raw != 0xFFFFFFFF:
                                package = strings[raw]
                            elif value_type == 3:
                                package = strings[value]
                            else:
                                raise ValueError("package is not a string")
                        elif is_application and attribute == "debuggable" and namespace != 0xFFFFFFFF and strings[namespace] == android_namespace:
                            if debug_seen or value_type != 0x12:
                                raise ValueError("ambiguous or unresolved debuggable flag")
                            debug_seen = True
                            debuggable = value != 0
                depth += 1
            elif chunk == 0x103:
                if depth <= 0:
                    raise ValueError("unbalanced XML elements")
                depth -= 1
            at += chunk_size
        if at != size or depth != 0:
            raise ValueError("truncated XML elements")
        if package is not None:
            return package, debuggable
    except (ValueError, IndexError, KeyError, struct.error, UnicodeError, ET.ParseError) as error:
        raise UpdateError("Cannot verify the APK's Android manifest.") from error
    raise UpdateError("The APK manifest does not declare a package.")


def manifest_package(data: bytes) -> str:
    """Compatibility wrapper for package-only inspection and parser tests."""
    return manifest_details(data)[0]


def verify_apk(path: Path) -> Path:
    path = path.expanduser().resolve()
    if not path.is_file():
        raise UpdateError("APK file not found: " + str(path))
    try:
        with zipfile.ZipFile(path) as archive:
            member = archive.getinfo("AndroidManifest.xml")
            if member.file_size > MAX_MANIFEST_BYTES:
                raise UpdateError("APK manifest is unexpectedly large.")
            package, debuggable = manifest_details(archive.read(member))
    except (OSError, KeyError, zipfile.BadZipFile, RuntimeError) as error:
        raise UpdateError("Cannot read the APK. Download the complete APK and retry.") from error
    if package != PACKAGE:
        raise UpdateError("This APK is not for " + PACKAGE + "; no device changes were made.")
    if not debuggable:
        raise UpdateError("This APK is not explicitly debuggable. Private save restoration requires this project's debug APK; no device changes were made.")
    return path


def _unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("duplicate JSON field")
        result[key] = value
    return result


def validate_save(data: bytes) -> None:
    """Reject missing, corrupt, foreign, non-finite, or incomplete native saves.

    This is a structural/version sanity check. The game's full world/item
    validation still runs when the player chooses Continue.
    """
    def require(condition: bool, field: str) -> None:
        if not condition:
            raise UpdateError("Save validation failed at " + field + ".")

    def number(value, low: float, high: float) -> bool:
        try:
            return type(value) in (int, float) and math.isfinite(value) and low <= value <= high
        except OverflowError:
            return False

    def uint(value, high: int = 1_000_000_000) -> bool:
        return type(value) is int and 0 <= value <= high

    def vector(value, length: int) -> bool:
        return isinstance(value, list) and len(value) == length and all(number(x, -1_000_000, 1_000_000) for x in value)

    def stacks(value, field: str) -> None:
        require(isinstance(value, list) and len(value) <= 1024, field)
        seen = set()
        for pair in value:
            require(isinstance(pair, list) and len(pair) == 2, field)
            key, count = pair
            require(isinstance(key, str) and 0 < len(key) <= 128 and key not in seen and uint(count, 1_000_000), field)
            seen.add(key)

    require(0 < len(data) <= MAX_SAVE_BYTES, "file size")
    try:
        save = json.loads(data.decode("utf-8"), object_pairs_hook=_unique_object,
                          parse_constant=lambda _: (_ for _ in ()).throw(ValueError("non-finite JSON")))
    except (UnicodeError, ValueError, RecursionError) as error:
        raise UpdateError("The save is not valid JSON.") from error
    pending = [save]
    while pending:
        node = pending.pop()
        if isinstance(node, dict):
            pending.extend(node.values())
        elif isinstance(node, list):
            pending.extend(node)
        elif type(node) is float:
            require(math.isfinite(node), "finite JSON values")
    require(isinstance(save, dict), "root")
    require(type(save.get("version")) is int and save["version"] == 1, "version (expected 1)")
    for key in ("campaign", "player", "inventory", "shelter", "environment", "needs", "world"):
        require(isinstance(save.get(key), dict), key)
    require(save.get("role_id") in ("office", "student", "courier", "nurse", "tech", "jp"), "role_id")
    require(type(save.get("female")) is bool, "female")
    campaign, player = save["campaign"], save["player"]
    require(number(campaign.get("game_min"), 0, 1_000_000_000), "campaign.game_min")
    require(type(campaign.get("day_hours")) is int and campaign["day_hours"] in (1, 2, 4), "campaign.day_hours")
    require(type(campaign.get("speed")) is int and campaign["speed"] in (1, 3, 20), "campaign.speed")
    require(number(campaign.get("clock_remainder", 0), -64, 64), "campaign.clock_remainder")
    require(number(player.get("hp"), 0.000001, 100), "player.hp")
    require(type(player.get("bleed")) is bool and number(player.get("stun"), 0, 3600), "player status")
    require(uint(player.get("kills")) and number(player.get("survived"), 0, 1_000_000_000), "player progress")
    require(vector(player.get("position"), 3) and vector(player.get("rotation"), 4), "player transform")
    require(0.99 <= sum(x * x for x in player["rotation"]) <= 1.01, "player.rotation")
    stacks(save["inventory"].get("items"), "inventory.items")
    require(isinstance(save["inventory"].get("equipped"), str), "inventory.equipped")
    shelter = save["shelter"]
    require(uint(shelter.get("day")) and shelter["day"] >= 1, "shelter.day")
    require(isinstance(shelter.get("group"), list) and isinstance(shelter.get("upgrades"), list), "shelter members/upgrades")
    stacks(shelter.get("stash"), "shelter.stash")
    require(shelter.get("home") is None or vector(shelter["home"], 2), "shelter.home")
    for key in ("raining", "power", "water"):
        require(type(save["environment"].get(key)) is bool, "environment." + key)
    for key in ("hunger", "thirst", "fatigue"):
        require(number(save["needs"].get(key), 0, 100), "needs." + key)
    world = save["world"]
    for key in ("searched", "asked", "night_seen", "dead", "drops", "loot"):
        require(isinstance(world.get(key), list) and len(world[key]) <= 100_000, "world." + key)
    for entry in world["loot"]:
        require(isinstance(entry, list) and len(entry) == 2 and isinstance(entry[0], str), "world.loot")
        stacks(entry[1], "world.loot stock")
    weather = world.get("weather")
    require(isinstance(weather, dict), "world.weather")
    for key in ("next", "until"):
        require(number(weather.get(key), 0, 1_000_000_000), "world.weather." + key)
    for key in ("amount", "intensity", "wet"):
        require(number(weather.get(key), 0, 1), "world.weather." + key)
    require(uint(weather.get("seed"), 4_294_967_295), "world.weather.seed")
    require(uint(world.get("fixtures_version", 0), 1), "world.fixtures_version")
    for key in ("closed_doors", "barricaded_doors"):
        ids = world.get(key, [])
        require(isinstance(ids, list) and len(ids) <= 100_000 and all(uint(x) for x in ids) and len(set(ids)) == len(ids), "world." + key)
    require(set(world.get("barricaded_doors", [])) <= set(world.get("closed_doors", [])), "world barricade state")
    if "map" in save:
        require(isinstance(save["map"], dict) and isinstance(save["map"].get("revealed"), list)
                and isinstance(save["map"].get("notes"), list), "map")


def _fsync_directory(path: Path) -> None:
    if os.name == "nt":
        # Windows does not expose directory fsync via os.open; file fsync still
        # calls FlushFileBuffers. Never skip file flush or read-back verification.
        return
    descriptor = os.open(path, os.O_RDONLY | getattr(os, "O_DIRECTORY", 0))
    try:
        os.fsync(descriptor)
    finally:
        os.close(descriptor)


def _write_exclusive(path: Path, data: bytes) -> None:
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(descriptor, "wb") as output:
        output.write(data)
        output.flush()
        os.fsync(output.fileno())


def persist_backup(data: bytes, directory: Path) -> Backup:
    validate_save(data)
    directory = directory.expanduser().resolve()
    created = []
    ancestor = directory
    while not ancestor.exists():
        created.append(ancestor)
        ancestor = ancestor.parent
    directory.mkdir(parents=True, exist_ok=True)
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    path = directory / ("defendtaipei-save-" + stamp + "-" + uuid.uuid4().hex[:12] + ".json")
    digest = sha256(data)
    _write_exclusive(path, data)
    _write_exclusive(Path(str(path) + ".sha256"), (digest + "\n").encode("ascii"))
    _fsync_directory(directory)
    # A new directory's own fsync does not persist its entry in its parent.
    # Flush the whole newly-created chain before uninstall can be reached.
    for child in created:
        _fsync_directory(child.parent)
    restored = path.read_bytes()
    validate_save(restored)
    if sha256(restored) != digest:
        raise UpdateError("Local backup read-back failed; original app was not removed.")
    if Path(str(path) + ".sha256").read_text(encoding="ascii").strip() != digest:
        raise UpdateError("Local checksum read-back failed; original app was not removed.")
    return Backup(path, digest, len(data))


def load_backup(path: Path) -> tuple[Backup, bytes]:
    path = path.expanduser().resolve()
    try:
        if path.stat().st_size > MAX_SAVE_BYTES:
            raise UpdateError("Backup exceeds the native save size limit.")
        data = path.read_bytes()
        digest = Path(str(path) + ".sha256").read_text(encoding="ascii").strip()
    except (OSError, UnicodeError) as error:
        raise UpdateError("Cannot read the backup and its matching .json.sha256 file: " + str(path)) from error
    validate_save(data)
    if not re.fullmatch(r"[0-9a-f]{64}", digest) or sha256(data) != digest:
        raise UpdateError("Backup checksum does not match; no restore was attempted.")
    return Backup(path, digest, len(data)), data


class Adb:
    def __init__(self, executable: str = "adb", serial: str | None = None, runner: Callable = subprocess.run):
        self.executable, self.serial, self.runner = executable, serial, runner

    def run(self, args: list[str], data: bytes | None = None):
        command = [self.executable] + (["-s", self.serial] if self.serial else []) + args
        try:
            return self.runner(command, input=data, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=180, check=False)
        except (OSError, subprocess.TimeoutExpired) as error:
            raise UpdateError("ADB could not finish the requested operation. Check the USB connection.") from error

    def require(self, args: list[str], operation: str, data: bytes | None = None) -> bytes:
        result = self.run(args, data)
        if result.returncode != 0:
            raise UpdateError(operation + " failed. No private save contents were printed.")
        return result.stdout


def select_device(adb: Adb, requested: str | None) -> Adb:
    result = adb.run(["devices"])
    if result.returncode:
        raise UpdateError("ADB could not list devices. Start adb and authorize USB debugging.")
    states = {}
    for line in result.stdout.decode("utf-8", "replace").splitlines():
        parts = line.split()
        if len(parts) >= 2 and parts[0] != "List" and not parts[0].startswith("*"):
            states[parts[0]] = parts[1]
    if requested:
        if states.get(requested) != "device":
            raise UpdateError("The requested --serial is not an online, authorized ADB device.")
        selected = requested
    else:
        online = [serial for serial, state in states.items() if state == "device"]
        if len(online) != 1:
            raise UpdateError("Connect exactly one authorized device, or select it with --serial. No device was changed.")
        selected = online[0]
    return Adb(adb.executable, selected, adb.runner)


def force_stop(adb: Adb) -> None:
    adb.require(["shell", "am", "force-stop", PACKAGE], "Stopping the game")


def backup_device(adb: Adb, directory: Path, say: Callable[[str], None] = print) -> Backup:
    force_stop(adb)
    data = adb.require(["exec-out", "run-as", PACKAGE, "cat", REMOTE_SAVE], "Reading the debug app's saved game")
    try:
        backup = persist_backup(data, directory)
    except OSError as error:
        raise UpdateError("Could not durably write the local backup; original app was not removed.") from error
    say("Verified backup: " + str(backup.path))
    say("SHA-256: " + backup.digest)
    return backup


def signature_mismatch(result) -> bool:
    if result.returncode == 0:
        return False
    text = (result.stdout + result.stderr).decode("utf-8", "replace")
    failures = re.findall(r"(?:^|: )Failure \[(INSTALL_FAILED_[A-Z_]+):\s*([^\]\r\n]+)\]\s*$", text, re.MULTILINE)
    if len(failures) != 1 or failures[0][0] != "INSTALL_FAILED_UPDATE_INCOMPATIBLE":
        return False
    return re.search(r"(?<![\w.])" + re.escape(PACKAGE) + r"(?![\w.])", failures[0][1]) is not None


def install(adb: Adb, apk: Path):
    return adb.run(["install", "-r", "-t", str(apk)])


def install_succeeded(result) -> bool:
    return result.returncode == 0 and any(line.strip() == b"Success" for line in result.stdout.splitlines())


def install_error_code(result) -> str:
    text = (result.stdout + result.stderr).decode("utf-8", "replace")
    codes = re.findall(r"\bINSTALL_FAILED_[A-Z_]+\b", text)
    return codes[-1] if codes else "ADB exit status " + str(result.returncode)


def restore_bytes(adb: Adb, backup: Backup, data: bytes) -> None:
    validate_save(data)
    if sha256(data) != backup.digest:
        raise UpdateError("Backup bytes changed before restore.")
    force_stop(adb)
    temporary = "files/.defendtaipei-restore-" + uuid.uuid4().hex + ".tmp"
    create = "umask 077 && mkdir -p files && cat > " + temporary
    try:
        # -T disables a PTY, preserving byte-for-byte JSON. The nested shell
        # command is quoted for adb's remote shell; no user strings enter it.
        adb.require(["shell", "-T", "run-as", PACKAGE, "sh", "-c", shlex.quote(create)], "Writing the private restore file", data)
        staged = adb.require(["exec-out", "run-as", PACKAGE, "cat", temporary], "Checking the private restore file")
        if sha256(staged) != backup.digest:
            raise UpdateError("Private staging checksum differs; the saved-game slot was not replaced.")
        replace = "sync && mv -f " + temporary + " " + REMOTE_SAVE + " && sync"
        adb.require(["shell", "-T", "run-as", PACKAGE, "sh", "-c", shlex.quote(replace)], "Atomically restoring the saved game")
        restored = adb.require(["exec-out", "run-as", PACKAGE, "cat", REMOTE_SAVE], "Checking the restored saved game")
        if sha256(restored) != backup.digest:
            raise UpdateError("Restored save checksum differs. The local verified backup is still available.")
    except UpdateError:
        try:
            adb.run(["shell", "run-as", PACKAGE, "rm", "-f", temporary])
        except UpdateError:
            pass
        raise


def recovery_command(adb: Adb, apk: Path | None, backup: Backup) -> str:
    command = [sys.executable, str(Path(__file__).resolve()), "restore", "--backup", str(backup.path), "--serial", str(adb.serial)]
    if apk:
        command += ["--apk", str(apk)]
    if os.name == "nt":
        return "& " + " ".join("'" + arg.replace("'", "''") + "'" for arg in command)
    return shlex.join(command)


def update(adb: Adb, apk: Path, directory: Path, migrate: bool, say: Callable[[str], None] = print) -> Backup | None:
    result = install(adb, apk)
    if install_succeeded(result):
        say("APK installed successfully without clearing app data. Open the game and choose Continue if available.")
        return None
    if not signature_mismatch(result):
        raise UpdateError("APK installation failed (" + install_error_code(result)
                          + ") for a reason other than this app's signing mismatch. Nothing was uninstalled.")
    if not migrate:
        raise UpdateError("The installed test build uses a different signing key. Save in the old game, then rerun with --migrate and --backup-dir. Nothing was uninstalled.")
    try:
        backup = backup_device(adb, directory, say)
    except UpdateError as error:
        raise UpdateError(str(error) + " Nothing was uninstalled.") from error
    # Reload both files immediately before the destructive operation; a failed
    # write, bad checksum, invalid save, or missing private slot cannot reach it.
    checked, data = load_backup(backup.path)
    try:
        result = adb.run(["uninstall", PACKAGE])
        if not install_succeeded(result):
            raise UpdateError("Android did not confirm uninstall. No replacement or restore was attempted.")
        replacement = install(adb, apk)
        if not install_succeeded(replacement):
            raise UpdateError("The old build was removed, but the new APK did not install ("
                              + install_error_code(replacement) + "). No rollback was performed.")
        restore_bytes(adb, checked, data)
    except UpdateError as error:
        raise UpdateError(str(error) + "\nVerified backup retained: " + str(backup.path)
                          + "\nAfter resolving the APK/ADB issue, recover with:\n" + recovery_command(adb, apk, backup)) from error
    say("Migration completed. The restored private save matches the verified backup byte for byte.")
    say("Open the game and choose Continue. Keep the .json and .json.sha256 backup files.")
    return backup


def main(argv: list[str] | None = None, runner: Callable = subprocess.run, say: Callable[[str], None] = print) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", nargs="?", choices=("update", "backup", "restore"), default="update")
    parser.add_argument("--apk", type=Path, help="Debug APK to install; required for update, optional for recovery restore")
    parser.add_argument("--serial", help="Exact authorized device serial from adb devices")
    parser.add_argument("--backup-dir", type=Path, default=Path.home() / "DefendTaipei-backups", help="Local verified-backup folder (default: ~/DefendTaipei-backups)")
    parser.add_argument("--migrate", action="store_true", help="Allow signature-changing reinstall only after verified save backup")
    parser.add_argument("--backup", type=Path, help="Verified .json backup to restore (keep its .json.sha256 sidecar)")
    args = parser.parse_args(argv)
    if args.command == "update" and args.apk is None:
        parser.error("update requires --apk")
    if args.command == "restore" and args.backup is None:
        parser.error("restore requires --backup")
    if args.migrate and args.command != "update":
        parser.error("--migrate is available only for update")
    try:
        apk = verify_apk(args.apk) if args.apk else None
        source = load_backup(args.backup) if args.command == "restore" else None
        executable = shutil.which("adb")
        if executable is None:
            raise UpdateError("adb is not on PATH. Install Android Platform Tools and enable USB debugging.")
        adb = select_device(Adb(executable, runner=runner), args.serial)
        say("Selected ADB device: " + str(adb.serial))
        if args.command == "backup":
            backup_device(adb, args.backup_dir, say)
        elif args.command == "update":
            update(adb, apk, args.backup_dir, args.migrate, say)
        else:
            backup, data = source
            try:
                if apk:
                    replacement = install(adb, apk)
                    if not install_succeeded(replacement):
                        raise UpdateError("Recovery APK installation failed (" + install_error_code(replacement)
                                          + "). Restore never uninstalls an app.")
                restore_bytes(adb, backup, data)
            except UpdateError as error:
                raise UpdateError(str(error) + "\nVerified backup retained: " + str(backup.path)
                                  + "\nRetry after resolving the issue:\n" + recovery_command(adb, apk, backup)) from error
            say("Restore verified byte for byte. Open the game and choose Continue.")
        return 0
    except (UpdateError, OSError) as error:
        say("Stopped: " + str(error))
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
