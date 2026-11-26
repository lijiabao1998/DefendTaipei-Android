"""ADB migration safety tests. Every ADB invocation is mocked; no device access."""

import importlib.util
import json
from pathlib import Path
import shlex
import struct
import subprocess
import sys
import tempfile
import unittest
from unittest import mock
import zipfile


SCRIPT = Path(__file__).resolve().parents[1] / "update-android-test-build.py"
SPEC = importlib.util.spec_from_file_location("android_test_update", SCRIPT)
updater = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = updater
SPEC.loader.exec_module(updater)

PRIVATE_TEXT = "DO_NOT_PRINT_私人存檔"


def saved_game():
    return {
        "version": 1,
        "campaign": {"game_min": 440.0, "clock_remainder": 0.125, "day_hours": 1,
                     "speed": 1, "notice": PRIVATE_TEXT},
        "player": {"hp": 91.0, "bleed": False, "stun": 0.0, "kills": 0,
                   "survived": 10.0, "position": [147.3, 0.0, -135.5], "rotation": [0.0, 0.0, 0.0, 1.0]},
        "role_id": "tech", "female": False,
        "inventory": {"items": [["water", 3], ["plank", 2]], "equipped": ""},
        "shelter": {"day": 1, "karma": 0, "scarcity": 1.0, "upgrades": [], "group": [],
                    "stash": [["rice", 2]], "event": "", "log": [], "plant_day": 0,
                    "pray_day": 0, "raided": 0, "stole": 0, "gave": 0, "home": [147.3, -135.5]},
        "environment": {"raining": False, "power": True, "water": True},
        "needs": {"hunger": 80.0, "thirst": 70.0, "fatigue": 10.0},
        "world": {"searched": [], "asked": [], "night_seen": [], "dead": [], "drops": [], "loot": [],
                  "closed_doors": [97], "barricaded_doors": [97], "fixtures_version": 1,
                  "weather": {"next": 900.0, "until": 0.0, "amount": 0.0, "intensity": 1.0,
                              "wet": 0.0, "seed": 20261024}},
        "map": {"revealed": [0, 1], "notes": []},
    }


def save_bytes():
    # The helper must preserve whitespace, UTF-8, and ordering, not re-encode JSON.
    return ("  " + json.dumps(saved_game(), ensure_ascii=False, indent=3) + "\n\n").encode("utf-8")


def success():
    return 0, b"Success\n", b""


def failure(code="INSTALL_FAILED_UPDATE_INCOMPATIBLE", package=updater.PACKAGE):
    return 1, b"", ("adb: failed to install app.apk: Failure [" + code + ": Existing package "
                     + package + " signatures do not match newer version; ignoring!]\n").encode()


