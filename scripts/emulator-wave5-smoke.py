#!/usr/bin/env python3
"""Real Android Wave 5 controls, vehicle maintenance/driving and mounted combat.

Runs only on the disposable CI emulator, after the existing audio/performance
checks and before the independent APK replacement check. Controlled scenario
copies are derived from the real prior Android save and the native-validated
vehicle descriptor. Later Continue uses exact bytes written by the running app.
No result in this script is a physical-phone FPS or felt-vibration claim.
"""
from __future__ import annotations

import argparse
import copy
import hashlib
import importlib.util
import json
import math
from pathlib import Path
import re
import subprocess
import time

SCRIPT_DIR = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location('dt_wave5_audio', SCRIPT_DIR / 'emulator-audio-smoke.py')
audio = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(audio)
FIXTURE = SCRIPT_DIR.parent / 'game/tests/fixtures/android-vehicles-smoke.json'
PORTRAIT = (390, 844)
LANDSCAPE = (844, 390)


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def require_emulator() -> None:
    if audio.adb('shell', 'getprop', 'ro.kernel.qemu') != '1':
        raise RuntimeError('Refusing fixture injection: a disposable Android emulator is required')


def json_logs(prefix: str) -> list[dict]:
    return [json.loads(raw) for raw in re.findall(re.escape(prefix) + r'(\{[^\r\n]+\})', audio.logs())]


def wait_json(prefix: str, predicate=lambda _: True, timeout: float = 90) -> dict:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if not audio.adb('shell', 'pidof', '-s', audio.PKG):
            raise AssertionError('Game process exited while waiting for ' + prefix)
        values = json_logs(prefix)
        if values and predicate(values[-1]):
            return values[-1]
        time.sleep(0.5)
    raise TimeoutError('Missing matching actual Android JSON: ' + prefix)


def center(rect: list[float]) -> tuple[int, int]:
    assert len(rect) == 4 and rect[2] > rect[0] and rect[3] > rect[1], rect
    return round((rect[0] + rect[2]) / 2), round((rect[1] + rect[3]) / 2)


def hold(rect: list[float], duration_ms: int) -> None:
    x, y = center(rect)
    audio.adb('shell', 'input', 'swipe', str(x), str(y), str(x), str(y), str(duration_ms))


def rotate(landscape: bool) -> tuple[int, int]:
    audio.adb('shell', 'settings', 'put', 'system', 'user_rotation', '1' if landscape else '0')
    audio.adb('emu', 'sensor', 'set', 'acceleration', '9.8:0:0' if landscape else '0:9.8:0')
    return LANDSCAPE if landscape else PORTRAIT


def panel(view: str, viewport: tuple[int, int] = PORTRAIT) -> dict:
    return wait_json('DTPANEL layout=', lambda value: value['view'] == view and tuple(value['viewport']) == viewport)


def button(action: str, view: str, viewport: tuple[int, int] = PORTRAIT) -> list[float]:
    """Find the rendered semantic action, using real page geometry when needed."""
    seen = set()
    for _ in range(12):
        value = panel(view, viewport)
        for entry in value['buttons']:
            if entry['action'] == action and entry['enabled']:
                return entry['rect']
        pages = [entry for entry in value['buttons']
                 if entry['enabled'] and entry['action'].startswith('Page(')]
        candidates = sorted(pages, key=lambda entry: entry['rect'][0], reverse=True)
        candidate = next((entry for entry in candidates if entry['action'] not in seen), None)
        if candidate is None:
            raise AssertionError(f'No enabled actual panel action {action}: {value}')
        seen.add(candidate['action'])
        count = audio.count(r'DTPANEL layout=') + 1
        audio.tap(candidate['rect'], r'DTUI action=' + re.escape(candidate['action']) + '$')
        audio.wait_log(r'DTPANEL layout=', count)
    raise AssertionError('Too many pages while finding ' + action)


def tap_panel(action: str, view: str, viewport: tuple[int, int] = PORTRAIT) -> None:
    audio.tap(button(action, view, viewport), r'DTUI action=' + re.escape(action) + '$')


def vehicle(save: dict, identity: int) -> dict:
    entries = [entry for entry in save['vehicles']['vehicles'] if entry['id'] == identity]
    assert len(entries) == 1, 'Vehicle state must have one authoritative stable ID'
    return entries[0]


