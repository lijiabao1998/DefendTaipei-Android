#!/usr/bin/env python3
"""Check Wave 5 runtime evidence without replacing the earlier native CI gates.

The GPU PNGs and state snapshots come from the running native application.
Geometry counters describe upload/view membership, not pixel occlusion or FPS.
"""
from __future__ import annotations

import argparse
import copy
import json
import math
from pathlib import Path
import wave

ROOT = Path(__file__).resolve().parent.parent
STAGES = [
    ('09w01-utility-signals-landscape', 'Utilities', (1280, 720)),
    ('09w02-market-cart-landscape', 'Market', (1280, 720)),
    ('09w03-temple-entrance-landscape', 'Temple', (1280, 720)),
    ('09w04-controls-portrait', 'Controls', (390, 844)),
    ('09w05-vehicle-inspect-landscape', 'Inspect', (1280, 720)),
    ('09w06-vehicle-board-portrait', 'Board', (390, 844)),
    ('09w07-vehicle-drive-landscape', 'Drive', (1280, 720)),
    ('09w08-vehicle-service-portrait', 'Service', (390, 844)),
    ('09w09-scooter-rider-portrait', 'Rider', (390, 844)),
    ('09w10-armored-board-landscape', 'Armor', (1280, 720)),
    ('09w11-armored-capture-landscape', 'Capture', (1280, 720)),
    ('09w12-mounted-controls-portrait', 'Mounted', (390, 844)),
]
AUDIO_SCENARIOS = [
    'showcase', 'muted', 'effects_zero', 'ambience', 'ambience_zero', 'paused',
    'resume', 'title_preview', 'radio', 'radio_zero', 'live_muted', 'live_paused',
    'live_background', 'armor_engine', 'armor_engine_zero',
    'armor_engine_live_muted', 'armor_engine_live_paused', 'armor_engine_live_background',
]
CONTROL_ACTIONS = {
    'up', 'down', 'left', 'right', 'sprint', 'sneak', 'interact', 'inventory', 'map',
    'shelter', 'trade', 'barricade', 'sleep', 'speed1', 'speed3', 'speed20',
    'vehicle_brake', 'vehicle_reload', 'vehicle_switch',
}


def one_vehicle(fleet: dict, identity: int) -> dict:
    vehicles = fleet['vehicles']
    assert len({value['id'] for value in vehicles}) == len(vehicles), 'Duplicate vehicle IDs'
    selected = [value for value in vehicles if value['id'] == identity]
    assert len(selected) == 1, ('Missing authoritative vehicle', identity)
    return selected[0]


def ammunition(vehicle: dict) -> list[int]:
    guns = vehicle['gun']['guns']
    assert 0 <= vehicle['gun']['selected'] < len(guns), vehicle
    for gun in guns:
        assert all(type(gun[key]) is int and gun[key] >= 0 for key in ('ready', 'reserve')), gun
    return [gun['ready'] + gun['reserve'] for gun in guns]


def verify_restore(evidence: dict) -> None:
    assert evidence['scene'] == 'Continued' and evidence['transport_restore_exact'] is True, evidence
    expected = copy.deepcopy(evidence['saved_vehicles'])
    if expected['current'] is not None:
        one_vehicle(expected, expected['current'])['speed'] = 0.0
    assert expected == evidence['expected_vehicles'], 'Expected restore changed more than occupied speed'
    assert expected == evidence['restored_vehicles'], 'Load-time fleet differs from the actual checkpoint'
    assert evidence['saved_armor'] == evidence['restored_armor'], 'Load-time armor/capture links changed'
    assert evidence['current'] == expected['current'], 'Continued capture lost the occupied vehicle'
    assert evidence['vehicles']['current'] == expected['current'], evidence


def verify_png(capture: dict, directory: Path, expected_size: tuple[int, int]) -> None:
    from PIL import Image
    path = Path(capture['file']).resolve()
    assert path == (directory / (capture['stage'] + '.png')).resolve(), capture
    assert (capture['width'], capture['height']) == expected_size, capture
    with Image.open(path) as image:
        assert image.size == expected_size, capture
        image.verify()