class FakeAdb:
    def __init__(self, payload=None, installs=None):
        self.payload = save_bytes() if payload is None else payload
        self.installs = list(installs if installs is not None else [failure(), success()])
        self.devices = b"List of devices attached\nphone-123\tdevice\n"
        self.calls = []
        self.stopped = False
        self.temporary = None
        self.renamed = False
        self.on_uninstall = None
        self.uninstall_result = success()
        self.bad_staging = False
        self.bad_readback = False

    def __call__(self, command, *, input, stdout, stderr, timeout, check):
        self.calls.append((command[:], input))
        args = command[1:]
        if args[:1] == ["-s"]:
            self.assert_serial(args[1])
            args = args[2:]
        if args == ["devices"]:
            result = (0, self.devices, b"")
        elif args[:1] == ["install"]:
            if "--no-incremental" not in args:
                raise AssertionError("all update/recovery installs must disable incremental fallback")
            result = self.installs.pop(0)
        elif args == ["uninstall", updater.PACKAGE]:
            if self.on_uninstall:
                self.on_uninstall()
            result = self.uninstall_result
            if result[0] == 0:
                self.payload = None
        elif args == ["shell", "am", "force-stop", updater.PACKAGE]:
            self.stopped = True
            result = success()
        elif args[:4] == ["exec-out", "run-as", updater.PACKAGE, "cat"]:
            if not self.stopped:
                raise AssertionError("save read before force-stop")
            if args[4] == updater.REMOTE_SAVE:
                content = b"corrupt" if self.bad_readback and self.renamed else self.payload
            else:
                content = b"corrupt staging" if self.bad_staging else self.temporary
            result = (0, content, b"") if content is not None else (1, b"", b"No such file")
        elif args[:6] == ["shell", "-T", "run-as", updater.PACKAGE, "sh", "-c"]:
            remote = shlex.split(args[6])
            if len(remote) != 1:
                raise AssertionError("remote command must remain one quoted shell argument")
            if remote[0].startswith("umask 077 && mkdir -p files && cat > files/.defendtaipei-restore-"):
                if not self.stopped or not isinstance(input, bytes):
                    raise AssertionError("restore must use stopped app and binary stdin")
                self.temporary = input
            elif remote[0].startswith("sync && mv -f files/.defendtaipei-restore-"):
                if not remote[0].endswith(" " + updater.REMOTE_SAVE + " && sync"):
                    raise AssertionError("wrong atomic rename destination")
                self.payload = self.temporary
                self.temporary = None
                self.renamed = True
            else:
                raise AssertionError("unexpected shell command: " + remote[0])
            result = success()
        elif args[:5] == ["shell", "run-as", updater.PACKAGE, "rm", "-f"]:
            self.temporary = None
            result = success()
        else:
            raise AssertionError("Unexpected ADB command: " + repr(command))
        return subprocess.CompletedProcess(command, *result)

    def assert_serial(self, serial):
        if serial not in ("phone-123", "phone-456"):
            raise AssertionError("wrong device selected")

    def uninstall_calls(self):
        return [c for c, _ in self.calls if "uninstall" in c]