def bag(save: dict) -> dict[str, int]:
    entries = save['inventory']['items']
    assert len(entries) == len({entry[0] for entry in entries})
    assert all(isinstance(count, int) and not isinstance(count, bool) and count >= 0 for _, count in entries)
    return {key: count for key, count in entries if count}


def ammo_total(value: dict) -> list[int]:
    guns = value['gun']['guns']
    assert guns and 0 <= value['gun']['selected'] < len(guns)
    for gun in guns:
        assert all(isinstance(gun[field], int) and not isinstance(gun[field], bool) and gun[field] >= 0
                   for field in ('ready', 'reserve'))
    return [gun['ready'] + gun['reserve'] for gun in guns]


def prepare_scenario(baseline: dict, descriptor: dict, name: str) -> dict:
    assert descriptor['version'] == 1
    case = descriptor[name]
    assert len(case['player']) == 3 and all(math.isfinite(v) for v in case['player'])
    assert case['vehicle_id'] == 50000 and case['fleet']['current'] is None
    value = copy.deepcopy(baseline)
    value['vehicles'] = copy.deepcopy(case['fleet'])
    value['armor'] = copy.deepcopy(case['armor'])
    value['player'].update(position=case['player'], hp=100.0, bleed=False, stun=0.0)
    value['campaign'].update(speed=1, sleeping=False, notice_t=0.0)
    value['needs'].update(hunger=80.0, thirst=80.0, fatigue=0.0)
    value['world']['dead'] = sorted(set(value['world']['dead']) | set(range(64)))
    value['inventory'] = {'items': [], 'equipped': ''}
    if name == 'civilian':
        value['inventory']['items'] = [['gas', 2], ['patch', 1], ['carbat', 1], ['hose', 1]]
        car = vehicle(value, case['vehicle_id'])
        car.update(fuel=4.0, tyre=0.3, battery=0.2)
    return value


def install_exact(raw: bytes, out: Path, name: str) -> None:
    require_emulator()
    audio.adb('shell', 'am', 'force-stop', audio.PKG)
    deadline = time.monotonic() + 20
    while time.monotonic() < deadline:
        process = subprocess.run(['adb', 'shell', 'pidof', '-s', audio.PKG], capture_output=True, timeout=20)
        if not process.stdout.strip():
            break
        time.sleep(0.5)
    else:
        raise AssertionError('Fixture injection requires a stopped app')
    # All shell tokens below are fixed package/save constants, not file content.
    command = (f"run-as {audio.PKG} sh -c 'cat > files/{audio.SAVE}.fixture "
               f"&& mv files/{audio.SAVE}.fixture files/{audio.SAVE}'")
    subprocess.run(['adb', 'shell', '-T', command], input=raw, check=True, timeout=60)
    actual = audio.raw_private(audio.SAVE)
    assert actual == raw, 'Installed private save is not byte-for-byte identical'
    (out / (name + '-installed.json')).write_bytes(actual)


def launch_continue(out: Path, tag: str) -> None:
    rotate(False)
    audio.launch()
    audio.wait_log(r'DTUI flow=Title ', 1, 180)
    audio.wait_log(r'DTPERF fps=', 2, 180)
    # The actual shipped title has this stable portrait Continue center.
    audio.tap([194, 598, 196, 600], r'DTUI action=Continue$')
    audio.wait_log(r'DTUI flow=Playing ', 1, 90)
    panel('None')
    (out / (tag + '-continue-logcat.txt')).write_text(audio.logs())


def checkpoint(out: Path, name: str, predicate=lambda _: True, already_open=False) -> tuple[dict, bytes]:
    if not already_open:
        tap_panel('Open(Inventory)', 'None')
        panel('Some(Inventory)')
    tap_panel('Save', 'Some(Inventory)')
    audio.wait_log(r'DTPERF fps=', audio.count(r'DTPERF fps=') + 1, 90)
    deadline = time.monotonic() + 60
    while time.monotonic() < deadline:
        raw = audio.raw_private(audio.SAVE)
        value = json.loads(raw)
        if predicate(value):
            (out / (name + '.json')).write_bytes(raw)
            return value, raw
        time.sleep(0.5)
    raise AssertionError('Saved runtime state did not satisfy ' + name)


def close_bag(viewport: tuple[int, int] = PORTRAIT) -> None:
    tap_panel('Close', 'Some(Inventory)', viewport)
    panel('None', viewport)


