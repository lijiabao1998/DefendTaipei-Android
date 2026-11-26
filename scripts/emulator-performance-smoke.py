#!/usr/bin/env python3
"""Real Android performance controls, live sampling and a share-chooser check.

Run after audio smoke and before update smoke on the disposable emulator. The
existing saved campaign is resumed, not replaced with invented benchmark data.
The measured app-frame intervals belong to software-emulator gameplay; they
are neither physical-phone FPS claims nor GPU/presentation timings.
"""
from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import math
from pathlib import Path
import re
import subprocess
import time


SCRIPT_DIR = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location('dt_audio_smoke', SCRIPT_DIR / 'emulator-audio-smoke.py')
audio = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(audio)
REPORT = 'perf-report.json'
EXPECTED_PREFERENCES = {'profile': 'balanced', 'frame_cap': 30, 'render_scale': 70, 'overlay': True}
SHARE_URI = re.compile(r'content://tw\.lijiabao\.defendtaipei\.performance_reports/'
                       r'performance_reports/perf-report-[0-9]+\.json')


def finite(value, *, positive=False) -> float:
    if isinstance(value, bool) or not isinstance(value, (int, float)) or not math.isfinite(value):
        raise AssertionError(f'Expected a finite measured number, got {value!r}')
    if (positive and value <= 0) or (not positive and value < 0):
        raise AssertionError(f'Expected a {"positive" if positive else "nonnegative"} number, got {value!r}')
    return float(value)


def validate_dimensions(value: dict, viewport: tuple[int, int], scale: int) -> None:
    assert value['ui'] == list(viewport), f'UI must keep the physical window resolution: {value}'
    assert value['scale'] == scale
    scene = value['scene']
    assert len(scene) == 2
    for actual, native in zip(scene, viewport):
        assert isinstance(actual, int) and not isinstance(actual, bool) and actual > 0
        assert abs(actual - native * scale / 100) <= 2, f'Unexpected scene resolution: {value}'


def validate_device(value: dict) -> None:
    assert value['schema_version'] == 1 and value['platform'] == 'android'
    assert value['reported'] is True and finite(value['sample_age_secs']) <= 10
    assert isinstance(value['sdk_int'], int) and value['sdk_int'] >= 31
    assert value['model'] and value['manufacturer']
    assert finite(value['total_memory_bytes'], positive=True) >= finite(value['available_memory_bytes'])
    assert finite(value['current_refresh_hz'], positive=True) <= 1000
    assert value['supported_refresh_hz']
    assert all(0 < finite(rate) <= 1000 for rate in value['supported_refresh_hz'])
    if value['process_pss_kb'] is not None:
        finite(value['process_pss_kb'], positive=True)
    if value['thermal_status'] is not None:
        assert isinstance(value['thermal_status'], int) and 0 <= value['thermal_status'] <= 6
    if value['thermal_headroom'] is not None:
        finite(value['thermal_headroom'])
    assert 0 <= value['media_volume'] <= value['media_max_volume']


def validate_live_report(value: dict, viewport: tuple[int, int]) -> None:
    assert value['schema_version'] == 1
    assert value['measurement'] == 'app_frame_interval_not_gpu_or_present_time'
    assert value['source'] == 'live_session', 'A fixed timestep QA fixture is not a live measurement'
    assert isinstance(value['build_revision'], str) and value['build_revision']
    duration = finite(value['duration_seconds'], positive=True)
    # The last discarded warmup interval may cross the five-second boundary.
    assert duration >= 60 and finite(value['warmup_seconds']) >= 5
    frames, summary = value['frames'], value['summary']
    assert 10 <= len(frames) <= 20_000, 'Need actual samples, without an emulator FPS performance claim'
    assert summary['samples'] == len(frames)
    measured = [finite(frame['ms'], positive=True) for frame in frames]
    assert math.isclose(sum(measured) / 1000, duration, rel_tol=1e-5, abs_tol=0.001)
    mean = finite(summary['mean_ms'], positive=True)
    assert math.isclose(mean, sum(measured) / len(measured), rel_tol=1e-5, abs_tol=0.001)
    assert math.isclose(finite(summary['fps'], positive=True), 1000 / mean, rel_tol=1e-5, abs_tol=0.001)
    quantiles = [finite(summary[key], positive=True) for key in ('p50_ms', 'p95_ms', 'p99_ms', 'max_ms')]
    assert quantiles == sorted(quantiles)
    assert math.isclose(quantiles[-1], max(measured), rel_tol=1e-5, abs_tol=0.001)
    over_budget = summary['over_budget_frames']
    assert isinstance(over_budget, int) and not isinstance(over_budget, bool) and 0 <= over_budget <= len(frames)
    assert math.isclose(finite(summary['over_budget_ratio']), over_budget / len(frames), abs_tol=1e-5)
    assert all(frame['target_fps'] == 30 and frame['render_scale'] == 70 for frame in frames)
    for edge in ('start', 'end'):
        validate_device(value['device_' + edge])
        validate_dimensions(value['render_dimensions_' + edge], viewport, 70)
        assert value['effective_' + edge]['target_fps'] == 30
        assert value['effective_' + edge]['render_scale'] == 70


