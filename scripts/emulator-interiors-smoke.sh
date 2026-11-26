#!/usr/bin/env bash
# Sourced after the ordinary gameplay smoke's exact baseline assertions.
# Uses controlled saves on the already-verified disposable emulator to isolate
# stock/door UI behavior. These stages do not test walking to the shop or combat.
if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  echo "Source this script from emulator-gameplay-smoke.sh after its baseline checks." >&2
  exit 2
fi
if [[ "$(adb shell getprop ro.kernel.qemu | tr -d '\r')" != "1" ]]; then
  echo "Refusing fixture injection outside a disposable emulator." >&2
  exit 2
fi

INDOOR_FIXTURE_FILE="$(dirname "${BASH_SOURCE[0]}")/../game/tests/fixtures/android-indoor-smoke.json"

# Derive both fixture saves from an actual checkpoint made by the baseline test.
# The original checkpoint is never edited. No new schema or game debug controls
# are introduced, and all later Continue checks use actual saved bytes unchanged.
python3 - "$INDOOR_FIXTURE_FILE" "$OUT_DIR/after-continue.json" "$OUT_DIR" <<'PY_FIXTURES'
import copy
import hashlib
import json
import math
import sys
from pathlib import Path

fixture_path, baseline_path, output = map(Path, sys.argv[1:])
fixture = json.loads(fixture_path.read_text())
baseline_bytes = baseline_path.read_bytes()
baseline = json.loads(baseline_bytes)
assert fixture["version"] == 1
assert fixture["shelf"]["id"] == "x814_3" and fixture["shelf"]["building"] == 814
assert fixture["door"]["id"] == 97
for point in (fixture["shelf"]["approach"], fixture["door"]["position"]):
    assert len(point) == 2 and all(math.isfinite(v) for v in point)
assert baseline["version"] == 1 and baseline["role_id"] == "office"