def vehicle_action(name: str, identity: int, action) -> dict:
    pattern = r'DTVEHICLE action=' + name + r' state='
    wanted = audio.count(pattern) + 1
    action()
    audio.wait_log(pattern, wanted)
    return wait_json('DTVEHICLE action=' + name + ' state=', lambda value: value.get('id', identity) == identity)


def use_item(item: str, semantic: str, identity: int) -> dict:
    result = vehicle_action(semantic, identity, lambda: tap_panel(f'Use("{item}")', 'Some(Inventory)'))
    # Consuming a stack can move every following row. Require the new actual
    # panel after the mutation before using another semantic item hit target.
    deadline = time.monotonic() + 90
    event = 'DTVEHICLE action=' + semantic + ' state='
    while time.monotonic() < deadline:
        observed = audio.logs()
        if observed.rfind('DTPANEL layout=') > observed.rfind(event) >= 0:
            panel('Some(Inventory)')
            return result
        time.sleep(.5)
    raise TimeoutError('Inventory did not rerender after vehicle item use: ' + item)


def controls_layout(viewport: tuple[int, int] = PORTRAIT) -> dict:
    return wait_json('DTCONTROLS layout=', lambda value: tuple(value['viewport']) == viewport)


def open_controls() -> tuple[dict, dict]:
    layout = audio.open_settings()
    audio.tap(layout['tabs'][1], r'DTSETTINGS action=page value=Game')
    layout = audio.current_layout(PORTRAIT)
    audio.tap(layout['controls_open'], r'DTSETTINGS action=page value=Controls')
    return layout, controls_layout()