def wait_json_log(prefix: str, predicate, timeout: float = 90) -> dict:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        audio.adb('shell', 'pidof', '-s', audio.PKG)
        found = re.findall(re.escape(prefix) + r'(\{[^\r\n]+\})', audio.logs())
        if found:
            value = json.loads(found[-1])
            if predicate(value):
                return value
        time.sleep(0.5)
    raise TimeoutError('Missing fresh matching runtime diagnostic: ' + prefix)


def wait_resolution(viewport: tuple[int, int], scale: int = 70) -> dict:
    deadline = time.monotonic() + 90
    while time.monotonic() < deadline:
        found = re.findall(r'DTRESOLUTION ui=(\d+)x(\d+) scene=(\d+)x(\d+) scale=(\d+)', audio.logs())
        if found:
            w, h, scene_w, scene_h, actual_scale = map(int, found[-1])
            if (w, h, actual_scale) == (*viewport, scale):
                dimensions = {'ui': [w, h], 'scene': [scene_w, scene_h], 'scale': actual_scale}
                validate_dimensions(dimensions, viewport, scale)
                return dimensions
        time.sleep(0.5)
    raise TimeoutError(f'Native UI / scaled scene resolution did not reach {viewport} at {scale}%')


def tap_point(x: int, y: int, pattern: str) -> None:
    audio.tap([x, y, x, y], pattern)


def open_performance(layout: dict, section: int = 0) -> dict:
    audio.tap(layout['tabs'][2], r'DTSETTINGS action=page value=Performance')
    layout = audio.current_layout()
    audio.tap(layout['performance_sections'][section],
              r'DTSETTINGS action=performance_section value=' + ['Display', 'Diagnostics'][section])
    return audio.current_layout()


def rotate(landscape: bool) -> tuple[int, int]:
    audio.adb('shell', 'settings', 'put', 'system', 'user_rotation', '1' if landscape else '0')
    audio.adb('emu', 'sensor', 'set', 'acceleration', '9.8:0:0' if landscape else '0:9.8:0')
    return (844, 390) if landscape else (390, 844)


def pause_settings() -> dict:
    tap_point(340, 787, r'DTUI pause=true')
    tap_point(330, 314, r'DTUI action=SettingsOpen')
    return audio.current_layout((390, 844))


def close_and_resume(layout: dict) -> None:
    audio.tap(layout['close'], r'DTUI action=SettingsClose')
    # The close press is intentionally consumed until the following update.
    audio.wait_log(r'DTPERF fps=', audio.count(r'DTPERF fps=') + 1, 90)
    tap_point(195, 380, r'DTUI pause=false')