def verify_report(report: dict, directory: Path, descriptor: dict) -> None:
    assert report['success'] and report['scope'] == 'full', report
    assert report['expected_captures'] == len(report['captures']) == 80, report
    added = [capture for capture in report['captures'] if capture['stage'].startswith('09w')]
    assert [capture['stage'] for capture in added] == [stage for stage, _, _ in STAGES], added
    names = [capture['stage'] for capture in report['captures']]
    first = names.index(STAGES[0][0])
    assert names[first - 1] == '09-barter-portrait' and names[first + 12] == '10-title-with-save', names
    assert names[first:first + 12] == [stage for stage, _, _ in STAGES], names
    scenes = {}
    for capture, (_, scene, size) in zip(added, STAGES):
        verify_png(capture, directory, size)
        evidence = capture['wave5']
        assert evidence['scene'] == scene, evidence
        scenes[scene] = evidence

    for scene in ('Utilities', 'Market', 'Temple'):
        evidence = scenes[scene]
        assert evidence['source'] == 'production_world_no_display_only_props', evidence
        assert evidence['visibility_measurement'] == 'actual_uploaded_geometry_owner_material_bounds_and_main_view_membership; source envelope screening; raw GPU image still requires visual review', evidence
        assert evidence['viewport'] == [1280, 720], evidence
        assert math.dist([c - p for c, p in zip(evidence['camera'], evidence['player'])], [40, 56, 40]) < .01, evidence
        lo, hi = evidence['projected_bounds']
        assert all(.02 * size <= low < high <= .98 * size for low, high, size in zip(lo, hi, evidence['viewport'])), evidence
    utilities = scenes['Utilities']['detail']
    assert utilities['cable_to'] is not None and utilities['batches'], utilities
    assert sum(batch['feature_vertices'] for batch in utilities['batches']) > 100, utilities
    for batch in utilities['batches']:
        assert batch['main_view_member'] and batch['source_vertices_in_chunk'], batch
        assert batch['actual_uploaded_vertices'] >= batch['feature_vertices'] > 0, batch
    assert utilities['signal_phase_seconds'] == 0, utilities
    assert [any(channel > 0 for channel in rgb) for rgb in utilities['signal_emissions']] == [False, False, True, True, False, False], utilities
    market = scenes['Market']['detail']
    for key in ('cart_inside_original_collision', 'body_in_main_view', 'stock_in_main_view', 'roof_hidden_by_actual_cutaway'):
        assert market[key] is True, (key, market)
    assert market['body_uploaded_vertices'] >= market['cart_vertices'] > 100, market
    assert market['stock_uploaded_vertices'] > 0 and market['saved_stock'] and market['original_loot_kind'], market
    temple = scenes['Temple']['detail']
    assert temple['original_name'] and temple['batches'], temple
    assert temple['generic_shop_name_suppressed'] is True and temple['displayed_shop_label'] is None, temple
    assert temple['plaque_style'] == 'unbranded_dark_temple', temple
    blank = temple['actual_uploaded_blank_sign_batches']
    assert blank, temple
    index = temple['stored_original_sign_index']
    expected_uv = [(index % 4 + 4.5 / 256) / 4, (index // 4 + 4.5 / 64) / 5]
    for batch in blank:
        assert batch['material'] == 'Sign' and batch['main_view_member'] is True, batch
        assert batch['actual_uploaded_vertices'] > 0 and math.dist(batch['flat_uv'], expected_uv) < 1e-6, batch
    assert sum(batch['ornament_vertices'] for batch in temple['batches']) > 100, temple
    for batch in temple['batches']:
        assert batch['main_view_member'] and batch['actual_uploaded_vertices'] >= batch['ornament_vertices'] > 0, batch

    controls = scenes['Controls']
    assert controls['button_percent'] == 110 and type(controls['vibration']) is bool, controls
    assert set(controls['bindings']) == CONTROL_ACTIONS, controls
    assert len(set(controls['bindings'].values())) == len(CONTROL_ACTIONS), controls
    assert not {'Escape', 'F3', 'KeyF'} & set(controls['bindings'].values()), controls

    for scene in ('Inspect', 'Board', 'Drive', 'Service', 'Rider', 'Armor', 'Capture', 'Mounted'):
        evidence = scenes[scene]
        vehicle = evidence['vehicle']
        assert evidence['body_in_main_view'] is True and evidence['body_vertices'] >= 100, evidence
        assert evidence['body_indices'] > 0 and evidence['body_indices'] % 3 == 0, evidence
        assert math.dist([evidence['rendered_position'][0], evidence['rendered_position'][2]], vehicle['pos']) <= .01, evidence
        if scene in ('Inspect', 'Service'):
            assert evidence['current'] is None, evidence
        else:
            assert evidence['current'] == vehicle['id'], evidence
            assert math.dist([evidence['player'][0], evidence['player'][2]], vehicle['pos']) < .01, evidence
    inspect, board, drive = (scenes[name] for name in ('Inspect', 'Board', 'Drive'))
    assert inspect['vehicle']['id'] == board['vehicle']['id'] == drive['vehicle']['id'] == descriptor['civilian']['vehicle_id'], scenes
    assert inspect['vehicle']['kind'] == board['vehicle']['kind'] == drive['vehicle']['kind'] == 'kei', scenes
    assert drive['distance_driven'] >= .2 and drive['fuel_used'] > 0 and drive['vehicle']['speed'] == 0, drive
    assert math.dist(inspect['vehicle']['pos'], drive['vehicle']['pos']) >= .2, drive
    assert drive['vehicle']['fuel'] < inspect['vehicle']['fuel'], drive
    service = scenes['Service']['vehicle']
    assert service['tyre'] == 1 and service['battery'] == 1 and abs(service['fuel'] - 7) < .001, service
    assert scenes['Rider']['vehicle']['kind'] == 'delivery', scenes['Rider']

    source_id = descriptor['armored']['vehicle_id']
    original = one_vehicle(descriptor['armored']['fleet'], source_id)
    before = ammunition(original)
    captured = scenes['Capture']
    assert captured['vehicle']['id'] == source_id and captured['vehicle']['kind'] == 'ztd05', captured
    assert ammunition(captured['vehicle']) == [before[0] - 1, *before[1:]], captured
    target = [unit for unit in captured['armor']['units'] if unit['id'] == descriptor['armored']['target_id']]
    assert len(target) == 1 and target[0]['hp'] == 30 and target[0]['abandoned'] and not target[0]['destroyed'], captured
    identity = target[0]['captured_id']
    assert identity is not None and identity != source_id, target
    mounted = scenes['Mounted']
    assert mounted['captured_vehicle_id'] == mounted['current'] == mounted['vehicle']['id'] == identity, mounted
    for key in ('captured_vehicle_boarded', 'source_gun_reload_started', 'source_gun_switch_verified'):
        assert mounted[key] is True, (key, mounted)
    assert mounted['vehicle']['kind'] == 'zbl08' and abs(mounted['vehicle']['engine'] - .4) < 1e-6, mounted
    assert mounted['vehicle']['gun']['selected'] == 1, mounted

    continued = next(capture for capture in report['captures'] if capture['stage'] == '11-continued')
    verify_png(continued, directory, (1280, 720))
    restore = continued['wave5']
    verify_restore(restore)
    assert restore['current'] == identity, restore
    assert ammunition(one_vehicle(restore['restored_vehicles'], source_id)) == [before[0] - 1, *before[1:]], restore
    assert one_vehicle(restore['restored_vehicles'], identity)['gun']['selected'] == 1, restore


def verify_audio(report: dict, directory: Path) -> None:
    assert report['success'] is True, report
    assert [case['name'] for case in report['scenarios']] == AUDIO_SCENARIOS, report
    manifest = json.loads((ROOT / 'assets/audio/manifest.json').read_text())
    assert len(manifest['clips']) == 105, 'Update the independent release gate when adding more audio'
    for case in report['scenarios']:
        runtime, pcm = case['runtime'], case['pcm']
        assert runtime['success'] and runtime['scenario'] == case['name'], case
        assert runtime['decoded_clips'] == len(manifest['clips']), runtime
        assert runtime['decoded_samples'] > 0 and 0 < runtime['decoded_peak'] <= .9, runtime
        assert case['expect_audible'] is (case['name'] not in {'muted', 'effects_zero', 'ambience_zero', 'paused', 'radio_zero', 'armor_engine_zero'}), case
        path = (directory / pcm['file']).resolve()
        assert path.parent == directory.resolve(), path
        with wave.open(str(path), 'rb') as output:
            assert (output.getframerate(), output.getnchannels(), output.getsampwidth()) == (48000, 2, 2), path
            assert abs(output.getnframes() / output.getframerate() - pcm['seconds']) < 1 / 48000, pcm
        assert pcm['seconds'] >= 1 and pcm['clipped_fraction'] <= .001, pcm
        assert pcm['rms'] >= .0001 if case['expect_audible'] else pcm['peak'] <= 2 / 32768, pcm
        if case['name'].startswith(('live_', 'armor_engine_live_')) or case['name'] == 'resume':
            windows = pcm['windows']
            assert len(windows) == 2, pcm
            assert [value['expect_audible'] for value in windows] == ([False, True] if case['name'] == 'resume' else [True, False]), pcm
            for window in windows:
                assert window['rms'] >= .0001 if window['expect_audible'] else window['peak'] <= 2 / 32768, window


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--report', type=Path)
    parser.add_argument('--audio', type=Path)
    args = parser.parse_args()
    if args.report is None and args.audio is None:
        parser.error('Specify --report, --audio, or both')
    if args.report is not None:
        fixture = json.loads((ROOT / 'game/tests/fixtures/android-vehicles-smoke.json').read_text())
        verify_report(json.loads(args.report.read_text()), args.report.parent, fixture)
        print('Verified all 12 Wave 5 captures and exact load-time transport continuation')
    if args.audio is not None:
        verify_audio(json.loads(args.audio.read_text()), args.audio.parent)
        print('Verified all 18 actual native output scenarios and 105 decoded clips')


if __name__ == '__main__':
    main()
