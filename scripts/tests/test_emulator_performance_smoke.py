"""Measured-report acceptance gates; no Android process or fabricated CI evidence."""

import copy
import importlib.util
from pathlib import Path
import sys
import unittest


SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))
SPEC = importlib.util.spec_from_file_location('emulator_performance_smoke', SCRIPTS / 'emulator-performance-smoke.py')
smoke = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(smoke)


def example_report():
    # A schema fixture is unit-test data only. It is never uploaded as a runtime
    # capture or used to replace the emulator's app-private perf-report.json.
    device = {'schema_version': 1, 'platform': 'android', 'reported': True,
              'sample_age_secs': 0.5, 'sdk_int': 34, 'manufacturer': 'test', 'model': 'fixture',
              'total_memory_bytes': 4_000_000_000, 'available_memory_bytes': 2_000_000_000,
              'current_refresh_hz': 60, 'supported_refresh_hz': [60], 'process_pss_kb': 100_000,
              'thermal_status': 0, 'thermal_headroom': None, 'media_volume': 25, 'media_max_volume': 25}
    return {'schema_version': 1, 'measurement': 'app_frame_interval_not_gpu_or_present_time',
            'source': 'live_session', 'build_revision': 'fixture-only', 'duration_seconds': 60,
            'warmup_seconds': 5, 'frames': [{'ms': 1000, 'target_fps': 30, 'render_scale': 70} for _ in range(60)],
            'summary': {'samples': 60, 'mean_ms': 1000, 'fps': 1, 'p50_ms': 1000,
                        'p95_ms': 1000, 'p99_ms': 1000, 'max_ms': 1000,
                        'over_budget_frames': 60, 'over_budget_ratio': 1},
            **{'device_' + edge: copy.deepcopy(device) for edge in ('start', 'end')},
            **{'render_dimensions_' + edge: {'ui': [390, 844], 'scene': [273, 591], 'scale': 70}
               for edge in ('start', 'end')},
            **{'effective_' + edge: {'target_fps': 30, 'render_scale': 70}
               for edge in ('start', 'end')}}


class EmulatorPerformanceSmokeTests(unittest.TestCase):
    def test_consistent_low_fps_live_measurement_is_valid_without_fps_floor(self):
        smoke.validate_live_report(example_report(), (390, 844))

    def test_fixed_timestep_or_short_capture_cannot_be_live_measurement(self):
        for key, value in (('source', 'fixed_timestep_qa'), ('duration_seconds', 59.9),
                           ('warmup_seconds', 0), ('measurement', 'gpu_time')):
            sample = example_report()
            sample[key] = value
            with self.subTest(key=key), self.assertRaises(AssertionError):
                smoke.validate_live_report(sample, (390, 844))

    def test_nonfinite_or_inconsistent_metrics_are_rejected(self):
        for key, value in (('samples', 61), ('mean_ms', 100), ('fps', float('nan')),
                           ('p95_ms', 500), ('max_ms', float('inf')),
                           ('over_budget_frames', 61), ('over_budget_ratio', 0.5)):
            sample = example_report()
            sample['summary'][key] = value
            with self.subTest(key=key), self.assertRaises(AssertionError):
                smoke.validate_live_report(sample, (390, 844))
        for value in (0, -1, float('nan'), float('inf'), True):
            sample = example_report()
            sample['frames'][0]['ms'] = value
            with self.subTest(frame=value), self.assertRaises(AssertionError):
                smoke.validate_live_report(sample, (390, 844))

    def test_ui_resolution_stays_native_when_scene_is_scaled(self):
        sample = example_report()
        sample['render_dimensions_start']['ui'] = [273, 591]
        with self.assertRaisesRegex(AssertionError, 'physical window'):
            smoke.validate_live_report(sample, (390, 844))
        sample = example_report()
        sample['render_dimensions_end']['scene'] = [390, 844]
        with self.assertRaisesRegex(AssertionError, 'scene resolution'):
            smoke.validate_live_report(sample, (390, 844))

    def test_stale_or_unreported_device_info_cannot_satisfy_bridge_gate(self):
        for field, value in (('reported', False), ('sample_age_secs', 11),
                             ('platform', 'linux'), ('sdk_int', 0), ('supported_refresh_hz', []),
                             ('media_volume', 26), ('current_refresh_hz', float('nan'))):
            sample = example_report()
            sample['device_end'][field] = value
            with self.subTest(field=field), self.assertRaises(AssertionError):
                smoke.validate_live_report(sample, (390, 844))

    def test_share_uri_match_is_limited_to_report_provider_subdirectory(self):
        good = 'content://tw.lijiabao.defendtaipei.performance_reports/performance_reports/perf-report-123456.json'
        self.assertEqual(smoke.SHARE_URI.fullmatch(good).group(), good)
        for bad in (good.replace('performance_reports/perf-', '../perf-'),
                    good.replace('perf-report-123456.json', 'settings.json'),
                    good.replace('content:', 'file:'),
                    good.replace('.performance_reports/', '.other/')):
            self.assertIsNone(smoke.SHARE_URI.fullmatch(bad))


if __name__ == '__main__':
    unittest.main()