def verify_share_chooser(out: Path, layout: dict) -> dict:
    wanted = audio.count(r'report_share=chooser_opened ') + 1
    audio.tap(layout['export'], r'DTSETTINGS action=export_report')
    audio.wait_log(r'report_share=chooser_opened ', wanted)
    share_lines = [line for line in audio.logs().splitlines() if 'report_share=chooser_opened ' in line]
    line = share_lines[-1]
    uri = SHARE_URI.search(line)
    assert uri is not None and 'grant_read=1' in line, 'Expected only a granted performance-report URI'
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline:
        activities = audio.adb('shell', 'dumpsys', 'activity', 'activities')
        if 'ChooserActivity' in activities and 'android.intent.action.CHOOSER' in activities:
            break
        time.sleep(0.5)
    else:
        raise AssertionError('Android share chooser did not become visible')
    (out / 'share-activity.txt').write_text(activities)
    (out / 'share-uri-permissions.txt').write_text(audio.adb('shell', 'dumpsys', 'activity', 'uri-permissions'))
    (out / '05-share-chooser.png').write_bytes(audio.adb('exec-out', 'screencap', '-p', binary=True))
    audio.adb('shell', 'input', 'keyevent', 'KEYCODE_BACK')
    # No recipient is selected and ACTION_SEND is never handed to a recipient.
    return {'chooser_opened': True, 'uri': uri.group(), 'grant_read_requested': True,
            'recipient_selected': False, 'sent': False}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out', required=True, type=Path)
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    assert audio.adb('shell', 'getprop', 'ro.kernel.qemu') == '1', 'Disposable emulator required'
    audio.adb('shell', 'am', 'force-stop', audio.PKG)
    baseline_save = audio.raw_private(audio.SAVE)
    baseline_settings = audio.preferences()
    expected = {**baseline_settings, 'performance': EXPECTED_PREFERENCES}
    record = {'success': False, 'source': 'Actual Android emulator touches, runtime observations and live app-frame samples',
              'baseline_save_sha256': hashlib.sha256(baseline_save).hexdigest(),
              'baseline_preferences': baseline_settings,
              'not_tested': ['Physical-phone FPS, GPU timing, actual display-presentation timestamps',
                             'Physical heat or battery drain', 'Recipient app reading a shared report']}
    thermal_overridden = False
    try:
        audio.adb('shell', 'wm', 'size', '390x844')
        audio.adb('shell', 'wm', 'density', '160')
        audio.adb('shell', 'settings', 'put', 'system', 'accelerometer_rotation', '1')
        rotate(False)
        audio.launch()
        layout = open_performance(audio.open_settings())
        audio.tap(layout['profiles'][2], r'DTSETTINGS action=performance_profile value=Balanced')
        audio.tap(layout['frame_caps'][0], r'DTSETTINGS action=frame_cap value=30')
        audio.tap(layout['render_scales'][1], r'DTSETTINGS action=render_scale value=70')
        if not audio.preferences()['performance']['overlay']:
            audio.tap(layout['performance_sections'][1], r'DTSETTINGS action=performance_section value=Diagnostics')
            audio.tap(layout['performance_overlay'], r'DTSETTINGS action=performance_overlay value=true')
            audio.tap(layout['performance_sections'][0], r'DTSETTINGS action=performance_section value=Display')
        configured = audio.preferences(lambda prefs: prefs == expected)
        assert audio.raw_private(audio.SAVE) == baseline_save, 'Title performance settings changed the campaign'
        record['settings_only_preserves_exact_save'] = True
        record['configured_preferences'] = configured
        record['portrait_dimensions'] = wait_resolution((390, 844))
        audio.screenshot(out, '01-performance-display-portrait', (390, 844))
        audio.tap(layout['close'], r'DTUI action=SettingsClose')
        (out / 'before-restart-logcat.txt').write_text(audio.logs())
        audio.adb('shell', 'am', 'force-stop', audio.PKG)
        audio.launch()
        layout = open_performance(audio.open_settings(), 1)
        assert audio.preferences() == expected
        record['restored_effective'] = wait_json_log('DTPERFORMANCE effective=',
            lambda value: value['target_fps'] == 30 and value['render_scale'] == 70)
        # A real mutation after cold load proves the runtime reapplies every
        # field, rather than merely leaving an unread settings file intact.
        audio.tap(layout['performance_overlay'], r'DTSETTINGS action=performance_overlay value=false')
        audio.preferences(lambda prefs: prefs == {**expected, 'performance': {**EXPECTED_PREFERENCES, 'overlay': False}})
        audio.tap(layout['performance_overlay'], r'DTSETTINGS action=performance_overlay value=true')
        audio.preferences(lambda prefs: prefs == expected)
        assert audio.raw_private(audio.SAVE) == baseline_save
        record['cold_restart_applies_complete_preferences'] = True
        audio.tap(layout['close'], r'DTUI action=SettingsClose')
        audio.wait_log(r'DTPERF fps=', audio.count(r'DTPERF fps=') + 1, 90)
        tap_point(195, 599, r'DTUI action=Continue')
        audio.wait_log(r'DTUI flow=Playing ')
        layout = open_performance(pause_settings())
        viewport = rotate(True)
        layout = audio.current_layout(viewport)
        record['landscape_dimensions'] = wait_resolution(viewport)
        audio.screenshot(out, '02-performance-display-landscape', viewport)
        viewport = rotate(False)
        layout = audio.current_layout(viewport)
        wait_resolution(viewport)
        audio.tap(layout['performance_sections'][1], r'DTSETTINGS action=performance_section value=Diagnostics')
        audio.screenshot(out, '03-performance-diagnostics-portrait', viewport)
        complete_wanted = audio.count(r'DTPERFORMANCE capture=complete ') + 1
        saved_wanted = audio.count(r'DTPERFORMANCE report_saved=') + 1
        audio.tap(layout['capture'], r'DTSETTINGS action=capture_start')
        audio.wait_log(r'DTPERFORMANCE capture=waiting')
        close_and_resume(layout)
        audio.wait_log(r'DTPERFORMANCE capture=recording', 1, 120)
        audio.screenshot(out, '04-performance-live-session', viewport)
        audio.wait_log(r'DTPERFORMANCE capture=complete ', complete_wanted, 240)
        audio.wait_log(r'DTPERFORMANCE report_saved=', saved_wanted, 90)
        raw_report = audio.raw_private(REPORT)
        assert len(raw_report) <= 4 * 1024 * 1024
        measured = json.loads(raw_report)
        validate_live_report(measured, viewport)
        (out / REPORT).write_bytes(raw_report)
        record['live_report_sha256'] = hashlib.sha256(raw_report).hexdigest()
        record['live_summary'] = measured['summary']
        record['live_duration_seconds'] = measured['duration_seconds']
        record['live_report_verified'] = True
        layout = open_performance(pause_settings(), 1)
        record['share'] = verify_share_chooser(out, layout)
        audio.wait_log(r'DTPERF fps=', audio.count(r'DTPERF fps=') + 1, 90)
        layout = open_performance(audio.current_layout(viewport))
        audio.tap(layout['profiles'][3], r'DTSETTINGS action=performance_profile value=Quality')
        audio.tap(layout['frame_caps'][3], r'DTSETTINGS action=frame_cap value=120')
        audio.tap(layout['render_scales'][3], r'DTSETTINGS action=render_scale value=100')
        wait_resolution(viewport, 100)
        record['before_thermal'] = wait_json_log('DTPERFORMANCE effective=', lambda value: value['render_scale'] == 100)
        help_text = audio.adb('shell', 'cmd', 'thermalservice', 'help')
        if 'override-status' not in help_text:
            record['thermal_injection'] = {'tested': False, 'reason': 'Emulator shell does not expose override-status'}
        else:
            thermal_overridden = True
            override = audio.adb('shell', 'cmd', 'thermalservice', 'override-status', '3')
            protected = wait_json_log('DTPERFORMANCE effective=', lambda value:
                value['reason'] == 'thermal protection' and value['target_fps'] == 30 and value['render_scale'] == 70)
            wait_resolution(viewport, 70)
            audio.tap(layout['performance_sections'][1], r'DTSETTINGS action=performance_section value=Diagnostics')
            audio.screenshot(out, '06-thermal-protection-portrait', viewport)
            audio.adb('shell', 'cmd', 'thermalservice', 'reset')
            thermal_overridden = False
            recovered = wait_json_log('DTPERFORMANCE effective=', lambda value:
                value['reason'] != 'thermal protection' and value['render_scale'] == 100, timeout=120)
            record['thermal_injection'] = {'tested': True, 'source': 'Emulator thermal-service status injection, not physical heat',
                                           'override_output': override, 'protected': protected, 'recovered': recovered}
        # Return to the explicitly saved, measured configuration before the
        # subsequent independent APK update/migration smoke.
        layout = open_performance(audio.current_layout(viewport))
        audio.tap(layout['profiles'][2], r'DTSETTINGS action=performance_profile value=Balanced')
        audio.tap(layout['frame_caps'][0], r'DTSETTINGS action=frame_cap value=30')
        audio.tap(layout['render_scales'][1], r'DTSETTINGS action=render_scale value=70')
        audio.preferences(lambda prefs: prefs == expected)
        audio.tap(layout['close'], r'DTUI action=SettingsClose')
        record['success'] = True
        print(json.dumps({'checkpoint': 'android-performance', 'success': True,
                          'samples': measured['summary']['samples'], 'duration_seconds': measured['duration_seconds']}), flush=True)
    except Exception as error:
        record['error'] = str(error)
        raise
    finally:
        if thermal_overridden:
            try:
                audio.adb('shell', 'cmd', 'thermalservice', 'reset')
            except subprocess.SubprocessError as error:
                record['thermal_reset_error'] = str(error)
        for name, read in (
            ('logcat.txt', lambda: audio.adb('logcat', '-d', binary=True)),
            ('final.png', lambda: audio.adb('exec-out', 'screencap', '-p', binary=True)),
            ('final-preferences.json', lambda: audio.raw_private(audio.PREFS)),
            ('final-save.json', lambda: audio.raw_private(audio.SAVE)),
        ):
            try:
                (out / name).write_bytes(read())
            except (OSError, subprocess.SubprocessError) as error:
                record.setdefault('diagnostic_errors', {})[name] = str(error)
        (out / 'performance-smoke-report.json').write_text(json.dumps(record, indent=2, allow_nan=False) + '\n')
        audio.adb('shell', 'am', 'force-stop', audio.PKG)
        audio.adb('shell', 'wm', 'size', 'reset')
        audio.adb('shell', 'wm', 'density', 'reset')
        rotate(False)


if __name__ == '__main__':
    main()
