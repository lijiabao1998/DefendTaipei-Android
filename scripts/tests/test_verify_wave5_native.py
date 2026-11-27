"""Reject incomplete/contradictory evidence; these are synthetic helper inputs."""
import copy
import importlib.util
from pathlib import Path
import unittest

SCRIPTS = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location('verify_wave5_native', SCRIPTS / 'verify-wave5-native.py')
verify = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(verify)


def restore_fixture():
    saved = {'current': 4, 'vehicles': [
        {'id': 4, 'speed': 8, 'fuel': 11, 'gun': {'selected': 0, 'guns': [{'ready': 7, 'reserve': 30}]}},
        {'id': 8, 'speed': 0, 'fuel': 3, 'gun': None},
    ]}
    expected = copy.deepcopy(saved)
    expected['vehicles'][0]['speed'] = 0
    armor = {'units': [{'id': 6, 'hp': 30, 'abandoned': True, 'captured_id': 4}]}
    return {'scene': 'Continued', 'transport_restore_exact': True, 'saved_vehicles': saved,
            'expected_vehicles': copy.deepcopy(expected), 'restored_vehicles': copy.deepcopy(expected),
            'saved_armor': copy.deepcopy(armor), 'restored_armor': copy.deepcopy(armor),
            'current': 4, 'vehicles': copy.deepcopy(expected)}


class NativeWave5EvidenceTests(unittest.TestCase):
    def test_only_occupied_speed_normalizes_on_continue(self):
        value = restore_fixture()
        before = copy.deepcopy(value)
        verify.verify_restore(value)
        self.assertEqual(value, before)
        for key, replacement in [('fuel', 12), ('speed', 1)]:
            with self.subTest(key=key), self.assertRaises(AssertionError):
                changed = restore_fixture()
                changed['expected_vehicles']['vehicles'][0][key] = replacement
                changed['restored_vehicles']['vehicles'][0][key] = replacement
                verify.verify_restore(changed)

    def test_true_success_flag_does_not_hide_lost_ammunition_or_capture_links(self):
        changed = restore_fixture()
        changed['restored_vehicles']['vehicles'][0]['gun']['guns'][0]['reserve'] = 29
        with self.assertRaises(AssertionError):
            verify.verify_restore(changed)
        changed = restore_fixture()
        changed['restored_armor']['units'][0]['captured_id'] = None
        with self.assertRaises(AssertionError):
            verify.verify_restore(changed)

    def test_duplicate_vehicle_ids_and_unoccupied_continuation_fail(self):
        changed = restore_fixture()
        changed['saved_vehicles']['vehicles'][1]['id'] = 4
        with self.assertRaises(AssertionError):
            verify.verify_restore(changed)
        changed = restore_fixture()
        changed['current'] = None
        with self.assertRaises(AssertionError):
            verify.verify_restore(changed)

    def test_audio_requires_all_original_and_added_scenarios_in_order(self):
        for names in [verify.AUDIO_SCENARIOS[:13],
                      [*verify.AUDIO_SCENARIOS[:-1], verify.AUDIO_SCENARIOS[0]],
                      list(reversed(verify.AUDIO_SCENARIOS))]:
            with self.subTest(names=names), self.assertRaises(AssertionError):
                verify.verify_audio({'success': True, 'scenarios': [{'name': name} for name in names]}, Path('/unused'))

    def test_wave5_cannot_pass_with_only_earlier_captures_or_missing_added_scene(self):
        for captures in [[{'stage': 'previous'}] * 68,
                         [{'stage': 'previous'}] * 80,
                         [{'stage': stage} for stage, _, _ in verify.STAGES[:-1]] + [{'stage': 'previous'}] * 69]:
            with self.subTest(count=len(captures)), self.assertRaises(AssertionError):
                verify.verify_report({'success': True, 'scope': 'full', 'expected_captures': len(captures), 'captures': captures}, Path('/unused'), {})

    def test_ammunition_totals_reject_invalid_counts(self):
        for amount in [-1, True, .5]:
            with self.subTest(amount=amount), self.assertRaises(AssertionError):
                verify.ammunition({'gun': {'selected': 0, 'guns': [{'ready': amount, 'reserve': 30}]}})


if __name__ == '__main__':
    unittest.main()
