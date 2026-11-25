#!/usr/bin/env bash
# Interactive smoke test for a disposable Android emulator, not a user's phone.
# Installs the APK, starts a fresh office-worker game, exercises the real touch
# panels, then verifies that deposit/prayer survive Save -> Menu -> Continue.
# The screenshots are evidence for visual review; this script does not perform OCR.
set -euo pipefail

APK="${1:-DefendTaipei-x86_64-debug.apk}"
PKG="tw.lijiabao.defendtaipei"
ACTIVITY="${PKG}/.MainActivity"
OUT_DIR="${GAMEPLAY_SMOKE_OUT:-emulator-gameplay-smoke}"
SAVE_NAME="defendtaipei-native-v1.json"

mkdir -p "$OUT_DIR"
adb wait-for-device
if [[ "$(adb shell getprop ro.kernel.qemu | tr -d '\r')" != "1" ]]; then
  echo "Refusing to clear game data: this test requires a disposable emulator." >&2
  exit 2
fi

collect() {
  adb logcat -d >"$OUT_DIR/logcat.txt" 2>/dev/null || true
  adb exec-out screencap -p >"$OUT_DIR/final.png" 2>/dev/null || true
  adb shell wm size reset >/dev/null 2>&1 || true
  adb shell wm density reset >/dev/null 2>&1 || true
  adb shell settings put system accelerometer_rotation 1 >/dev/null 2>&1 || true
  adb emu sensor set acceleration 0:9.8:0 >/dev/null 2>&1 || true
}
trap collect EXIT

capture() {
  adb exec-out screencap -p >"$OUT_DIR/$1.png"
  python3 - "$OUT_DIR/$1.png" "$2" "$3" <<'PY'
import struct
import sys
from pathlib import Path

png = Path(sys.argv[1]).read_bytes()
assert png[:8] == b"\x89PNG\r\n\x1a\n", "screencap did not return PNG bytes"
width, height = struct.unpack(">II", png[16:24])
assert (width, height) == (int(sys.argv[2]), int(sys.argv[3])), (width, height)
PY
}

tap() {
  # Keep the press across a software-rendered frame, so down/up cannot both be
  # missed between two updates on a slow CI emulator.
  adb shell input swipe "$1" "$2" "$1" "$2" 180
  sleep 1
}

alive() {
  local process_id
  process_id="$(adb shell pidof -s "$PKG" | tr -d '\r')"
  [[ -n "$process_id" ]] || { echo "Game process exited" >&2; exit 1; }
}

wait_log() {
  local pattern="$1"
  for _ in {1..45}; do
    adb logcat -d >"$OUT_DIR/logcat-current.txt"
    if grep -Eq "$pattern" "$OUT_DIR/logcat-current.txt"; then
      return 0
    fi
    alive
    sleep 1
  done
  echo "Missing runtime evidence: $pattern" >&2
  return 1
}

read_save() {
  adb exec-out run-as "$PKG" cat "files/$SAVE_NAME" >"$OUT_DIR/$1.json"
  python3 -m json.tool "$OUT_DIR/$1.json" >/dev/null
}

adb install -r -t "$APK"
adb shell am force-stop "$PKG"
adb shell pm clear "$PKG"
# At mdpi one Android pixel is one Bevy logical pixel. These viewport dimensions
# are covered by ui::layout and ui::shelter tests, so taps follow the shipped UI.
adb shell wm size 390x844
adb shell wm density 160
adb shell settings put system accelerometer_rotation 1
adb shell settings put system user_rotation 0
adb emu sensor set acceleration 0:9.8:0
adb logcat -c
adb shell am start -n "$ACTIVITY"
wait_log 'DTPERF fps='
sleep 6
capture 01-title-portrait 390 844

# TitleLayout: New game is the right bottom button. Prologue skip is below its
# phone copy, and these taps never rely on tapping through the selection cards.
tap 287 768
capture 02-prologue-portrait 390 844
tap 195 328
sleep 3
capture 03-playing-portrait 390 844

# Bottom toolbar: Bag, Shelter, Barter, Interact, Pause.
tap 50 787
wait_log 'DTUI panel=Some\(Inventory\)'
capture 04-inventory-portrait 390 844
tap 332 77
tap 122 787
wait_log 'DTUI panel=Some\(Shelter\)'
capture 05-shelter-before-actions 390 844

# At the initial home, Deposit and Pray are the first two shelter rows.
# Board windows is disabled because the office worker has no planks.
tap 323 225
wait_log 'DTUI action=Deposit'
tap 323 279
wait_log 'DTUI action=Pray'
tap 323 333
capture 06-shelter-after-actions 390 844
tap 323 777
wait_log 'DTUI action=Save'
sleep 2
read_save before-menu
python3 - "$OUT_DIR/before-menu.json" <<'PY'
import json
import sys

save = json.load(open(sys.argv[1], encoding="utf-8"))
bag = dict(save["inventory"]["items"])
stock = dict(save["shelter"]["stash"])
assert save["role_id"] == "office", save["role_id"]
assert all(bag.get(key, 0) == 0 for key in ("coffee", "water", "noodle")), bag
assert stock.get("coffee", 0) == 1 and stock.get("noodle", 0) == 1, stock
assert stock.get("water", 0) == 3 and stock.get("rice", 0) == 2, stock
assert save["shelter"]["pray_day"] == save["shelter"]["day"] == 1
assert not save["shelter"]["upgrades"], "Unaffordable build should remain disabled"
print("Deposit, once-per-day prayer and disabled build verified in actual Android save.")
PY

tap 332 77
tap 340 787
capture 07-pause-portrait 390 844
tap 195 464
sleep 2
capture 08-title-with-save 390 844
tap 104 768
sleep 3
tap 122 787
capture 09-continued-shelter 390 844
# A second prayer is disabled. Saving again captures the restored state.
tap 323 279
tap 323 777
sleep 2
read_save after-continue
python3 - "$OUT_DIR/before-menu.json" "$OUT_DIR/after-continue.json" <<'PY'
import json
import sys

before, after = (json.load(open(path, encoding="utf-8")) for path in sys.argv[1:])
for field in ("day", "stash", "upgrades", "pray_day", "group", "karma"):
    assert after["shelter"][field] == before["shelter"][field], field
assert after["inventory"] == before["inventory"]
assert after["campaign"]["game_min"] >= before["campaign"]["game_min"]
assert after["player"]["position"] == before["player"]["position"]
print("Continue restored shelter, inventory and position without resetting or duplicate dawn/prayer.")
PY

# Native sensor activity: rotate the live session, then inspect both the shelter
# and barter layouts at the smaller landscape height. No new game is started.
adb shell settings put system user_rotation 1
adb emu sensor set acceleration 9.8:0:0
sleep 4
capture 10-shelter-landscape 844 390
# Panel: tabs across y=76..114 at this viewport; Barter is the third tab.
tap 518 95
wait_log 'DTUI panel=Some\(Trade\)'
capture 11-barter-landscape 844 390
adb shell settings put system user_rotation 0
adb emu sensor set acceleration 0:9.8:0
sleep 4
capture 12-barter-portrait 390 844
alive

adb logcat -d >"$OUT_DIR/logcat.txt"
if grep -En 'panicked at|FATAL EXCEPTION|Fatal signal|AM crashed:|Native crash|error\[B000' "$OUT_DIR/logcat.txt"; then
  echo "Runtime crash signature found" >&2
  exit 1
fi
grep -E 'DTUI|DTPERF' "$OUT_DIR/logcat.txt" >"$OUT_DIR/events.txt"
printf '%s\n' 'Android gameplay smoke passed: touch panels, persistence and orientation.'