def controls_check(out: Path, baseline_raw: bytes, record: dict) -> dict:
    audio.launch()
    layout, controls = open_controls()
    prefs = audio.preferences()
    if not prefs['controls']['vibration']:
        audio.tap(controls['vibration'], r'DTSETTINGS action=vibration value=true')
    before = audio.count(r'DTHAPTIC.*status=')
    audio.tap(controls['preview'], r'DTSETTINGS action=haptic_preview queued=true enabled=true')
    audio.wait_log(r'DTHAPTIC.*status=(issued|unavailable)', before + 1)
    haptic_lines = [line for line in audio.logs().splitlines() if 'DTHAPTIC' in line and 'status=' in line]
    record['haptic_bridge'] = {'observed_result': haptic_lines[-1], 'native_request_queued': True,
                               'physical_vibration_observed': False}
    audio.tap(controls['vibration'], r'DTSETTINGS action=vibration value=false')
    audio.preferences(lambda value: value['controls']['vibration'] is False)
    before = audio.count(r'DTHAPTIC.*status=')
    audio.tap(controls['preview'], r'DTSETTINGS action=haptic_preview queued=false enabled=false')
    audio.wait_log(r'DTPERF fps=', audio.count(r'DTPERF fps=') + 1)
    assert audio.count(r'DTHAPTIC.*status=') == before, 'Disabled haptics still reached Android'
    record['disabled_haptics_suppress_platform_requests'] = True
    audio.tap(controls['sizes'][0], r'DTSETTINGS action=button_size value=85')
    audio.preferences(lambda value: value['controls']['button_percent'] == 85)
    audio.tap(controls['sizes'][2], r'DTSETTINGS action=button_size value=110')
    audio.preferences(lambda value: value['controls']['button_percent'] == 110)
    audio.screenshot(out, '01-controls-touch-portrait', PORTRAIT)
    audio.tap(controls['sections'][1], r'DTSETTINGS action=controls_section value=Keyboard')
    controls = controls_layout()
    # Default Up=W and Down=S. First prove the real conflict swap; then choose
    # a free key so the persisted control change is unambiguous on cold start.
    audio.tap(controls['bindings'][0], r'DTSETTINGS action=binding_capture value=Some\(Up\)')
    wanted = audio.count(r'DTSETTINGS action=rebind action_id=Up key=KeyS') + 1
    audio.adb('shell', 'input', 'keyevent', 'KEYCODE_S')
    audio.wait_log(r'DTSETTINGS action=rebind action_id=Up key=KeyS', wanted)
    swapped = audio.preferences(lambda value: value['controls']['bindings']['up'] == 'KeyS'
                                and value['controls']['bindings']['down'] == 'KeyW')
    audio.tap(controls['bindings'][0], r'DTSETTINGS action=binding_capture value=Some\(Up\)')
    wanted = audio.count(r'DTSETTINGS action=rebind action_id=Up key=KeyW') + 1
    audio.adb('shell', 'input', 'keyevent', 'KEYCODE_W')
    audio.wait_log(r'DTSETTINGS action=rebind action_id=Up key=KeyW', wanted)
    audio.preferences(lambda value: value['controls']['bindings']['down'] == 'KeyS')
    audio.tap(controls['bindings'][0], r'DTSETTINGS action=binding_capture value=Some\(Up\)')
    wanted = audio.count(r'DTSETTINGS action=rebind action_id=Up key=KeyL') + 1
    audio.adb('shell', 'input', 'keyevent', 'KEYCODE_L')
    audio.wait_log(r'DTSETTINGS action=rebind action_id=Up key=KeyL', wanted)
    expected = audio.preferences(lambda value: value['controls']['bindings']['up'] == 'KeyL')
    audio.tap(controls['bindings'][1], r'DTSETTINGS action=binding_capture value=Some\(Down\)')
    wanted = audio.count(r'DTSETTINGS action=binding_cancel') + 1
    audio.adb('shell', 'input', 'keyevent', 'KEYCODE_ESCAPE')
    audio.wait_log(r'DTSETTINGS action=binding_cancel', wanted)
    assert audio.preferences() == expected
    assert audio.raw_private(audio.SAVE) == baseline_raw, 'Title controls changed the game save'
    (out / 'settings-only-save-readback.json').write_bytes(audio.raw_private(audio.SAVE))
    (out / 'controls-before-restart.json').write_bytes(audio.raw_private(audio.PREFS))
    record['binding_conflict_swap'] = swapped['controls']['bindings']
    record['binding_cancel_preserves_settings'] = True
    audio.screenshot(out, '02-controls-bindings-portrait', PORTRAIT)
    (out / 'controls-before-restart-logcat.txt').write_text(audio.logs())
    audio.adb('shell', 'am', 'force-stop', audio.PKG)
    audio.launch()
    layout, controls = open_controls()
    assert audio.preferences() == expected
    # Changing a loaded value after restart proves that it was applied live,
    # rather than merely leaving an unparsed old JSON file untouched.
    audio.tap(controls['vibration'], r'DTSETTINGS action=vibration value=true')
    audio.preferences(lambda value: value['controls']['vibration'] is True)
    audio.tap(controls['vibration'], r'DTSETTINGS action=vibration value=false')
    audio.preferences(lambda value: value == expected)
    audio.tap(controls['sections'][1], r'DTSETTINGS action=controls_section value=Keyboard')
    viewport = rotate(True)
    controls_layout(viewport)
    audio.current_layout(viewport)
    audio.screenshot(out, '03-controls-bindings-landscape', viewport)
    rotate(False)
    layout = audio.current_layout(PORTRAIT)
    controls_layout()
    assert audio.raw_private(audio.SAVE) == baseline_raw
    (out / 'controls-after-restart.json').write_bytes(audio.raw_private(audio.PREFS))
    record['cold_restart_applies_controls'] = True
    record['settings_preserve_exact_game_save'] = True
    record['final_preferences'] = expected
    audio.tap(layout['close'], r'DTUI action=SettingsClose')
    (out / 'controls-after-restart-logcat.txt').write_text(audio.logs())
    audio.adb('shell', 'am', 'force-stop', audio.PKG)
    return expected


