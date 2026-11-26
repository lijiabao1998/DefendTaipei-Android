#!/usr/bin/env python3
"""Real settings taps, captured Android audio, restart and replacement checks.

Requires a disposable emulator already started with audio enabled and
PULSE_SINK=defendtaipei_android. It never runs on a physical phone. Existing
gameplay-smoke save bytes must survive every title/preferences action.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import struct
import subprocess
import time

from audio_capture import AudioCapture

PKG = 'tw.lijiabao.defendtaipei'
ACTIVITY = PKG + '/.MainActivity'
SAVE = 'defendtaipei-native-v1.json'
PREFS = 'settings.json'


def adb(*args: str, binary: bool = False):
    data = subprocess.check_output(['adb', *args], timeout=60)
    return data if binary else data.decode(errors='replace').strip()


def logs() -> str:
    value = adb('logcat', '-d')
    if re.search(r'panicked at|FATAL EXCEPTION|Fatal signal|error\[B000', value):
        raise AssertionError('Runtime failure in Android logcat')
    return value


def count(pattern: str) -> int:
    return len(re.findall(pattern, logs(), re.MULTILINE))


def wait_log(pattern: str, wanted: int = 1, timeout: int = 90) -> None:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if not adb('shell', 'pidof', '-s', PKG):
            raise AssertionError('Android game process exited')
        if count(pattern) >= wanted:
            return
        time.sleep(0.5)
    raise TimeoutError(f'Missing actual Android action: {pattern}, wanted {wanted}')


def current_layout(viewport: tuple[int, int] | None = None) -> dict:
    deadline = time.monotonic() + 90
    while time.monotonic() < deadline:
        found = re.findall(r'DTSETTINGS layout=(\{[^\r\n]+\})', logs())
        if found:
            layout = json.loads(found[-1])
            if viewport is None or tuple(layout['viewport']) == viewport:
                return layout
        time.sleep(0.5)
    raise TimeoutError(f'Settings did not render at {viewport}')


def tap(rect: list[float], pattern: str | None = None) -> None:
    wanted = count(pattern) + 1 if pattern else 0
    x, y = round((rect[0] + rect[2]) / 2), round((rect[1] + rect[3]) / 2)
    # An ordinary short touch exercises the shipped ordered touch handler.
    adb('shell', 'input', 'tap', str(x), str(y))
    if pattern:
        wait_log(pattern, wanted)
    else:
        time.sleep(0.8)


def raw_private(name: str) -> bytes:
    return adb('exec-out', 'run-as', PKG, 'cat', 'files/' + name, binary=True)


def preferences(predicate=lambda _: True) -> dict:
    deadline = time.monotonic() + 45
    while time.monotonic() < deadline:
        try:
            value = json.loads(raw_private(PREFS))
            if predicate(value):
                return value
        except (subprocess.CalledProcessError, json.JSONDecodeError):
            pass
        time.sleep(0.5)
    raise TimeoutError('Actual private preferences did not reach the expected state')


def slider(layout: dict, index: int, value: float) -> None:
    rect = layout['sliders'][index]
    x0, y = round((rect[0] + rect[2]) / 2), round((rect[1] + rect[3]) / 2)
    x1 = round(rect[0] - 12 if value == 0 else rect[0] + (rect[2] - rect[0]) * value)
    # Capture persists while dragging beyond the slider boundary, clamping to0.
    adb('shell', 'input', 'swipe', str(x0), str(y), str(x1), str(y), '600')
    field = ['master', 'effects', 'ambience', 'radio'][index]
    preferences(lambda p: abs(p['audio'][field] - value) < 0.012)


def open_settings() -> dict:
    wait_log(r'DTPERF fps=', 1, 180)
    wait_log(r'DTUI flow=Title ', 1)
    wanted = count(r'DTUI action=SettingsOpen') + 1
    # Stable Home layout at390x844/mdpi; the overlay reports its own rectangles.
    adb('shell', 'input', 'tap', '195', '730')
    wait_log(r'DTUI action=SettingsOpen', wanted)
    return current_layout((390, 844))


def launch() -> None:
    adb('logcat', '-c')
    adb('shell', 'am', 'start', '-W', '-n', ACTIVITY)
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline:
        try:
            if adb('shell', 'pidof', '-s', PKG):
                return
        except subprocess.CalledProcessError:
            pass
        time.sleep(0.5)
    raise TimeoutError('Android process failed to start')


def screenshot(out: Path, name: str, viewport: tuple[int, int]) -> None:
    wait_log(r'DTPERF fps=', count(r'DTPERF fps=') + 2, 180)
    data = adb('exec-out', 'screencap', '-p', binary=True)
    assert data[:8] == b'\x89PNG\r\n\x1a\n'
    assert struct.unpack('>II', data[16:24]) == viewport
    (out / (name + '.png')).write_bytes(data)


def audition(capture: AudioCapture, layout: dict, name: str, category: int,
             audible: bool, report: dict) -> None:
    time.sleep(1.5)  # Allow any preceding click/preview tail to finish.
    capture.start(name)
    category_name = ['Effects', 'Ambience', 'Radio'][category - 1]
    for _ in range(2):
        tap(layout['audio_actions'][category], r'DTSETTINGS action=preview category=' + category_name)
        time.sleep(1.0)
    time.sleep(2.0)
    pcm = capture.stop(expect_audible=audible)
    report['audio'].append({'name': name, 'category': category_name, 'expect_audible': audible, 'pcm': pcm})
    print(json.dumps({'checkpoint': name, 'pcm': pcm}), flush=True)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--apk', required=True, type=Path)
    parser.add_argument('--out', required=True, type=Path)
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    assert adb('shell', 'getprop', 'ro.kernel.qemu').strip() == '1', 'Disposable emulator required'
    assert os.environ.get('PULSE_SINK') == 'defendtaipei_android', 'Dedicated emulator audio sink required'
    # Previous gameplay smoke may leave a live session with autosave enabled.
    adb('shell', 'am', 'force-stop', PKG)
    baseline = raw_private(SAVE)
    baseline_save = json.loads(baseline)
    report = {'success': False, 'audio': [], 'source': 'Actual Android emulator audio output after real UI touches',
              'baseline_game_sha256': hashlib.sha256(baseline).hexdigest(),
              'baseline_day_hours': baseline_save['campaign']['day_hours'],
              'not_tested': ['Physical phone speakers, latency or Bluetooth', 'Voice synthesis',
                             'Cross-signer preference migration', 'Long-term device performance']}
    try:
        adb('shell', 'wm', 'size', '390x844')
        adb('shell', 'wm', 'density', '160')
        adb('shell', 'settings', 'put', 'system', 'accelerometer_rotation', '1')
        adb('shell', 'settings', 'put', 'system', 'user_rotation', '0')
        adb('emu', 'sensor', 'set', 'acceleration', '0:9.8:0')
        adb('shell', 'settings', 'put', 'system', 'sound_effects_enabled', '0')
        adb('shell', 'cmd', 'media_session', 'volume', '--stream', '3', '--set', '15')
        launch()
        layout = open_settings()
        tap(layout['tabs'][0], r'DTSETTINGS action=page value=Audio')
        # Actual Reset control creates known defaults without injecting settings.
        tap(layout['reset'])
        slider(layout, 0, 0.6)
        slider(layout, 0, 0.8)
        with AudioCapture(out, existing_sink='defendtaipei_android') as capture:
            audition(capture, layout, '01-effects', 1, True, report)
            tap(layout['audio_actions'][0], r'DTSETTINGS action=mute value=true')
            preferences(lambda p: p['audio']['muted'])
            audition(capture, layout, '02-muted', 1, False, report)
            tap(layout['audio_actions'][0], r'DTSETTINGS action=mute value=false')
            slider(layout, 1, 0.0)
            audition(capture, layout, '03-effects-zero', 1, False, report)
            audition(capture, layout, '04-independent-ambience', 2, True, report)
            audition(capture, layout, '05-radio', 3, True, report)
            slider(layout, 3, 0.0)
            audition(capture, layout, '06-radio-zero', 3, False, report)
            slider(layout, 0, 0.0)
            audition(capture, layout, '07-master-zero', 2, False, report)
            slider(layout, 0, 0.8)
            slider(layout, 1, 0.65)
            slider(layout, 2, 0.4)
            slider(layout, 3, 0.5)
            tap(layout['audio_actions'][0], r'DTSETTINGS action=mute value=true')
            tap(layout['tabs'][1], r'DTSETTINGS action=page value=Game')
            tap(layout['durations'][2], r'DTSETTINGS action=day_hours value=4')
            tap(layout['languages'][1])
            prefs = preferences(lambda p: p['day_hours'] == 4 and p['language'] == 'en' and p['audio']['muted'])
            (out / 'preferences-before-restart.json').write_text(json.dumps(prefs, indent=2))
            screenshot(out, '08-game-preferences-portrait', (390, 844))
            assert raw_private(SAVE) == baseline, 'Settings changed the saved game or current day length'
            (out / 'before-restart-logcat.txt').write_text(logs())
            adb('shell', 'am', 'force-stop', PKG)
            launch()
            layout = open_settings()
            assert preferences() == prefs
            tap(layout['tabs'][0], r'DTSETTINGS action=page value=Audio')
            audition(capture, layout, '09-restart-muted', 1, False, report)
            tap(layout['audio_actions'][0], r'DTSETTINGS action=mute value=false')
            # An actual runtime mutation must rewrite every restored field.
            # Reading unchanged bytes alone cannot prove the loader applied them.
            expected_restored = {**prefs, 'audio': {**prefs['audio'], 'muted': False}}
            restored = preferences(lambda p: p == expected_restored)
            (out / 'preferences-restored-runtime.json').write_text(json.dumps(restored, indent=2))
            report['restart_applies_all_preferences'] = True
            audition(capture, layout, '10-restart-unmuted', 1, True, report)
            screenshot(out, '11-audio-preferences-portrait', (390, 844))
            adb('shell', 'settings', 'put', 'system', 'user_rotation', '1')
            adb('emu', 'sensor', 'set', 'acceleration', '9.8:0:0')
            layout = current_layout((844, 390))
            screenshot(out, '12-audio-preferences-landscape', (844, 390))
            tap(layout['tabs'][1], r'DTSETTINGS action=page value=Game')
            screenshot(out, '13-game-preferences-landscape', (844, 390))
            report['preferences'] = preferences(lambda p: p == expected_restored)
            before_update = raw_private(PREFS)
            (out / 'before-update-logcat.txt').write_text(logs())
            adb('shell', 'am', 'force-stop', PKG)
            adb('install', '--no-incremental', '-r', '-t', str(args.apk.resolve()))
            assert raw_private(PREFS) == before_update, 'Normal replacement lost preferences'
            assert raw_private(SAVE) == baseline, 'Normal replacement changed the game save'
            report['same_signer_update_preserves_preferences'] = True
            report['preferences_sha256'] = hashlib.sha256(before_update).hexdigest()
            report['saved_game_unchanged'] = True
        report['success'] = True
    except Exception as error:
        report['error'] = str(error)
        raise
    finally:
        (out / 'audio-settings-report.json').write_text(json.dumps(report, indent=2) + '\n')
        try:
            (out / 'logcat.txt').write_text(adb('logcat', '-d'))
            (out / 'final.png').write_bytes(adb('exec-out', 'screencap', '-p', binary=True))
        finally:
            adb('shell', 'wm', 'size', 'reset')
            adb('shell', 'wm', 'density', 'reset')
            adb('shell', 'settings', 'put', 'system', 'user_rotation', '0')
            adb('emu', 'sensor', 'set', 'acceleration', '0:9.8:0')


if __name__ == '__main__':
    main()
