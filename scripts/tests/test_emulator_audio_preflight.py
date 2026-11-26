"""Host-routing gate regressions. Pulse, process identity and time are mocked."""

import importlib.util
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest import mock


SCRIPT = Path(__file__).resolve().parents[1] / 'emulator-audio-preflight.py'
SPEC = importlib.util.spec_from_file_location('emulator_audio_preflight', SCRIPT)
preflight = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(preflight)


class EmulatorAudioPreflightTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.output = Path(temporary.name)
        self.server = 'unix:/run/user/1001/pulse/native'
        self.environment = {'PULSE_SERVER': self.server, 'QEMU_PA_SERVER': self.server,
                            'QEMU_AUDIO_DRV': 'pa', 'PULSE_SINK': preflight.SINK,
                            'QEMU_PA_SINK': preflight.SINK}

    def snapshot(self):
        return {'sinks': [{'index': 17, 'name': preflight.SINK, 'mute': False}],
                'clients': [{'index': 9, 'properties': {
                    'application.name': self.server,
                    'application.process.binary': 'qemu-system-x86_64'}}],
                'sink-inputs': [{'index': 23, 'client': 9, 'sink': 17, 'mute': False}]}

    def test_setup_exports_the_existing_unix_server_to_pulse_and_qemu(self):
        environment_file = self.output / 'github-env'
        with mock.patch.object(preflight, 'pulse_library_version', return_value='17.0'), \
                mock.patch.object(preflight, 'pactl', return_value=(
                'Server String: /run/user/1001/pulse/native\nCookie: DO_NOT_COPY')), \
                mock.patch.object(Path, 'is_socket', return_value=True), \
                mock.patch.dict(os.environ, {'GITHUB_ENV': str(environment_file)}):
            report = preflight.configure(self.output)
        self.assertEqual(report['environment'], self.environment)
        exported = dict(line.split('=', 1) for line in environment_file.read_text().splitlines())
        self.assertEqual(exported, self.environment)
        self.assertNotIn('DO_NOT_COPY', (self.output / 'pulse-setup.json').read_text())

    def test_legacy_qemu_loader_name_is_checked_before_emulator_launch(self):
        with mock.patch.object(preflight.ctypes, 'CDLL', side_effect=OSError('missing')) as load, \
                self.assertRaisesRegex(RuntimeError, 'libpulse.so; install libpulse-dev'):
            preflight.pulse_library_version()
        load.assert_called_once_with('libpulse.so')
        library = mock.Mock()
        library.pa_get_library_version.return_value = b'17.0'
        with mock.patch.object(preflight.ctypes, 'CDLL', return_value=library):
            self.assertEqual(preflight.pulse_library_version(), '17.0')

    def test_tcp_or_missing_socket_cannot_be_exported_as_authenticated_local_routing(self):
        for server, exists in [('tcp:127.0.0.1:4713', True), ('/tmp/missing-pulse', False)]:
            with self.subTest(server=server), \
                    mock.patch.object(preflight, 'pactl', return_value='Server String: ' + server), \
                    mock.patch.object(Path, 'is_socket', return_value=exists), \
                    self.assertRaisesRegex(RuntimeError, 'local Unix socket'):
                preflight.local_server()

    def test_name_alone_is_not_identity_and_source_server_name_still_matches_binary(self):
        snapshot = self.snapshot()
        self.assertEqual([stream['index'] for stream in preflight.emulator_streams(snapshot)], [23])
        for properties in ({'application.name': 'QEMU'},
                           {'application.name': 'Android Emulator', 'application.process.binary': 'paplay'}):
            with self.subTest(properties=properties):
                snapshot['clients'][0]['properties'] = properties
                self.assertEqual(preflight.emulator_streams(snapshot), [])

    def test_missing_binary_requires_a_real_emulator_process_executable(self):
        properties = {'application.name': self.server, 'application.process.id': '1234'}
        with mock.patch.object(os, 'readlink', return_value='/sdk/emulator/qemu/linux-x86_64/qemu-system-x86_64') as link:
            self.assertTrue(preflight.emulator_process(properties))
            link.assert_called_once_with('/proc/1234/exe')
        with mock.patch.object(os, 'readlink', return_value='/usr/bin/python3'):
            self.assertFalse(preflight.emulator_process(properties))
        with mock.patch.object(os, 'readlink', side_effect=FileNotFoundError):
            self.assertFalse(preflight.emulator_process(properties))
        with mock.patch.object(os, 'readlink') as link:
            self.assertFalse(preflight.emulator_process({'application.process.id': '../1234'}))
            link.assert_not_called()

    def test_actual_emulator_stream_on_dedicated_sink_passes(self):
        snapshot = self.snapshot()
        with mock.patch.dict(os.environ, self.environment), \
                mock.patch.object(preflight, 'local_server', return_value=self.server), \
                mock.patch.object(preflight, 'routing_snapshot', return_value=snapshot):
            report = preflight.preflight(self.output, timeout=0)
        self.assertTrue(report['success'])
        self.assertEqual(report['matched_sink_index'], 17)
        self.assertEqual(report['matched_stream_ids'], [23])

    def test_wrong_sink_unrelated_muted_or_missing_streams_fail_with_diagnostics(self):
        for name, stream in (
            ('wrong-sink', {'index': 23, 'client': 9, 'sink': 99, 'mute': False}),
            ('unrelated-client', {'index': 23, 'client': 11, 'sink': 17, 'mute': False}),
            ('muted', {'index': 23, 'client': 9, 'sink': 17, 'mute': True}),
            ('missing', None),
        ):
            snapshot = self.snapshot()
            snapshot['sink-inputs'] = [] if stream is None else [stream]
            with self.subTest(name=name), mock.patch.dict(os.environ, self.environment), \
                    mock.patch.object(preflight, 'local_server', return_value=self.server), \
                    mock.patch.object(preflight, 'routing_snapshot', return_value=snapshot), \
                    self.assertRaises(RuntimeError):
                preflight.preflight(self.output, timeout=0)
            evidence = json.loads((self.output / 'pulse-routing.json').read_text())
            self.assertFalse(evidence['success'])
            self.assertEqual(evidence['snapshot'], snapshot)


if __name__ == '__main__':
    unittest.main()