def civilian_check(out: Path, descriptor: dict, prepared: dict, record: dict) -> None:
    case = descriptor['civilian']
    identity = case['vehicle_id']
    raw = (json.dumps(prepared, ensure_ascii=False, separators=(',', ':'), allow_nan=False) + '\n').encode()
    (out / 'civilian-fixture.json').write_bytes(raw)
    install_exact(raw, out, 'civilian-fixture')
    launch_continue(out, 'civilian')
    tap_panel('Open(Inventory)', 'None')
    panel('Some(Inventory)')
    patch = use_item('patch', 'repair', identity)
    assert patch['tyre'] == 1 and patch['patch_items'] == 0 and patch['changed']
    battery = use_item('carbat', 'repair', identity)
    assert battery['battery'] == 1 and battery['battery_items'] == 0 and battery['changed']
    fueled = use_item('gas', 'refuel', identity)
    assert fueled['fuel'] == 6 and fueled['gas_items'] == 0 and fueled['changed']
    siphoned = use_item('hose', 'siphon', identity)
    assert siphoned['fuel'] == 1 and siphoned['gas_items'] == 5 and siphoned['changed']
    returned = use_item('gas', 'refuel', identity)
    assert returned['fuel'] == 6 and returned['gas_items'] == 0
    maintained, _ = checkpoint(out, 'civilian-maintained',
        lambda save: vehicle(save, identity)['fuel'] == 6 and bag(save) == {'hose': 1}, already_open=True)
    assert vehicle(maintained, identity)['tyre'] == vehicle(maintained, identity)['battery'] == 1
    close_bag()
    entered = vehicle_action('enter', identity, lambda: tap_panel('Interact', 'None'))
    assert entered['current'] == identity
    pad = wait_json('DTPAD layout=', lambda value: value['driving'] and not value['armored'])
    assert pad['button_percent'] == 110
    audio.screenshot(out, '04-civilian-driving-portrait', PORTRAIT)
    cx, cy = center(pad['joy'])
    right, up = case['drive_screen']
    end = round(cx + right * 54), round(cy - up * 54)
    audio.adb('shell', 'input', 'swipe', str(cx), str(cy), str(end[0]), str(end[1]), '6000')
    hold(pad['sec'], 4000)
    moved, _ = checkpoint(out, 'civilian-driven',
        lambda save: save['vehicles']['current'] == identity and
        math.dist(vehicle(save, identity)['pos'], vehicle(maintained, identity)['pos']) > 0.25)
    car = vehicle(moved, identity)
    assert abs(car['speed']) < 0.05, 'Held brake did not stop the actual vehicle'
    assert 0 <= car['fuel'] < 6, 'Actual driving must consume fuel'
    assert car['engine'] == 1 and car['tyre'] == 1, 'The clear-path input fixture collided'
    assert math.dist([moved['player']['position'][0], moved['player']['position'][2]], car['pos']) < 1e-4
    close_bag()
    viewport = rotate(True)
    wait_json('DTPAD layout=', lambda value: tuple(value['viewport']) == viewport and value['driving'])
    panel('None', viewport)
    audio.screenshot(out, '05-civilian-driving-landscape', viewport)
    vehicle_action('exit', identity, lambda: tap_panel('Interact', 'None', viewport))
    wait_json('DTPAD layout=', lambda value: tuple(value['viewport']) == viewport and not value['driving'])
    audio.screenshot(out, '06-civilian-exit-landscape', viewport)
    rotate(False)
    panel('None')
    exited, saved_raw = checkpoint(out, 'civilian-exited', lambda save: save['vehicles']['current'] is None)
    saved_car = vehicle(exited, identity)
    assert math.dist([exited['player']['position'][0], exited['player']['position'][2]], saved_car['pos']) > 1.0
    (out / 'civilian-before-continue-logcat.txt').write_text(audio.logs())
    install_exact(saved_raw, out, 'civilian-real-checkpoint')
    launch_continue(out, 'civilian-real-checkpoint')
    restored, _ = checkpoint(out, 'civilian-after-continue')
    assert vehicle(restored, identity) == saved_car, 'Cold Continue changed an unoccupied vehicle'
    assert bag(restored) == {'hose': 1}
    assert restored['vehicles']['current'] is None
    record['civilian'] = {'vehicle_id': identity, 'maintenance': [patch, battery, fueled, siphoned, returned],
                         'distance_m': math.dist(car['pos'], vehicle(maintained, identity)['pos']),
                         'held_brake_speed': car['speed'], 'fuel_after_drive': car['fuel'],
                         'cold_continue_exact_vehicle_state': True, 'checkpoint_sha256': digest(saved_raw)}
    (out / 'civilian-after-continue-logcat.txt').write_text(audio.logs())
    audio.adb('shell', 'am', 'force-stop', audio.PKG)


