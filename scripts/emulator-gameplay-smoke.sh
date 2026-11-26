#!/usr/bin/env bash
# Interactive smoke test for a disposable Android emulator, not a user's phone.
# Installs the APK, starts a fresh office-worker game, exercises the real touch
# panels and map annotations, then verifies that both survive Save -> Menu -> Continue.
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
  # State/action logs precede rendering. On SwiftShader, an immediate screencap
  # can still contain the previous screen or a stretched pre-rotation buffer.
  # Two later real-time summaries leave an intervening update/render interval;
  # the first summary may belong to the update that emitted the state log.
  local rendered_count
  alive || return 1
  refresh_log || return 1
  rendered_count=$(( $(log_count 'DTPERF fps=') + 2 ))
  wait_log 'DTPERF fps=' "$rendered_count" 180 || return 1
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
  # Exercise held presses as well as the short taps used by the map below.
  adb shell input swipe "$1" "$2" "$1" "$2" 1000
  sleep 1
}

alive() {
  local process_id
  process_id="$(adb shell pidof -s "$PKG" | tr -d '\r' || true)"
  [[ -n "$process_id" ]] || { echo "Game process exited" >&2; return 1; }
}

wait_pid() {
  local deadline=$((SECONDS + 30)) process_id
  while (( SECONDS < deadline )); do
    process_id="$(adb shell pidof -s "$PKG" | tr -d '\r' || true)"
    if [[ -n "$process_id" ]]; then
      printf 'Started game process: %s\n' "$process_id"
      return 0
    fi
    sleep 1
  done
  echo "Game process did not start within 30 seconds" >&2
  return 1
}

refresh_log() {
  adb logcat -d >"$OUT_DIR/logcat-current.txt" || return 1
  if grep -En 'panicked at|FATAL EXCEPTION|Fatal signal|AM crashed:|Native crash|error\[B000' "$OUT_DIR/logcat-current.txt"; then
    echo "Runtime crash signature found" >&2
    return 1
  fi
}

log_count() {
  # grep prints zero but returns1 when there is no match.
  grep -Ec "$1" "$OUT_DIR/logcat-current.txt" || true
}

wait_log() {
  local pattern="$1" wanted="${2:-1}" deadline=$((SECONDS + ${3:-45}))
  while (( SECONDS < deadline )); do
    alive || return 1
    refresh_log || return 1
    if (( $(log_count "$pattern") >= wanted )); then
      return 0
    fi
    sleep 1
  done
  echo "Missing fresh runtime evidence: $pattern (wanted $wanted)" >&2
  return 1
}

tap_wait() {
  local x="$1" y="$2" pattern="$3" flow="${4:-}" mode="${5:-held}" wanted flow_wanted=0
  alive || return 1
  refresh_log || return 1
  # Take both baselines before input: the app may process it during the swipe.
  wanted=$(( $(log_count "$pattern") + 1 ))
  if [[ -n "$flow" ]]; then
    flow_wanted=$(( $(log_count "$flow") + 1 ))
  fi
  if [[ "$mode" == "quick" ]]; then
    adb shell input tap "$x" "$y"
  else
    tap "$x" "$y"
  fi
  wait_log "$pattern" "$wanted" || return 1
  if [[ -n "$flow" ]]; then
    wait_log "$flow" "$flow_wanted" 90
  fi
}

read_save() {
  adb exec-out run-as "$PKG" cat "files/$SAVE_NAME" >"$OUT_DIR/$1.json" || return 1
  python3 -m json.tool "$OUT_DIR/$1.json" >/dev/null || return 1
}

