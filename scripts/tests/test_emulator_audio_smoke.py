"""Acceptance preconditions: exact zero and effective guest volume, no emulator needed."""

import importlib.util
from pathlib import Path
import sys
import unittest
from unittest import mock


SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))
SPEC = importlib.util.spec_from_file_location('emulator_audio_smoke', SCRIPTS / 'emulator-audio-smoke.py')
smoke = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(smoke)


class EmulatorAudioSmokeTests(unittest.TestCase):
    def test_zero_and_full_endpoints_cannot_accept_nearby_slider_values(self):
        self.assertTrue(smoke.volume_matches(0.0, 0.0))
        self.assertTrue(smoke.volume_matches(1.0, 1.0))
        self.assertFalse(smoke.volume_matches(0.01, 0.0))
        self.assertFalse(smoke.volume_matches(0.99, 1.0))
        self.assertTrue(smoke.volume_matches(0.65, 0.65))
        self.assertFalse(smoke.volume_matches(0.8, 0.65))

    def test_volume_readback_preserves_actual_guest_range(self):
        for maximum in (15, 25):
            result = smoke.parse_media_volume(f'[v] volume is 7 in range [0..{maximum}]\n')
            self.assertEqual(result, {'current': 7, 'minimum': 0, 'maximum': maximum})

    def test_volume_readback_requires_valid_effective_value(self):
        for output in ('[v] will set volume to 15', 'volume is 16 in range [0..15]',
                       'volume is 0 in range [0..0]', ''):
            with self.subTest(output=output), self.assertRaises(AssertionError):
                smoke.parse_media_volume(output)

    def test_media_volume_reaches_exact_maximum_after_first_key_only_opens_panel(self):
        # API 34's first idle volume key may only reveal the system panel.
        readings = [{'current': current, 'minimum': 0, 'maximum': 15}
                    for current in [5, 5, *range(6, 16)]]
        report = {}
        with mock.patch.object(smoke, 'media_volume', side_effect=readings), \
                mock.patch.object(smoke, 'adb') as adb, \
                mock.patch.object(smoke.time, 'sleep'), \
                mock.patch.object(smoke.time, 'monotonic', return_value=0):
            smoke.configure_media_volume(report)
        self.assertEqual(report['initial_media_volume'], readings[0])
        self.assertEqual(report['configured_media_volume'], readings[-1])
        self.assertEqual(report['media_volume_setup']['readbacks'], readings)
        presses = [call for call in adb.call_args_list if 'KEYCODE_VOLUME_UP' in call.args]
        self.assertEqual(len(presses), 11)
        self.assertEqual(report['media_volume_setup']['key_presses'], len(presses))
        # The broken media-session setter and permission changes cannot be a
        # fallback: only ordinary Home/volume key events configure the device.
        self.assertTrue(all(call.args in (
            ('shell', 'input', 'keyevent', 'KEYCODE_HOME'),
            ('shell', 'input', 'keyevent', 'KEYCODE_VOLUME_UP')) for call in adb.call_args_list))

    def test_media_volume_uses_device_maximum_instead_of_assuming_fifteen(self):
        readings = [{'current': current, 'minimum': 0, 'maximum': 25}
                    for current in range(5, 26)]
        report = {}
        with mock.patch.object(smoke, 'media_volume', side_effect=readings), \
                mock.patch.object(smoke, 'adb'), \
                mock.patch.object(smoke.time, 'sleep'), \
                mock.patch.object(smoke.time, 'monotonic', return_value=0):
            smoke.configure_media_volume(report)
        self.assertEqual(report['configured_media_volume']['current'], 25)
        self.assertEqual(report['media_volume_setup']['readbacks'], readings)

    def test_media_volume_already_at_maximum_needs_no_input(self):
        initial = {'current': 15, 'minimum': 0, 'maximum': 15}
        report = {}
        with mock.patch.object(smoke, 'media_volume', return_value=initial), \
                mock.patch.object(smoke, 'adb') as adb:
            smoke.configure_media_volume(report)
        adb.assert_not_called()
        self.assertEqual(report['configured_media_volume'], initial)
        self.assertEqual(report['media_volume_setup']['key_presses'], 0)

    def test_media_volume_unchanged_is_bounded_failure_with_actual_readbacks(self):
        initial = {'current': 5, 'minimum': 0, 'maximum': 15}
        report = {}
        with mock.patch.object(smoke, 'media_volume', return_value=initial), \
                mock.patch.object(smoke, 'adb'), \
                mock.patch.object(smoke.time, 'sleep'), \
                mock.patch.object(smoke.time, 'monotonic', return_value=0), \
                self.assertRaisesRegex(AssertionError, 'maximum'):
            smoke.configure_media_volume(report)
        self.assertEqual(report['configured_media_volume'], initial)
        evidence = report['media_volume_setup']
        self.assertGreater(evidence['key_presses'], 0)
        self.assertLessEqual(evidence['key_presses'], 64)
        self.assertEqual(evidence['readbacks'], [initial] * (evidence['key_presses'] + 1))

    def test_media_volume_changed_range_is_rejected_and_retained(self):
        initial = {'current': 5, 'minimum': 0, 'maximum': 15}
        for changed in ({'current': 6, 'minimum': 0, 'maximum': 25},
                        {'current': 6, 'minimum': 1, 'maximum': 15}):
            report = {}
            with self.subTest(changed=changed), \
                    mock.patch.object(smoke, 'media_volume', side_effect=[initial, changed]), \
                    mock.patch.object(smoke, 'adb'), \
                    mock.patch.object(smoke.time, 'sleep'), \
                    mock.patch.object(smoke.time, 'monotonic', return_value=0), \
                    self.assertRaisesRegex(AssertionError, 'range'):
                smoke.configure_media_volume(report)
            self.assertEqual(report['configured_media_volume'], changed)
            self.assertEqual(report['media_volume_setup']['readbacks'], [initial, changed])

    def test_media_volume_deadline_stops_before_exhausting_key_budget(self):
        initial = {'current': 5, 'minimum': 0, 'maximum': 15}
        clock = [0.0]

        def advance(seconds):
            clock[0] += seconds

        report = {}
        with mock.patch.object(smoke, 'media_volume', return_value=initial), \
                mock.patch.object(smoke, 'adb'), \
                mock.patch.object(smoke.time, 'sleep', side_effect=advance), \
                mock.patch.object(smoke.time, 'monotonic', side_effect=lambda: clock[0]), \
                self.assertRaisesRegex(AssertionError, 'maximum'):
            smoke.configure_media_volume(report, timeout=1.0)
        self.assertLessEqual(report['media_volume_setup']['key_presses'], 3)
        self.assertEqual(report['configured_media_volume'], initial)

    def test_silence_precondition_requires_actual_zero_or_mute(self):
        audio = {'master': 0.8, 'effects': 1.0, 'ambience': 0.7, 'radio': 0.01, 'muted': False}
        with self.assertRaisesRegex(AssertionError, 'actual saved settings'):
            smoke.assert_audition_settings({'audio': audio}, 3, False)
        smoke.assert_audition_settings({'audio': {**audio, 'radio': 0.0}}, 3, False)
        smoke.assert_audition_settings({'audio': {**audio, 'master': 0.0}}, 3, False)
        smoke.assert_audition_settings({'audio': {**audio, 'muted': True}}, 3, False)
        smoke.assert_audition_settings({'audio': audio}, 3, True)
        with self.assertRaises(AssertionError):
            smoke.assert_audition_settings({'audio': {**audio, 'radio': 0.0}}, 3, True)


if __name__ == '__main__':
    unittest.main()