class AndroidUpdateTests(unittest.TestCase):
    def setUp(self):
        self.folder = tempfile.TemporaryDirectory()
        self.addCleanup(self.folder.cleanup)
        self.root = Path(self.folder.name)
        self.apk = self.root / "new test build.apk"
        self.write_apk(self.apk, updater.PACKAGE)
        self.backups = self.root / "durable backups"
        self.output = []

    @staticmethod
    def write_apk(path, package):
        with zipfile.ZipFile(path, "w") as archive:
            archive.writestr("AndroidManifest.xml", '<manifest xmlns:android="http://schemas.android.com/apk/res/android" package="'
                             + package + '"><application android:debuggable="true"/></manifest>')

    def adb(self, fake):
        return updater.select_device(updater.Adb("adb", runner=fake), "phone-123")

    def invoke(self, fake, *args):
        with mock.patch.object(updater.shutil, "which", return_value="adb"):
            return updater.main(list(args), runner=fake, say=self.output.append)

    def assert_no_private_output(self):
        self.assertNotIn(PRIVATE_TEXT, "\n".join(self.output))

    def test_fresh_install_and_same_certificate_update_never_clear_or_read_data(self):
        for old in (None, save_bytes()):
            with self.subTest(existing=old is not None):
                fake = FakeAdb(installs=[success()])
                fake.payload = old
                self.assertEqual(self.invoke(fake, "--apk", str(self.apk), "--migrate"), 0)
                self.assertEqual(len(fake.calls), 2)  # device selection and install -r only
                self.assertIn("-r", fake.calls[1][0])
                self.assertEqual(fake.payload, old)
                self.assertFalse(fake.uninstall_calls())

    def test_signer_mismatch_requires_explicit_migrate(self):
        fake = FakeAdb()
        self.assertEqual(self.invoke(fake, "--apk", str(self.apk), "--backup-dir", str(self.backups)), 1)
        self.assertFalse(fake.uninstall_calls())
        self.assertFalse(self.backups.exists())
        self.assertIn("--migrate", "\n".join(self.output))

    def test_repeated_identical_fallback_failure_keeps_explicit_migration_gate(self):
        # ADB can emit an incremental failure on stdout and the same streamed
        # failure on stderr. They describe one conflict, not two authorizations.
        _, _, diagnostic = failure()
        bare = diagnostic[diagnostic.index(b"Failure ["):]
        repeated = (1, b"Performing Incremental Install\n" + bare + b"Performing Streamed Install\n", diagnostic)
        fake = FakeAdb(installs=[repeated])
        self.assertEqual(self.invoke(fake, "--apk", str(self.apk)), 1)
        self.assertFalse(fake.uninstall_calls())
        self.assertIn("--migrate", "\n".join(self.output))

        fake = FakeAdb(installs=[repeated, success()])
        original = fake.payload
        self.assertEqual(self.invoke(fake, "--apk", str(self.apk), "--migrate", "--backup-dir", str(self.backups)), 0)
        self.assertEqual(len(fake.uninstall_calls()), 1)
        self.assertEqual(fake.payload, original)
        self.assertEqual(list(self.backups.glob("*.json"))[0].read_bytes(), original)
        self.assert_no_private_output()

    def test_mixed_or_malformed_fallback_failures_never_uninstall(self):
        _, _, first = failure()
        for second in (failure("INSTALL_FAILED_VERSION_DOWNGRADE")[2],
                       failure(package="example.other")[2],
                       failure(package=updater.PACKAGE + ".other")[2],
                       b"Failure [malformed diagnostic]\n"):
            with self.subTest(second=second):
                fake = FakeAdb(installs=[(1, first, second)])
                self.assertEqual(self.invoke(fake, "--apk", str(self.apk), "--migrate", "--backup-dir", str(self.backups)), 1)
                self.assertFalse(fake.uninstall_calls())
                self.assertFalse(self.backups.exists())
                self.assertEqual(fake.payload, save_bytes())
                self.assertEqual(len(fake.calls), 2)

    def test_unclassified_install_diagnostics_are_retained_bounded_and_escaped(self):
        result = (1, b"Performing Streamed Install\n", b"Unexpected diagnostic\x1b[2J\n" + b"x" * 20000)
        fake = FakeAdb(installs=[result])
        self.assertEqual(self.invoke(fake, "--apk", str(self.apk), "--migrate"), 1)
        printed = "\n".join(self.output)
        self.assertIn("Performing Streamed Install", printed)
        self.assertIn("Unexpected diagnostic", printed)
        self.assertIn("[truncated]", printed)
        self.assertNotIn("\x1b", printed)
        self.assertLess(len(printed), 18000)
        self.assertFalse(fake.uninstall_calls())
        self.assert_no_private_output()

    def test_sdk_library_incompatibility_is_not_a_signer_migration(self):
        # AOSP InstallPackageHelper uses the same exact error code for this
        # non-signature incompatibility, including the correct package name.
        diagnostic = ("Failure [INSTALL_FAILED_UPDATE_INCOMPATIBLE: Failure updating " + updater.PACKAGE
                      + " as it updates an sdk library <test.library> without changing the versionMajor, but the"
                      + " targetSdkVersion or minSdkVersion has changed: Old targetSdkVersion: 33 new targetSdkVersion: 34"
                      + " Old minSdkVersion: 23 new minSdkVersion: 23 versionMajor: 1]\n").encode()
        for stdout in (b"", diagnostic):
            with self.subTest(repeated=bool(stdout)):
                fake = FakeAdb(installs=[(1, stdout, diagnostic)])
                self.assertEqual(self.invoke(fake, "--apk", str(self.apk), "--migrate", "--backup-dir", str(self.backups)), 1)
                self.assertFalse(fake.uninstall_calls())
                self.assertFalse(self.backups.exists())
                self.assertEqual(fake.payload, save_bytes())
                self.assertEqual(len(fake.calls), 2)

    def test_wrong_install_error_or_package_cannot_trigger_uninstall(self):
        for result in (failure("INSTALL_FAILED_VERSION_DOWNGRADE"), failure(package="example.other"),
                       failure(package=updater.PACKAGE + ".other"), failure("INSTALL_FAILED_UPDATE_INCOMPATIBLE_EXTRA"),
                       (1, b"", b"Unable to install; diagnostic text INSTALL_FAILED_UPDATE_INCOMPATIBLE tw.lijiabao.defendtaipei")):
            with self.subTest(result=result):
                fake = FakeAdb(installs=[result])
                self.assertEqual(self.invoke(fake, "--apk", str(self.apk), "--migrate"), 1)
                self.assertFalse(fake.uninstall_calls())
                self.assertEqual(len(fake.calls), 2)

    def test_wrong_apk_package_is_rejected_before_adb(self):
        self.write_apk(self.apk, "example.other")
        fake = FakeAdb()
        self.assertEqual(self.invoke(fake, "--apk", str(self.apk), "--migrate"), 1)
        self.assertFalse(fake.calls)

    def test_non_debuggable_apk_is_rejected_before_any_device_action(self):
        with zipfile.ZipFile(self.apk, "w") as archive:
            archive.writestr("AndroidManifest.xml", '<manifest package="' + updater.PACKAGE + '"><application/></manifest>')
        fake = FakeAdb()
        self.assertEqual(self.invoke(fake, "--apk", str(self.apk), "--migrate"), 1)
        self.assertFalse(fake.calls)

    def test_missing_corrupt_wrong_version_or_incomplete_save_never_uninstalls(self):
        wrong_version = saved_game()
        wrong_version["version"] = 2
        incomplete = saved_game()
        del incomplete["player"]["position"]
        for data in (b"", b"{}", b"not JSON", json.dumps(wrong_version).encode(), json.dumps(incomplete).encode()):
            with self.subTest(data_type=len(data)):
                fake = FakeAdb(payload=data)
                self.assertEqual(self.invoke(fake, "--apk", str(self.apk), "--migrate", "--backup-dir", str(self.backups)), 1)
                self.assertFalse(fake.uninstall_calls())
                self.assertEqual(fake.payload, data)
                self.assert_no_private_output()

    def test_missing_private_save_never_uninstalls(self):
        fake = FakeAdb()
        fake.payload = None
        self.assertEqual(self.invoke(fake, "--apk", str(self.apk), "--migrate", "--backup-dir", str(self.backups)), 1)
        self.assertFalse(fake.uninstall_calls())

    def test_failed_local_fsync_never_uninstalls(self):
        fake = FakeAdb()
        with mock.patch.object(updater.os, "fsync", side_effect=OSError("disk full")):
            self.assertEqual(self.invoke(fake, "--apk", str(self.apk), "--migrate", "--backup-dir", str(self.backups)), 1)
        self.assertFalse(fake.uninstall_calls())
        self.assertEqual(fake.payload, save_bytes())

    def test_local_backup_changed_after_write_is_rejected_before_uninstall(self):
        fake = FakeAdb()
        original = updater.persist_backup

        def corrupt_after_backup(data, directory):
            backup = original(data, directory)
            backup.path.write_bytes(b"{}")
            return backup

        with mock.patch.object(updater, "persist_backup", side_effect=corrupt_after_backup):
            self.assertEqual(self.invoke(fake, "--apk", str(self.apk), "--migrate", "--backup-dir", str(self.backups)), 1)
        self.assertFalse(fake.uninstall_calls())
        self.assertEqual(fake.payload, save_bytes())

    def test_migration_has_verified_durable_backup_before_uninstall_and_exact_atomic_restore(self):
        fake = FakeAdb()
        original = fake.payload

        def verify_before_uninstall():
            files = list(self.backups.glob("*.json"))
            self.assertEqual(len(files), 1)
            backup, data = updater.load_backup(files[0])
            self.assertEqual(data, original)
            self.assertEqual(backup.digest, updater.sha256(original))
            self.assertTrue(fake.stopped)

        fake.on_uninstall = verify_before_uninstall
        self.assertEqual(self.invoke(fake, "--apk", str(self.apk), "--migrate", "--backup-dir", str(self.backups)), 0)
        self.assertEqual(len(fake.uninstall_calls()), 1)
        self.assertEqual(fake.payload, original)
        self.assertTrue(fake.renamed)
        self.assertEqual(list(self.backups.glob("*.json"))[0].read_bytes(), original)
        self.assert_no_private_output()

    def test_new_install_failure_retains_verified_backup_and_concrete_recovery_command(self):
        fake = FakeAdb(installs=[failure(), failure("INSTALL_FAILED_INSUFFICIENT_STORAGE")])
        self.assertEqual(self.invoke(fake, "--apk", str(self.apk), "--migrate", "--backup-dir", str(self.backups)), 1)
        self.assertEqual(len(fake.uninstall_calls()), 1)
        self.assertIsNone(fake.payload)
        backup = list(self.backups.glob("*.json"))[0]
        self.assertEqual(updater.load_backup(backup)[1], save_bytes())
        output = "\n".join(self.output)
        self.assertIn("No rollback was performed", output)
        self.assertIn(" restore --backup ", output)
        self.assertIn("--serial phone-123", output)
        self.assertIn(str(backup), output)
        self.assert_no_private_output()

    def test_uninstall_failure_retains_backup_and_does_not_attempt_replacement(self):
        fake = FakeAdb()
        fake.uninstall_result = (1, b"Failure [DELETE_FAILED_INTERNAL_ERROR]\n", b"")
        self.assertEqual(self.invoke(fake, "--apk", str(self.apk), "--migrate", "--backup-dir", str(self.backups)), 1)
        self.assertEqual(fake.payload, save_bytes())
        self.assertEqual(sum("install" in command for command, _ in fake.calls), 1)
        self.assertEqual(updater.load_backup(list(self.backups.glob("*.json"))[0])[1], save_bytes())
        self.assertIn("did not confirm uninstall", "\n".join(self.output))

    def test_staging_hash_failure_does_not_rename_and_keeps_backup(self):
        fake = FakeAdb()
        fake.bad_staging = True
        self.assertEqual(self.invoke(fake, "--apk", str(self.apk), "--migrate", "--backup-dir", str(self.backups)), 1)
        self.assertFalse(fake.renamed)
        self.assertIsNone(fake.payload)
        self.assertEqual(updater.load_backup(list(self.backups.glob("*.json"))[0])[1], save_bytes())
        self.assertIn("staging checksum differs", "\n".join(self.output))

    def test_final_hash_failure_is_not_reported_as_success_and_keeps_backup(self):
        fake = FakeAdb()
        fake.bad_readback = True
        self.assertEqual(self.invoke(fake, "--apk", str(self.apk), "--migrate", "--backup-dir", str(self.backups)), 1)
        self.assertNotIn("Migration completed", "\n".join(self.output))
        self.assertEqual(updater.load_backup(list(self.backups.glob("*.json"))[0])[1], save_bytes())

    def test_device_ambiguity_and_wrong_serial_fail_before_install(self):
        fake = FakeAdb()
        fake.devices += b"phone-456\tdevice\n"
        self.assertEqual(self.invoke(fake, "--apk", str(self.apk), "--migrate"), 1)
        self.assertEqual(len(fake.calls), 1)
        fake.calls.clear()
        self.assertEqual(self.invoke(fake, "--apk", str(self.apk), "--serial", "missing", "--migrate"), 1)
        self.assertEqual(len(fake.calls), 1)
        fake.installs = [success()]
        fake.calls.clear()
        self.assertEqual(self.invoke(fake, "--apk", str(self.apk), "--serial", "phone-456"), 0)
        self.assertEqual(fake.calls[1][0][1:3], ["-s", "phone-456"])

    def test_backup_and_recovery_restore_subcommands_work_without_uninstall(self):
        fake = FakeAdb()
        self.assertEqual(self.invoke(fake, "backup", "--backup-dir", str(self.backups)), 0)
        backup = list(self.backups.glob("*.json"))[0]
        fake.payload = None
        fake.installs = [success()]
        self.assertEqual(self.invoke(fake, "restore", "--apk", str(self.apk), "--backup", str(backup)), 0)
        self.assertEqual(fake.payload, save_bytes())
        self.assertFalse(fake.uninstall_calls())
        self.assert_no_private_output()

    def test_corrupt_backup_sidecar_refuses_restore_before_adb(self):
        backup = updater.persist_backup(save_bytes(), self.backups)
        Path(str(backup.path) + ".sha256").write_text("0" * 64)
        fake = FakeAdb()
        self.assertEqual(self.invoke(fake, "restore", "--backup", str(backup.path)), 1)
        self.assertFalse(fake.calls)

    def test_each_backup_is_unique_and_previous_bytes_are_kept(self):
        one = updater.persist_backup(save_bytes(), self.backups)
        two = updater.persist_backup(save_bytes(), self.backups)
        self.assertNotEqual(one.path, two.path)
        self.assertEqual(one.path.read_bytes(), two.path.read_bytes())

    def test_new_backup_directory_chain_is_synced_before_it_can_be_used(self):
        destination = self.root / "new parent" / "new child"
        with mock.patch.object(updater, "_fsync_directory", wraps=updater._fsync_directory) as sync:
            updater.persist_backup(save_bytes(), destination)
        synced = [call.args[0] for call in sync.call_args_list]
        self.assertEqual(synced, [destination, destination.parent, self.root])

    def test_duplicate_json_fields_and_nonfinite_values_are_rejected(self):
        for data in (b'{"version":1,"version":1}', save_bytes().replace(b'"hp": 91.0', b'"hp": NaN')):
            with self.assertRaises(updater.UpdateError):
                updater.validate_save(data)

    def test_binary_android_manifest_package_is_read_without_aapt(self):
        strings = ["manifest", "package", updater.PACKAGE, "application", "debuggable",
                   "http://schemas.android.com/apk/res/android"]
        encoded = [bytes([len(s), len(s.encode())]) + s.encode() + b"\0" for s in strings]
        offsets, offset = [], 0
        for value in encoded:
            offsets.append(offset)
            offset += len(value)
        payload = b"".join(encoded)
        payload += b"\0" * (-len(payload) % 4)
        pool_size = 28 + len(strings) * 4 + len(payload)
        pool = struct.pack("<HHIIIIII", 1, 28, pool_size, len(strings), 0, 0x100, 28 + len(strings) * 4, 0)
        pool += struct.pack("<" + "I" * len(offsets), *offsets) + payload
        start = struct.pack("<HHIII", 0x102, 16, 56, 1, 0xFFFFFFFF)
        start += struct.pack("<IIHHHHHH", 0xFFFFFFFF, 0, 20, 20, 1, 0, 0, 0)
        start += struct.pack("<IIIHBBI", 0xFFFFFFFF, 1, 2, 8, 0, 3, 2)
        application = struct.pack("<HHIII", 0x102, 16, 56, 2, 0xFFFFFFFF)
        application += struct.pack("<IIHHHHHH", 0xFFFFFFFF, 3, 20, 20, 1, 0, 0, 0)
        application += struct.pack("<IIIHBBI", 5, 4, 0xFFFFFFFF, 8, 0, 0x12, 0xFFFFFFFF)
        end_application = struct.pack("<HHIIIII", 0x103, 16, 24, 3, 0xFFFFFFFF, 0xFFFFFFFF, 3)
        end_manifest = struct.pack("<HHIIIII", 0x103, 16, 24, 4, 0xFFFFFFFF, 0xFFFFFFFF, 0)
        body = pool + start + application + end_application + end_manifest
        manifest = struct.pack("<HHI", 3, 8, 8 + len(body)) + body
        self.assertEqual(updater.manifest_package(manifest), updater.PACKAGE)
        self.assertEqual(updater.manifest_details(manifest), (updater.PACKAGE, True))
        false_application = application[:-4] + struct.pack("<I", 0)
        body = pool + start + false_application + end_application + end_manifest
        self.assertEqual(updater.manifest_details(struct.pack("<HHI", 3, 8, 8 + len(body)) + body), (updater.PACKAGE, False))


if __name__ == "__main__":
    unittest.main()
