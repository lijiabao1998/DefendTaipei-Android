"""Acceptance helpers only; fixtures never masquerade as emulator captures."""
import copy
import importlib.util
import json
from pathlib import Path
import sys
import unittest
from unittest import mock

SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))
SPEC = importlib.util.spec_from_file_location('emulator_wave5_smoke', SCRIPTS / 'emulator-wave5-smoke.py')
smoke = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(smoke)


def baseline():
    return {'version': 1, 'player': {'position': [0, 0, 0], 'hp': 19, 'bleed': True, 'stun': 4},
            'campaign': {'speed': 20, 'sleeping': True, 'notice_t': 7, 'day_hours': 4},
            'needs': {'hunger': 10, 'thirst': 20, 'fatigue': 60},
            'world': {'dead': [9, 72], 'loot': [['test', [['water', 3]]]]},
            'map': {'notes': ['unrelated original annotation']}, 'role_id': 'office',
            'inventory': {'items': [['rice', 1]], 'equipped': ''}, 'vehicles': {'old': True}}


class Wave5AcceptanceTests(unittest.TestCase):
    def test_every_fixture_install_refuses_non_emulator_before_writing(self):
        with mock.patch.object(smoke.audio, 'adb', return_value='0') as adb, \
             mock.patch.object(smoke.subprocess, 'run') as run:
            with self.assertRaisesRegex(RuntimeError, 'disposable Android emulator'):
                smoke.install_exact(b'not sent', Path('/unused'), 'test')
            adb.assert_called_once_with('shell', 'getprop', 'ro.kernel.qemu')
            run.assert_not_called()

    def test_preparation_preserves_unrelated_campaign_and_keeps_input_immutable(self):
        descriptor = json.loads(smoke.FIXTURE.read_text())
        original = baseline()
        before = copy.deepcopy(original)
        descriptor_before = copy.deepcopy(descriptor)
        prepared = smoke.prepare_scenario(original, descriptor, 'civilian')
        self.assertEqual(original, before)
        self.assertEqual(descriptor, descriptor_before)
        self.assertEqual(prepared['map'], original['map'])
        self.assertEqual(prepared['world']['loot'], original['world']['loot'])
        self.assertEqual(prepared['role_id'], original['role_id'])
        self.assertEqual(prepared['campaign']['day_hours'], 4)
        self.assertIn(72, prepared['world']['dead'])
        self.assertTrue(set(range(64)) <= set(prepared['world']['dead']))
        car = smoke.vehicle(prepared, descriptor['civilian']['vehicle_id'])
        self.assertEqual((car['fuel'], car['tyre'], car['battery']), (4, .3, .2))
        self.assertIsNone(prepared['vehicles']['current'])
        self.assertEqual(smoke.bag(prepared), {'gas': 2, 'patch': 1, 'carbat': 1, 'hose': 1})
        self.assertEqual(prepared['armor'], descriptor['civilian']['armor'])

    def test_armored_fixture_keeps_live_target_and_real_mounted_ammunition(self):
        descriptor = json.loads(smoke.FIXTURE.read_text())
        prepared = smoke.prepare_scenario(baseline(), descriptor, 'armored')
        self.assertEqual(smoke.ammo_total(smoke.vehicle(prepared, 50000)), [38, 1650])
        self.assertEqual(len(prepared['armor']['units']), 1)
        self.assertFalse(prepared['armor']['units'][0]['destroyed'])
        self.assertFalse(prepared['armor']['units'][0]['abandoned'])
        self.assertEqual(smoke.bag(prepared), {})
        self.assertEqual(prepared['player']['hp'], 100)
        self.assertFalse(prepared['player']['bleed'])

    def test_duplicate_or_missing_authoritative_ids_fail(self):
        for fleet in [[], [{'id': 3}, {'id': 3}]]:
            with self.subTest(fleet=fleet), self.assertRaises(AssertionError):
                smoke.vehicle({'vehicles': {'vehicles': fleet}}, 3)

    def test_invalid_inventory_or_ammunition_cannot_pass_conservation_checks(self):
        for entries in [[['gas', 1], ['gas', 1]], [['gas', -1]], [['gas', True]], [['gas', 1.5]]]:
            with self.subTest(entries=entries), self.assertRaises(AssertionError):
                smoke.bag({'inventory': {'items': entries}})
        for ready in [-1, True, .5]:
            with self.subTest(ready=ready), self.assertRaises(AssertionError):
                smoke.ammo_total({'gun': {'selected': 0, 'guns': [{'ready': ready, 'reserve': 1}]}})

    def test_touch_targets_are_real_nonempty_minmax_rectangles(self):
        self.assertEqual(smoke.center([10, 20, 30, 40]), (20, 30))
        for rect in [[10, 20, 0, 0], [1, 2, 1, 4], [1, 2, 3]]:
            with self.subTest(rect=rect), self.assertRaises(AssertionError):
                smoke.center(rect)


if __name__ == '__main__':
    unittest.main()
