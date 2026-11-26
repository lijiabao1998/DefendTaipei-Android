"""Acceptance preconditions: exact zero and effective guest volume, no emulator needed."""

import importlib.util
from pathlib import Path
import sys
import unittest


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