prepared = {}
for kind in ("shelf", "door"):
    save = copy.deepcopy(baseline)
    world = save["world"]
    world["fixtures_version"] = 1
    # Keep unrelated patrol combat from racing slow software-rendered UI taps.
    # The fixed native patrol planner uses IDs 0..63; this isolation is explicit
    # in the provenance artifact and is not evidence of combat or traversal.
    world["dead"] = sorted(set(world["dead"]) | set(range(64)))
    world.setdefault("barricaded_doors", [])
    save["inventory"] = {"items": [], "equipped": ""}
    point = fixture[kind]["approach" if kind == "shelf" else "position"]
    save["player"]["position"] = [point[0], 0.0, point[1]]
    save["campaign"]["speed"] = 1
    save["campaign"]["sleeping"] = False
    if kind == "shelf":
        key = fixture["shelf"]["id"]
        world["loot"] = [entry for entry in world["loot"] if entry[0] != key]
        world["loot"].append([key, [["water", 3], ["can", 2]]])
        for field in ("searched", "asked", "night_seen"):
            if key not in world[field]:
                world[field].append(key)
        owner = f'c{fixture["shelf"]["building"]}'
        if owner not in world["asked"]:
            world["asked"].append(owner)
        world["closed_doors"] = [i for i in world["closed_doors"] if i != fixture["shelf"]["building"]]
        world["barricaded_doors"] = [i for i in world["barricaded_doors"] if i != fixture["shelf"]["building"]]
    else:
        door = fixture["door"]["id"]
        save["inventory"]["items"] = [["hammer", 1], ["plank", 2]]
        world["closed_doors"] = sorted(set(world["closed_doors"]) | {door})
        world["barricaded_doors"] = [i for i in world["barricaded_doors"] if i != door]
    path = output / f"indoor-{kind}-fixture.json"
    path.write_text(json.dumps(save, ensure_ascii=False, separators=(",", ":")) + "\n")
    prepared[kind] = {"file": path.name, "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}

(output / "indoor-smoke-provenance.json").write_text(json.dumps({
    "test": "Actual Android touch actions from controlled disposable save fixtures",
    "baseline_file": baseline_path.name,
    "baseline_sha256": hashlib.sha256(baseline_bytes).hexdigest(),
    "geometry_descriptor": fixture,
    "prepared_saves": prepared,
    "patrol_isolation": "Native patrol IDs 0..63 are marked dead only in the injected fixture copies",
    "scope": ["item selection", "partial and empty stock persistence", "barricade spend/refund", "closed/open door state", "portrait/landscape layouts"],
    "not_tested": ["walking to either location", "combat", "physical-device performance"],
    "continue_checkpoints": "Actual Android saved files are reinstalled byte-for-byte, without editing counts or door state",
}, ensure_ascii=False, indent=2) + "\n")
print("Prepared isolated indoor smoke saves from:", baseline_path.name)
PY_FIXTURES

indoor_assert_save() {
  python3 - "$INDOOR_FIXTURE_FILE" "$OUT_DIR/$1.json" "$2" "$OUT_DIR/after-continue.json" <<'PY_VERIFY'
import json
import math
import sys

fixture, save, mode, baseline = (
    json.load(open(sys.argv[1], encoding="utf-8")),
    json.load(open(sys.argv[2], encoding="utf-8")),
    sys.argv[3],
    json.load(open(sys.argv[4], encoding="utf-8")),
)
def counts(entries):
    assert len(entries) == len({key for key, _ in entries}), "Duplicate item IDs"
    assert all(isinstance(n, int) and n >= 0 for _, n in entries)
    return {key: n for key, n in entries if n}

bag = counts(save["inventory"]["items"])
world = save["world"]
assert world["fixtures_version"] == 1
assert save["map"]["notes"] == baseline["map"]["notes"], "Fixture smoke altered the player's map note"
assert save["role_id"] == baseline["role_id"] and save["female"] == baseline["female"]
assert save["inventory"]["equipped"] == ""
assert save["player"]["hp"] > 0
assert set(range(64)) <= set(world["dead"])

if mode in ("partial", "empty"):
    key = fixture["shelf"]["id"]
    records = [stock for name, stock in world["loot"] if name == key]
    assert len(records) == 1, "Missing or duplicate authoritative shelf record"
    stock = counts(records[0])
    expected_bag, expected_stock = (
        ({"water": 1}, {"water": 2, "can": 2}) if mode == "partial"
        else ({"water": 3, "can": 2}, {})
    )
    assert bag == expected_bag, (mode, "bag", bag)
    assert stock == expected_stock, (mode, "stock", stock)
    for item, total in (("water", 3), ("can", 2)):
        assert bag.get(item, 0) + stock.get(item, 0) == total
    for field in ("searched", "asked", "night_seen"):
        assert world[field].count(key) == 1, (field, key)
    point = fixture["shelf"]["approach"]
else:
    assert mode in ("boarded", "removed", "opened"), mode
    door = fixture["door"]["id"]
    assert bag == {"hammer": 1, "plank": 1 if mode == "boarded" else 2}, (mode, bag)
    assert (door in world["barricaded_doors"]) == (mode == "boarded"), (mode, world["barricaded_doors"])
    assert (door in world["closed_doors"]) == (mode != "opened"), mode
    point = fixture["door"]["position"]
position = save["player"]["position"]
assert math.dist([position[0], position[2]], point) < 0.0001, "UI touch moved the player"
assert abs(position[1]) < 0.0001
print("Verified actual Android checkpoint:", mode, "bag", bag)
PY_VERIFY
}

indoor_save_checkpoint() {
  local name="$1" x="$2" y="$3"
  # An autosave may have happened during the preceding render wait. Baseline
  # immediately before this press so an older automatic write cannot satisfy it.
  read_save "$name-before-manual"
  tap_wait "$x" "$y" 'DTUI action=Save$'
  wait_save "$name" "$OUT_DIR/$name-before-manual.json"
}

indoor_restart_from() {
  local checkpoint="$1" tag="$2" title_wanted deadline process_id
  adb shell am force-stop "$PKG"
  deadline=$((SECONDS + 15))
  while :; do
    process_id="$(adb shell pidof -s "$PKG" | tr -d '\r' || true)"
    [[ -z "$process_id" ]] && break
    (( SECONDS < deadline )) || { echo "Fixture injection requires a stopped app" >&2; return 1; }
    sleep 1
  done
  # Input goes over a non-PTY channel to a temporary private file; move only
  # after all bytes arrive. Fixed package/save names come from the parent test.
  adb shell -T "run-as $PKG sh -c 'cat > files/$SAVE_NAME.fixture && mv files/$SAVE_NAME.fixture files/$SAVE_NAME'" <"$checkpoint"
  adb exec-out run-as "$PKG" cat "files/$SAVE_NAME" >"$OUT_DIR/$tag-installed.json"
  cmp "$checkpoint" "$OUT_DIR/$tag-installed.json"
  adb shell settings put system user_rotation 0
  adb emu sensor set acceleration 0:9.8:0
  refresh_log
  title_wanted=$(( $(log_count 'DTUI flow=Title ') + 1 ))
  adb shell am start -W -n "$ACTIVITY"
  wait_pid
  wait_log 'DTUI flow=Title ' "$title_wanted" 180
  # Required before the first tap following any rotation or cold launch.
  capture "$tag-title" 390 844
  tap_wait 195 599 'DTUI action=Continue$' 'DTUI flow=Playing '
  capture "$tag-world" 390 844
}

refresh_log
indoor_take_start=$(log_count 'DTLOOT take id=x814_3 ')
indoor_open_start=$(log_count 'DTLOOT open id=x814_3 ')
indoor_board_start=$(log_count 'DTDOOR barricaded=true id=97 ')
indoor_remove_start=$(log_count 'DTDOOR barricaded=false id=97 ')

indoor_restart_from "$OUT_DIR/indoor-shelf-fixture.json" 15-shelf-fixture
# The bottom Interact button selects the real fixture at the checked position.
tap_wait 268 787 'DTLOOT open id=x814_3 stacks=2$' 'DTUI panel=Some\(Scavenge\)$'
tap_wait 271 217 'DTLOOT take id=x814_3 moved=1 left=4 weight='
indoor_save_checkpoint indoor-shelf-partial 324 731
indoor_assert_save indoor-shelf-partial partial
capture 16-scavenge-partial-portrait 390 844

tap_wait 332 217 'DTLOOT take id=x814_3 moved=2 left=2 weight='
tap_wait 110 731 'DTLOOT take id=x814_3 moved=2 left=0 weight='
indoor_save_checkpoint indoor-shelf-empty 324 731
indoor_assert_save indoor-shelf-empty empty
adb shell settings put system user_rotation 1
adb emu sensor set acceleration 9.8:0:0
capture 17-scavenge-empty-landscape 844 390

# Reinstall exactly the checkpoint written by real pickups, not a new fixture.
indoor_restart_from "$OUT_DIR/indoor-shelf-empty.json" 18-shelf-continue
tap_wait 268 787 'DTLOOT open id=x814_3 stacks=0$' 'DTUI panel=Some\(Scavenge\)$'
indoor_save_checkpoint indoor-shelf-continued 324 731
indoor_assert_save indoor-shelf-continued empty
capture 19-scavenge-empty-continued 390 844

indoor_restart_from "$OUT_DIR/indoor-door-fixture.json" 20-door-fixture
tap_wait 50 787 'DTUI panel=Some\(Inventory\)$'
tap_wait 323 277 'DTDOOR barricaded=true id=97 plank=1$'
indoor_save_checkpoint indoor-door-boarded 323 777
indoor_assert_save indoor-door-boarded boarded
tap_wait 332 77 'DTUI panel=None$'
capture 21-door-barricaded-portrait 390 844

indoor_restart_from "$OUT_DIR/indoor-door-boarded.json" 22-door-continue
tap_wait 50 787 'DTUI panel=Some\(Inventory\)$'
indoor_save_checkpoint indoor-door-restored 323 777
indoor_assert_save indoor-door-restored boarded
tap_wait 323 277 'DTDOOR barricaded=false id=97 plank=2$'
indoor_save_checkpoint indoor-door-removed 323 777
indoor_assert_save indoor-door-removed removed
tap_wait 332 77 'DTUI panel=None$'
capture 23-door-removed-closed 390 844

tap_wait 268 787 'DTUI action=Interact$'
capture 24-door-open-portrait 390 844
tap_wait 50 787 'DTUI panel=Some\(Inventory\)$'
indoor_save_checkpoint indoor-door-open 323 777
indoor_assert_save indoor-door-open opened
tap_wait 332 77 'DTUI panel=None$'
alive
refresh_log
[[ $(log_count 'DTLOOT take id=x814_3 ') -eq $((indoor_take_start + 3)) ]]
[[ $(log_count 'DTLOOT open id=x814_3 ') -eq $((indoor_open_start + 2)) ]]
[[ $(log_count 'DTDOOR barricaded=true id=97 ') -eq $((indoor_board_start + 1)) ]]
[[ $(log_count 'DTDOOR barricaded=false id=97 ') -eq $((indoor_remove_start + 1)) ]]

# One further controlled scenario: already-opened cold storage reaches the Web
# six-game-hour blackout boundary, then survives an unmodified cold Continue.
# Preserve all seven checkpoints above and their original runtime assertions.
AGING_FIXTURE_FILE="$(dirname "${BASH_SOURCE[0]}")/../game/tests/fixtures/android-aging-smoke.json"
python3 - "$AGING_FIXTURE_FILE" "$OUT_DIR/after-continue.json" "$OUT_DIR" <<'PY_AGING_PREPARE'
import copy
import hashlib
import json
import math
import sys
from pathlib import Path

descriptor_path, baseline_path, output = map(Path, sys.argv[1:])
fixture = json.loads(descriptor_path.read_text())
baseline_bytes = baseline_path.read_bytes()
save = copy.deepcopy(json.loads(baseline_bytes))
cold = fixture["container"]
assert fixture["version"] == 1 and cold["id"] == "x814_1" and cold["building"] == 814
assert fixture["control_id"] == "x814_3"
assert fixture["power_off"] == 4020 and fixture["game_min"] == fixture["power_off"] + 361
assert len(cold["approach"]) == 2 and all(math.isfinite(v) for v in cold["approach"])
minute = float(fixture["game_min"])
world = save["world"]
world["fixtures_version"] = 1
world["dead"] = sorted(set(world["dead"]) | set(range(64)))
# Only these two authoritative stock records participate. The ordinary shelf
# contains the same fresh food as a negative control, plus unchanged dry goods.
world["loot"] = [[key, [["bento", 2], ["can", 4], ["water", 2]]]
                 for key in (cold["id"], fixture["control_id"])]
world["drops"] = []
for field in ("searched", "asked", "night_seen"):
    world[field] = list(dict.fromkeys([*world[field], cold["id"], fixture["control_id"]]))
world["asked"] = list(dict.fromkeys([*world["asked"], f'c{cold["building"]}']))
world["closed_doors"] = [i for i in world["closed_doors"] if i != cold["building"]]
world["barricaded_doors"] = [i for i in world.get("barricaded_doors", []) if i != cold["building"]]
world["container_aging"] = {"minute": minute - 1.0, "elapsed_hours": 6.0, "seed": 1972}
world["weather"].update(next=minute + 1440.0, until=0.0, amount=0.0, wet=0.0)
save["campaign"].update(game_min=minute, clock_remainder=0.0, day_hours=1,
                         speed=1, sleeping=False, notice="", notice_t=0.0)
save["shelter"]["day"] = 1 + int((minute - 360.0) // 1440.0)
save["environment"].update(power=False, water=True, raining=False)
save["needs"].update(hunger=100.0, thirst=100.0, fatigue=0.0)
save["inventory"] = {"items": [], "equipped": ""}
save["player"].update(hp=100.0, bleed=False, stun=0.0,
                       position=[cold["approach"][0], 0.0, cold["approach"][1]])
path = output / "indoor-aging-fixture.json"
path.write_text(json.dumps(save, ensure_ascii=False, separators=(",", ":")) + "\n")
(output / "indoor-aging-provenance.json").write_text(json.dumps({
    "test": "Actual Android opened-container aging followed by cold Continue",
    "baseline_file": baseline_path.name,
    "baseline_sha256": hashlib.sha256(baseline_bytes).hexdigest(),
    "geometry_descriptor": fixture,
    "prepared_save": {"file": path.name, "sha256": hashlib.sha256(path.read_bytes()).hexdigest()},
    "controlled_state": ["checked refrigerator approach", "game minute and power flag",
                         "six elapsed blackout hours", "deterministic seed 1972",
                         "opened cold storage and ordinary-shelf control", "empty carried bag",
                         "native patrol IDs 0..63 suppressed"],
    "expected_runtime_mutation": "one bento becomes one rotten item only in cold storage; seed advances to 1380227",
    "continue_check": "The first actual Android saved file is reinstalled byte-for-byte; no clock or stock edit",
    "not_tested": ["waiting six real/game hours", "walking to the refrigerator", "combat", "physical-device performance"],
}, ensure_ascii=False, indent=2) + "\n")
PY_AGING_PREPARE

aging_assert_save() {
  python3 - "$AGING_FIXTURE_FILE" "$OUT_DIR/$1.json" "$OUT_DIR/after-continue.json" "$2" <<'PY_AGING_VERIFY'
import json
import math
import sys

fixture, save, baseline = [json.load(open(path, encoding="utf-8")) for path in sys.argv[1:4]]
mode = sys.argv[4]
world = save["world"]
def counts(stock):
    assert len(stock) == len({key for key, _ in stock}), "Duplicate stock IDs"
    assert all(isinstance(n, int) and n >= 0 for _, n in stock)
    return {key: n for key, n in stock if n}
records = world["loot"]
assert len(records) == 2 and len({key for key, _ in records}) == 2
stocks = {key: counts(items) for key, items in records}
assert stocks[fixture["container"]["id"]] == {"bento": 1, "rotten": 1, "can": 4, "water": 2}, stocks
assert stocks[fixture["control_id"]] == {"bento": 2, "can": 4, "water": 2}, stocks
assert counts(save["inventory"]["items"]) == {} and save["inventory"]["equipped"] == ""
assert world["fixtures_version"] == 1 and set(range(64)) <= set(world["dead"])
assert save["role_id"] == baseline["role_id"] and save["female"] == baseline["female"]
assert save["map"]["notes"] == baseline["map"]["notes"]
assert save["player"]["hp"] > 0 and not save["environment"]["power"]
position = save["player"]["position"]
assert math.dist([position[0], position[2]], fixture["container"]["approach"]) < 0.0001
clock = world["container_aging"]
minute = save["campaign"]["game_min"]
assert fixture["game_min"] <= minute < fixture["game_min"] + 350, "A second six-hour interval must not be forced"
assert clock["seed"] == 1380227, "Continue rerolled or skipped the first real RNG draw"
assert abs(clock["minute"] - minute) < 0.02
assert abs(clock["elapsed_hours"] - (minute - fixture["game_min"] + 1.0) / 60.0) < 0.02
assert 0 < clock["elapsed_hours"] < 6.0
if mode == "continued":
    print("Verified actual Android checkpoint: indoor-food-aged-continued; exact stock, controls and saved RNG preserved")
else:
    assert mode == "initial"
    print("Verified initial Android aging snapshot before cold Continue")
PY_AGING_VERIFY
}

refresh_log
aging_start=$(log_count 'DTLOOT aging converted=1 intervals=1$')
indoor_restart_from "$OUT_DIR/indoor-aging-fixture.json" 25-aging-fixture
wait_log 'DTLOOT aging converted=1 intervals=1$' "$((aging_start + 1))" 90
tap_wait 268 787 'DTLOOT open id=x814_1 stacks=4$' 'DTUI panel=Some\(Scavenge\)$'
indoor_save_checkpoint indoor-food-aged 324 731
aging_assert_save indoor-food-aged initial
capture 26-aged-fridge-portrait 390 844

indoor_restart_from "$OUT_DIR/indoor-food-aged.json" 27-aging-continue
tap_wait 268 787 'DTLOOT open id=x814_1 stacks=4$' 'DTUI panel=Some\(Scavenge\)$'
indoor_save_checkpoint indoor-food-aged-continued 324 731
aging_assert_save indoor-food-aged-continued continued
capture 28-aged-fridge-continued 390 844
tap_wait 332 77 'DTUI panel=None$'
alive
refresh_log
[[ $(log_count 'DTLOOT aging converted=1 intervals=1$') -eq $((aging_start + 1)) ]]
printf '%s\n' 'Indoor Android smoke passed: fixture-based touch pickups, exact saved stock, barricade spend/refund, cold Continue, door opening and persisted blackout aging.'