wait_save() {
  local name="$1" previous="${2:-}" deadline=$((SECONDS + 45))
  while (( SECONDS < deadline )); do
    alive || return 1
    refresh_log || return 1
    if read_save "$name" 2>/dev/null && python3 - "$OUT_DIR/$name.json" "$previous" <<'PY'
import json
import sys

current = json.load(open(sys.argv[1], encoding="utf-8"))
def clock(save):
    return save["campaign"]["game_min"] + save["campaign"]["clock_remainder"]
if sys.argv[2]:
    previous = json.load(open(sys.argv[2], encoding="utf-8"))
    if clock(current) <= clock(previous):
        sys.exit(1)
print("Fresh saved campaign clock:", clock(current))
PY
    then
      return 0
    fi
    sleep 1
  done
  echo "Save did not complete with a fresh campaign clock: $name" >&2
  return 1
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
adb shell am start -W -n "$ACTIVITY"
# Activity launch and process naming are asynchronous. Absence is tolerated only
# during this bounded startup phase; later waits fail immediately if it exits.
wait_pid
wait_log 'DTPERF fps=' 1 180
wait_log 'DTUI flow=Title ' 1
capture 01-title-portrait 390 844

# The real title flow has separate survivor and departure pages. Visiting them
# does not clear the existing save or start the campaign; only Depart does.
tap_wait 195 667 'DTUI action=ChooseSurvivor$'
capture 01a-survivor-portrait 390 844
tap_wait 262 784 'DTUI action=ReviewDeparture$'
capture 01b-departure-portrait 390 844
tap_wait 262 784 'DTUI action=NewGame$' 'DTUI flow=Prologue '
capture 02-prologue-portrait 390 844
tap_wait 195 328 'DTUI action=Skip$' 'DTUI flow=Playing '
capture 03-playing-portrait 390 844

# The map tool creates a note only when the finger is released. A regular short
# Android tap deliberately permits DOWN/UP to arrive in the same slow frame;
# ordered raw touch handling must preserve it without retries or longer holds.
# Reopening while Supplies remains selected catches the held-button regression:
# the opening finger must never become an annotation on the paper beneath it.
tap_wait 334 576 'DTMAP open=true$'
tap_wait 104 719 'DTMAP tool=Mark\(Supplies\)$' '' quick
tap_wait 220 350 'DTMAP notes=1$' '' quick
tap_wait 342 65 'DTMAP open=false$'
tap_wait 334 576 'DTMAP open=true$'
capture 03a-map-supply-portrait 390 844
refresh_log
[[ "$(log_count 'DTMAP notes=')" == 1 ]]
[[ "$(log_count 'DTMAP notes=1$')" == 1 ]]
tap_wait 342 65 'DTMAP open=false$'

# Bottom toolbar: Bag, Shelter, Barter, Interact, Pause.
tap_wait 50 787 'DTUI panel=Some\(Inventory\)$'
capture 04-inventory-portrait 390 844
tap_wait 332 77 'DTUI panel=None$'
tap_wait 122 787 'DTUI panel=Some\(Shelter\)$'
capture 05-shelter-before-actions 390 844

# At the initial home, Deposit and Pray are the first two shelter rows.
# Board windows is disabled because the office worker has no planks.
tap_wait 323 225 'DTUI action=Deposit$'
tap_wait 323 279 'DTUI action=Pray$'
tap 323 333
capture 06-shelter-after-actions 390 844
previous_save=""
if read_save pre-manual 2>/dev/null; then
  previous_save="$OUT_DIR/pre-manual.json"
fi
tap_wait 323 777 'DTUI action=Save$'
wait_save before-menu "$previous_save"
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
assert len(save["map"]["notes"]) == 1, save["map"]["notes"]
assert save["map"]["notes"][0]["kind"] == "supplies", save["map"]["notes"]
assert len(save["map"]["revealed"]) > 0, "Starting area was not discovered"
print("Deposit, once-per-day prayer, disabled build and map note verified in actual Android save.")
PY

tap_wait 332 77 'DTUI panel=None$'
tap_wait 340 787 'DTUI pause=true$'
capture 07-pause-portrait 390 844
tap_wait 195 464 'DTUI action=SaveMenu$' 'DTUI flow=Title '
# Title is reached only after SaveAndMenu completes the write. Keep that exact
# checkpoint too, so a stale menu save cannot pass the post-Continue assertion.
read_save menu-save
capture 08-title-with-save 390 844
tap_wait 195 599 'DTUI action=Continue$' 'DTUI flow=Playing '
tap_wait 122 787 'DTUI panel=Some\(Shelter\)$'
capture 09-continued-shelter 390 844
# A second prayer is disabled. Saving again captures the restored state.
tap 323 279
tap_wait 323 777 'DTUI action=Save$'
wait_save after-continue "$OUT_DIR/menu-save.json"
python3 - "$OUT_DIR/before-menu.json" "$OUT_DIR/menu-save.json" "$OUT_DIR/after-continue.json" <<'PY'
import json
import sys

before, menu, after = (json.load(open(path, encoding="utf-8")) for path in sys.argv[1:])
def clock(save):
    return save["campaign"]["game_min"] + save["campaign"]["clock_remainder"]
for field in ("day", "stash", "upgrades", "pray_day", "group", "karma"):
    assert after["shelter"][field] == before["shelter"][field], field
assert after["inventory"] == before["inventory"]
assert clock(after) > clock(menu) >= clock(before), (clock(before), clock(menu), clock(after))
assert after["player"]["position"] == before["player"]["position"]
assert before["map"]["notes"] == menu["map"]["notes"] == after["map"]["notes"]
assert len(after["map"]["notes"]) == 1 and after["map"]["notes"][0]["kind"] == "supplies"
assert set(before["map"]["revealed"]) <= set(menu["map"]["revealed"]) <= set(after["map"]["revealed"])
print("Continue restored shelter, inventory, position, map notes and discovery without resetting or duplicate dawn/prayer.")
PY

# Native sensor activity: rotate the live session, then inspect both the shelter
# and barter layouts at the smaller landscape height. No new game is started.
adb shell settings put system user_rotation 1
adb emu sensor set acceleration 9.8:0:0
sleep 4
capture 10-shelter-landscape 844 390
# Panel: tabs across y=76..114 at this viewport; Barter is the third tab.
tap_wait 518 95 'DTUI panel=Some\(Trade\)$'
capture 11-barter-landscape 844 390
adb shell settings put system user_rotation 0
adb emu sensor set acceleration 0:9.8:0
sleep 4
capture 12-barter-portrait 390 844

# Inspect the restored map in both orientations. Reopening after Continue still
# has the mark tool selected; holding the map button must not create a second note.
tap_wait 332 77 'DTUI panel=None$'
tap_wait 334 576 'DTMAP open=true$'
capture 13-map-restored-portrait 390 844
adb shell settings put system user_rotation 1
adb emu sensor set acceleration 9.8:0:0
sleep 4
capture 14-map-restored-landscape 844 390
tap_wait 776 35 'DTMAP open=false$'
alive
refresh_log
[[ "$(log_count 'DTUI action=Save$')" == 2 ]]
[[ "$(log_count 'DTUI action=Continue$')" == 1 ]]
[[ "$(log_count 'DTUI action=ChooseSurvivor$')" == 1 ]]
[[ "$(log_count 'DTUI action=ReviewDeparture$')" == 1 ]]
[[ "$(log_count 'DTUI action=NewGame$')" == 1 ]]
[[ "$(log_count 'DTUI action=Pray$')" == 1 ]]
[[ "$(log_count 'DTMAP open=true$')" == 3 ]]
[[ "$(log_count 'DTMAP open=false$')" == 3 ]]
[[ "$(log_count 'DTMAP notes=')" == 1 ]]
[[ "$(log_count 'DTMAP notes=1$')" == 1 ]]

source "$(dirname "${BASH_SOURCE[0]}")/emulator-interiors-smoke.sh"

adb logcat -d >"$OUT_DIR/logcat.txt"
if grep -En 'panicked at|FATAL EXCEPTION|Fatal signal|AM crashed:|Native crash|error\[B000' "$OUT_DIR/logcat.txt"; then
  echo "Runtime crash signature found" >&2
  exit 1
fi
grep -E 'DTUI|DTMAP|DTLOOT|DTDOOR|DTPERF' "$OUT_DIR/logcat.txt" >"$OUT_DIR/events.txt"
printf '%s\n' 'Android gameplay smoke passed: touch panels, short map taps, stock and barricade conservation, cold Continue and orientation.'
