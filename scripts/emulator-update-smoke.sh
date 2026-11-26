#!/usr/bin/env bash
# Run only after gameplay smoke has saved a real checkpoint on a disposable emulator.
# Exercises the user-facing updater with same-key and cross-key APK installations.
set -euo pipefail
umask 077

if [[ $# -ne 1 || ! -f "$1" ]]; then
  printf 'Usage: %s FIXED_SIGNER_X86_64_DEBUG_APK\n' "$0" >&2
  exit 2
fi
SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
HELPER="$SCRIPT_DIR/update-android-test-build.py"
SIGNER_PARSER="$SCRIPT_DIR/../android/test-signing/apksigner_output.py"
APK=$(python3 -c 'from pathlib import Path; import sys; print(Path(sys.argv[1]).resolve())' "$1")
OUT_DIR=$(python3 -c 'from pathlib import Path; import sys; print(Path(sys.argv[1]).resolve())' "${UPDATE_SMOKE_OUT:-emulator-update-smoke}")
PACKAGE=tw.lijiabao.defendtaipei
SAVE_NAME=defendtaipei-native-v1.json

# Discovery is the only unselected adb command. Every subsequent operation,
# including helper invocations, is pinned to this exact authorized serial.
DEVICE_SERIAL=$(adb devices | python3 -c '
import re, sys
requested = sys.argv[1]
devices = {}
for line in sys.stdin:
    fields = line.split()
    if len(fields) >= 2 and fields[0] != "List" and not fields[0].startswith("*"):
        devices[fields[0]] = fields[1]
if requested:
    if devices.get(requested) != "device":
        raise SystemExit("Requested ANDROID_SERIAL is not online and authorized.")
    selected = requested
else:
    online = [serial for serial, state in devices.items() if state == "device"]
    if len(online) != 1:
        raise SystemExit("Update smoke requires exactly one online device or explicit ANDROID_SERIAL.")
    selected = online[0]
if not re.fullmatch(r"emulator-[0-9]+", selected):
    raise SystemExit("Refusing update smoke outside an emulator-* serial.")
print(selected)
' "${ANDROID_SERIAL:-}")
ADB=(adb -s "$DEVICE_SERIAL")
if [[ "$("${ADB[@]}" shell getprop ro.kernel.qemu | tr -d '\r\n')" != 1 ]]; then
  printf 'Refusing signature-changing smoke outside a disposable emulator.\n' >&2
  exit 2
fi

APKSIGNER_BIN=${APKSIGNER:-}
if [[ -z "$APKSIGNER_BIN" ]]; then APKSIGNER_BIN=$(command -v apksigner || true); fi
if [[ -z "$APKSIGNER_BIN" ]]; then
  SDK_ROOT=${ANDROID_SDK_ROOT:-${ANDROID_HOME:-}}
  if [[ -n "$SDK_ROOT" ]]; then
    for candidate in "$SDK_ROOT"/build-tools/*/apksigner; do
      if [[ -x "$candidate" ]]; then APKSIGNER_BIN=$candidate; fi
    done
  fi
fi
if [[ -z "$APKSIGNER_BIN" || ! -x "$APKSIGNER_BIN" ]]; then
  printf 'apksigner is required; set APKSIGNER or ANDROID_SDK_ROOT.\n' >&2
  exit 2
fi
KEYTOOL_BIN=$(command -v keytool || true)
if [[ -z "$KEYTOOL_BIN" ]]; then
  printf 'A JDK keytool is required for the disposable alternate signer.\n' >&2
  exit 2
fi

mkdir -p "$OUT_DIR"
RUN_DIR=$(mktemp -d "$OUT_DIR/run-XXXXXX")
# This directory must never be uploaded as a workflow artifact. All private
# signing material, its password and the alternate APK are deleted on exit.
PRIVATE_DIR=$(mktemp -d /tmp/defendtaipei-update-key.XXXXXX)
cleanup() {
  local status=$?
  rm -rf -- "$PRIVATE_DIR"
  exit "$status"
}
trap cleanup EXIT
python3 - "$PRIVATE_DIR" "$OUT_DIR" <<'PY_PRIVATE'
from pathlib import Path
import sys
private, output = (Path(p).resolve() for p in sys.argv[1:])
if private == output or output in private.parents:
    raise SystemExit("Private signing directory must be outside UPDATE_SMOKE_OUT.")
PY_PRIVATE

"$APKSIGNER_BIN" verify --verbose --print-certs-pem "$APK" >"$RUN_DIR/fixed-signer.log" 2>&1
EXPECTED_SIGNER=$(tr -d '[:space:]' < "$SCRIPT_DIR/../android/test-signing/certificate.sha256")
FIXED_SIGNER=$(python3 "$SIGNER_PARSER" --expected "$EXPECTED_SIGNER" <"$RUN_DIR/fixed-signer.log")
python3 - "$APK" "$RUN_DIR/evidence.json" "$DEVICE_SERIAL" "$FIXED_SIGNER" <<'PY_BASE'
import hashlib
import json
from pathlib import Path
import sys
import zipfile
apk, output = map(Path, sys.argv[1:3])
expected = sys.argv[4]
with zipfile.ZipFile(apk) as archive:
    assert any(name.startswith("lib/x86_64/") and name.endswith(".so") for name in archive.namelist()), "Expected the x86_64 debug APK"
output.write_text(json.dumps({
    "test": "Actual disposable-emulator execution of the user-facing ADB update helper",
    "result": "in_progress",
    "serial": sys.argv[3],
    "package": "tw.lijiabao.defendtaipei",
    "apk_file": apk.name,
    "apk_sha256": hashlib.sha256(apk.read_bytes()).hexdigest(),
    "fixed_signer_sha256": expected,
    "checkpoints": [],
    "scope": ["same-signer update", "different-signer migration", "return to fixed signer", "private save bytes and backup checksums"],
    "not_tested": ["physical-device USB reliability", "playing or pressing Continue after migration"],
    "temporary_key": "Generated outside the artifact directory and deleted on script exit",
}, indent=2) + "\n")
PY_BASE

run_helper() {
  local stage=$1
  shift
  python3 "$HELPER" "$@" --serial "$DEVICE_SERIAL" 2>&1 | tee "$RUN_DIR/$stage.log"
}

# Force-stop + exact private-file read are performed by the same CLI users run.
run_helper baseline-backup backup --backup-dir "$RUN_DIR/baseline-backups"
BASELINE=$(python3 - "$RUN_DIR/baseline-backups" <<'PY_BACKUP'
from pathlib import Path
import sys
files = list(Path(sys.argv[1]).glob("*.json"))
assert len(files) == 1, "Expected exactly one new baseline backup"
print(files[0])
PY_BACKUP
)

checkpoint() {
  local stage=$1 backup_dir=${2:-}
  "${ADB[@]}" exec-out run-as "$PACKAGE" cat "files/$SAVE_NAME" >"$RUN_DIR/$stage-save.json"
  python3 - "$BASELINE" "$RUN_DIR/$stage-save.json" "$RUN_DIR/evidence.json" "$stage" "$backup_dir" <<'PY_CHECKPOINT'
import hashlib
import json
from pathlib import Path
import sys
baseline, current, evidence = map(Path, sys.argv[1:4])
expected, actual = baseline.read_bytes(), current.read_bytes()
digest = hashlib.sha256(expected).hexdigest()
assert baseline.with_name(baseline.name + ".sha256").read_text().strip() == digest, "Baseline checksum mismatch"
assert actual == expected, "Private save changed during APK update/migration"
assert json.loads(actual)["version"] == 1, "Expected a real native version-1 checkpoint"
backups = []
if sys.argv[5]:
    directory = Path(sys.argv[5])
    files = list(directory.glob("*.json"))
    assert len(files) == 1, "A signature migration must produce exactly one verified backup"
    for path in files:
        assert path.read_bytes() == expected, "Migration backup differs from baseline"
        assert path.with_name(path.name + ".sha256").read_text().strip() == digest, "Migration backup checksum mismatch"
        backups.append(str(path.relative_to(evidence.parent)))
record = json.loads(evidence.read_text())
record["baseline_save_sha256"] = digest
record["checkpoints"].append({"stage": sys.argv[4], "save_file": current.name, "save_sha256": digest,
                              "bytes": len(actual), "verified_migration_backups": backups})
temporary = evidence.with_suffix(".tmp")
temporary.write_text(json.dumps(record, indent=2) + "\n")
temporary.replace(evidence)
print(sys.argv[4] + ": exact private save and backup checksums verified")
PY_CHECKPOINT
}

run_helper same-signer-update --apk "$APK" --backup-dir "$RUN_DIR/same-signer-unused"
checkpoint same-signer-update
if [[ -d "$RUN_DIR/same-signer-unused" ]]; then
  printf 'Same-signer update unexpectedly entered the migration backup path.\n' >&2
  exit 1
fi

python3 - "$PRIVATE_DIR/password.txt" <<'PY_PASSWORD'
from pathlib import Path
import secrets
import sys
Path(sys.argv[1]).write_text(secrets.token_hex(32) + "\n")
PY_PASSWORD
"$KEYTOOL_BIN" -genkeypair -noprompt -storetype PKCS12 -keystore "$PRIVATE_DIR/alternate.p12" \
  -storepass:file "$PRIVATE_DIR/password.txt" -keypass:file "$PRIVATE_DIR/password.txt" \
  -alias update-smoke -keyalg RSA -keysize 2048 -validity 2 \
  -dname 'CN=DefendTaipei CI Update Smoke,O=Disposable Test,C=TW' >"$RUN_DIR/alternate-key-generation.log" 2>&1
cp -- "$APK" "$PRIVATE_DIR/alternate-source.apk"
"$APKSIGNER_BIN" sign --ks "$PRIVATE_DIR/alternate.p12" --ks-key-alias update-smoke \
  --ks-pass "file:$PRIVATE_DIR/password.txt" --v4-signing-enabled false --out "$PRIVATE_DIR/alternate.apk" \
  "$PRIVATE_DIR/alternate-source.apk" >"$RUN_DIR/alternate-signing.log" 2>&1
"$APKSIGNER_BIN" verify --verbose --print-certs-pem "$PRIVATE_DIR/alternate.apk" >"$RUN_DIR/alternate-signer.log" 2>&1
ALTERNATE_SIGNER=$(python3 "$SIGNER_PARSER" <"$RUN_DIR/alternate-signer.log")
python3 - "$RUN_DIR/evidence.json" "$ALTERNATE_SIGNER" <<'PY_ALTERNATE'
import json
from pathlib import Path
import sys
evidence = Path(sys.argv[1])
alternate = sys.argv[2]
record = json.loads(evidence.read_text())
assert alternate != record["fixed_signer_sha256"], "Alternate signer did not change"
record["alternate_signer_sha256"] = alternate
evidence.write_text(json.dumps(record, indent=2) + "\n")
PY_ALTERNATE

run_helper migrate-alternate --apk "$PRIVATE_DIR/alternate.apk" --backup-dir "$RUN_DIR/alternate-backups" --migrate
checkpoint migrate-alternate "$RUN_DIR/alternate-backups"
run_helper migrate-fixed --apk "$APK" --backup-dir "$RUN_DIR/fixed-backups" --migrate
checkpoint migrate-fixed "$RUN_DIR/fixed-backups"
python3 - "$APK" "$RUN_DIR/evidence.json" <<'PY_FINISH'
import hashlib
import json
from pathlib import Path
import sys
apk, evidence = map(Path, sys.argv[1:])
record = json.loads(evidence.read_text())
assert hashlib.sha256(apk.read_bytes()).hexdigest() == record["apk_sha256"], "Original APK was modified"
assert [step["stage"] for step in record["checkpoints"]] == ["same-signer-update", "migrate-alternate", "migrate-fixed"]
record["result"] = "passed"
evidence.write_text(json.dumps(record, indent=2) + "\n")
print("Android update smoke passed: same signer and both migration directions preserved exact private save bytes.")
print("Evidence:", evidence)
PY_FINISH