def armored_check(out: Path, descriptor: dict, prepared: dict, record: dict) -> None:
    case = descriptor['armored']
    identity = case['vehicle_id']
    raw = (json.dumps(prepared, ensure_ascii=False, separators=(',', ':'), allow_nan=False) + '\n').encode()
    (out / 'armored-fixture.json').write_bytes(raw)
    install_exact(raw, out, 'armored-fixture')
    launch_continue(out, 'armored')
    entered = vehicle_action('enter', identity, lambda: tap_panel('Interact', 'None'))
    assert entered['current'] == identity
    pad = wait_json('DTPAD layout=', lambda value: value['driving'] and value['armored'])
    hud = wait_json('DTVEHICLEUI layout=', lambda value: value['id'] == identity and tuple(value['viewport']) == PORTRAIT)
    before_ammo = ammo_total(vehicle(prepared, identity))
    # A short tap must traverse the real touch -> WeaponInput -> mounted-fire
    # route; no debug fire action or injected post-shot state is used.
    audio.tap(pad['act'])
    audio.wait_log(r'DTPERF fps=', audio.count(r'DTPERF fps=') + 1)
    fired, _ = checkpoint(out, 'armored-fired', lambda save:
        ammo_total(vehicle(save, identity))[0] == before_ammo[0] - 1)
    target = [unit for unit in fired['armor']['units'] if unit['id'] == case['target_id']]
    assert len(target) == 1 and target[0]['hp'] < 100, 'Actual mounted shot did not damage the live armored target'
    assert fired['player']['hp'] > 0
    close_bag()
    hud = wait_json('DTVEHICLEUI layout=', lambda value: value['id'] == identity and tuple(value['viewport']) == PORTRAIT)
    wanted = audio.count(r'DTVEHICLE action=reload state=') + 1
    completed_count = audio.count(r'DTVEHICLE action=reload_complete state=') + 1
    audio.tap(hud['buttons'][0])
    audio.wait_log(r'DTVEHICLE action=reload state=', wanted)
    reloading = wait_json('DTVEHICLE action=reload state=')
    assert reloading['guns'][0]['reload_remaining'] > 0 and reloading['guns'][0]['ready'] == 0
    assert reloading['guns'][0]['reserve'] == before_ammo[0] - 1
    audio.wait_log(r'DTVEHICLE action=reload_complete state=', completed_count, 180)
    completed = wait_json('DTVEHICLE action=reload_complete state=', lambda value: value['vehicle_id'] == identity)
    expected_ready = vehicle(prepared, identity)['gun']['guns'][0]['ready']
    expected_reserve = before_ammo[0] - 1 - expected_ready
    assert completed['guns'][0]['reload_remaining'] == 0
    assert completed['guns'][0]['ready'] == expected_ready
    assert completed['guns'][0]['reserve'] == expected_reserve
    reloaded, _ = checkpoint(out, 'armored-reload-completed', lambda save:
        vehicle(save, identity)['gun']['guns'][0]['reload_remaining'] == 0
        and vehicle(save, identity)['gun']['guns'][0]['ready'] == expected_ready
        and vehicle(save, identity)['gun']['guns'][0]['reserve'] == expected_reserve)
    assert ammo_total(vehicle(reloaded, identity)) == [before_ammo[0] - 1, before_ammo[1]]
    close_bag()
    hud = wait_json('DTVEHICLEUI layout=', lambda value: value['id'] == identity and tuple(value['viewport']) == PORTRAIT)
    wanted = audio.count(r'DTVEHICLE action=switch state=') + 1
    audio.tap(hud['buttons'][1])
    audio.wait_log(r'DTVEHICLE action=switch state=', wanted)
    switched = wait_json('DTVEHICLE action=switch state=')
    assert switched['selected'] == 1
    audio.screenshot(out, '07-mounted-weapon-portrait', PORTRAIT)
    saved, saved_raw = checkpoint(out, 'armored-real-checkpoint', lambda save:
        save['vehicles']['current'] == identity and vehicle(save, identity)['gun']['selected'] == 1)
    total = ammo_total(vehicle(saved, identity))
    assert total == [before_ammo[0] - 1, before_ammo[1]], 'Reload/switch created or lost ammunition'
    (out / 'armored-before-continue-logcat.txt').write_text(audio.logs())
    install_exact(saved_raw, out, 'armored-real-checkpoint')
    launch_continue(out, 'armored-real-checkpoint')
    wait_json('DTPAD layout=', lambda value: value['driving'] and value['armored'])
    viewport = rotate(True)
    wait_json('DTPAD layout=', lambda value: value['driving'] and tuple(value['viewport']) == viewport)
    audio.screenshot(out, '08-mounted-weapon-continued-landscape', viewport)
    rotate(False)
    panel('None')
    restored, restored_raw = checkpoint(out, 'armored-after-continue', lambda save:
        save['vehicles']['current'] == identity and vehicle(save, identity)['gun']['selected'] == 1)
    assert ammo_total(vehicle(restored, identity)) == total
    assert abs(vehicle(restored, identity)['speed']) < 1e-6
    assert math.dist(vehicle(restored, identity)['pos'], vehicle(saved, identity)['pos']) < 1e-6
    assert restored['player']['hp'] > 0
    record['armored'] = {'vehicle_id': identity, 'target_id': case['target_id'],
                        'target_hp_after_shot': target[0]['hp'], 'reloading_state': reloading,
                        'reload_completed_state': completed,
                        'switched_state': switched, 'saved_ammunition': total,
                        'cold_continue_preserves_ammunition_and_selection': True,
                        'checkpoint_sha256': digest(saved_raw), 'final_checkpoint_sha256': digest(restored_raw)}
    record['final_save_for_apk_update_sha256'] = digest(restored_raw)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out', required=True, type=Path)
    args = parser.parse_args()
    require_emulator()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    audio.adb('shell', 'am', 'force-stop', audio.PKG)
    baseline_raw = audio.raw_private(audio.SAVE)
    baseline = json.loads(baseline_raw)
    baseline_prefs = audio.raw_private(audio.PREFS)
    descriptor = json.loads(FIXTURE.read_text())
    (out / 'baseline-save.json').write_bytes(baseline_raw)
    (out / 'baseline-preferences.json').write_bytes(baseline_prefs)
    (out / 'native-vehicle-descriptor.json').write_bytes(FIXTURE.read_bytes())
    record = {'success': False, 'source': 'Actual Android emulator touches and saved runtime checkpoints',
              'baseline_save_sha256': digest(baseline_raw), 'descriptor_sha256': digest(FIXTURE.read_bytes()),
              'fixture_changes': ['vehicles and armor from native-validated clear-road descriptor',
                                  'player location/HP/bleed/stun; inventory limited to scenario items',
                                  'speed=1, sleeping=false, old notice hidden, needs restored to 80/80/0',
                                  'ordinary patrol IDs0..63 marked dead; the armored target remains active',
                                  'civilian fuel4L, tire0.3, battery0.2 to exercise actual repair tools'],
              'not_tested': ['Physical phone frame rate, temperatures or battery drain',
                             'Whether vibration was physically felt', 'Walking from home to the controlled scenario',
                             'Vehicle lockpicking via Android in this particular smoke; native QA covers that path']}
    try:
        audio.adb('shell', 'wm', 'size', '390x844')
        audio.adb('shell', 'wm', 'density', '160')
        audio.adb('shell', 'settings', 'put', 'system', 'accelerometer_rotation', '1')
        rotate(False)
        expected = controls_check(out, baseline_raw, record)
        civilian_check(out, descriptor, prepare_scenario(baseline, descriptor, 'civilian'), record)
        armored_check(out, descriptor, prepare_scenario(baseline, descriptor, 'armored'), record)
        assert audio.preferences() == expected
        record['success'] = True
        print(json.dumps({'checkpoint': 'android-wave5', 'success': True,
                          'civilian_distance_m': record['civilian']['distance_m'],
                          'armored_target_hp': record['armored']['target_hp_after_shot']}), flush=True)
    except Exception as error:
        record['error'] = str(error)
        raise
    finally:
        for name, read in [('logcat.txt', lambda: audio.adb('logcat', '-d', binary=True)),
                           ('final.png', lambda: audio.adb('exec-out', 'screencap', '-p', binary=True)),
                           ('final-save.json', lambda: audio.raw_private(audio.SAVE)),
                           ('final-preferences.json', lambda: audio.raw_private(audio.PREFS))]:
            try:
                (out / name).write_bytes(read())
            except (OSError, subprocess.SubprocessError) as error:
                record.setdefault('diagnostic_errors', {})[name] = str(error)
        (out / 'wave5-smoke-report.json').write_text(json.dumps(record, ensure_ascii=False, indent=2, allow_nan=False) + '\n')
        audio.adb('shell', 'am', 'force-stop', audio.PKG)
        audio.adb('shell', 'wm', 'size', 'reset')
        audio.adb('shell', 'wm', 'density', 'reset')
        rotate(False)


if __name__ == '__main__':
    main()
