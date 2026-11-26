#!/usr/bin/env python3
"""Configure authenticated local Pulse routing and inspect the emulator stream.

Run --configure before launching the emulator, then run without it after the
normal 45-second application launch smoke. This never generates test sounds:
the later Android PCM scenarios remain the audible-output acceptance gate.
"""
from __future__ import annotations

import argparse
import ctypes
import json
import os
from pathlib import Path
import subprocess
import time


SINK = 'defendtaipei_android'
PCM_SPEC = 's16le 2ch 48000Hz'


def expected_environment(server: str) -> dict[str, str]:
    # AOSP audio/audio.c reads QEMU_AUDIO_DAC_FIXED_* before opening the
    # hardware voice. Avoid its default 44.1 kHz -> 48 kHz host conversion:
    # https://android.googlesource.com/platform/external/qemu/+/ae9d18d2b6261179fbd57fffec720a04f7bfb053/audio/audio.c
    return {'PULSE_SERVER': server, 'QEMU_PA_SERVER': server,
            'QEMU_AUDIO_DRV': 'pa', 'PULSE_SINK': SINK, 'QEMU_PA_SINK': SINK,
            'QEMU_AUDIO_DAC_FIXED_SETTINGS': '1', 'QEMU_AUDIO_DAC_FIXED_FREQ': '48000',
            'QEMU_AUDIO_DAC_FIXED_FMT': 's16', 'QEMU_AUDIO_DAC_FIXED_CHANNELS': '2'}


def pulse_library_version() -> str:
    # Check the installed Pulse runtime used by the GUI emulator executable.
    # The headless executable has PA stubs and cannot validate real output.
    try:
        library = ctypes.CDLL('libpulse.so.0')
    except OSError as error:
        raise RuntimeError('PulseAudio runtime libpulse.so.0 is unavailable') from error
    library.pa_get_library_version.argtypes = []
    library.pa_get_library_version.restype = ctypes.c_char_p
    return library.pa_get_library_version().decode('utf-8', errors='replace')


def pactl(*args: str) -> str:
    result = subprocess.run(['pactl', *args], capture_output=True, text=True,
                            timeout=5, check=True, env={**os.environ, 'LC_ALL': 'C'})
    return result.stdout.strip()


def local_server() -> str:
    # Read the server actually reached by the authenticated Pulse client. The
    # cookie and unrelated server-info fields are not written to diagnostics.
    info = pactl('info')
    server = next((line.partition(':')[2].strip() for line in info.splitlines()
                   if line.startswith('Server String:')), '')
    socket = server.removeprefix('unix:')
    if not socket.startswith('/') or not Path(socket).is_socket():
        raise RuntimeError('PulseAudio must expose an existing authenticated local Unix socket')
    return 'unix:' + socket


def configure(output: Path) -> dict:
    library_version = pulse_library_version()
    server = local_server()
    # AOSP audio_process_options("pa", ...) prefixes SERVER/SINK with QEMU_PA_.
    environment = expected_environment(server)
    environment_file = os.environ.get('GITHUB_ENV')
    if not environment_file:
        raise RuntimeError('--configure requires the workflow GITHUB_ENV file')
    pactl('set-sink-volume', SINK, '100%')
    pactl('set-sink-mute', SINK, '0')
    with Path(environment_file).open('a') as target:
        for key, value in environment.items():
            if '\n' in value or '\r' in value:
                raise ValueError('Invalid multiline Pulse environment value')
            target.write(f'{key}={value}\n')
    report = {'success': True, 'environment': environment, 'libpulse_version': library_version,
              'transport': 'Existing authenticated Unix socket; no TCP listener'}
    (output / 'pulse-setup.json').write_text(json.dumps(report, indent=2) + '\n')
    return report


def routing_snapshot() -> dict:
    return {kind: json.loads(pactl('--format=json', 'list', kind))
            for kind in ('sinks', 'clients', 'sink-inputs')}


def emulator_process(properties: dict) -> bool:
    def matching_binary(value: str) -> bool:
        binary = str(value).rsplit('/', 1)[-1].casefold()
        return binary == 'emulator' or binary.startswith('qemu-system-')

    # AOSP paaudio.c passes glob_paaudio.server to pa_context_new, so the
    # application name may be the Unix socket address. It is not identity proof.
    if matching_binary(properties.get('application.process.binary', '')):
        return True
    pid = str(properties.get('application.process.id', ''))
    if not pid.isascii() or not pid.isdecimal() or int(pid) <= 0:
        return False
    try:
        return matching_binary(os.readlink('/proc/' + pid + '/exe'))
    except OSError:
        return False


def emulator_streams(snapshot: dict) -> list[dict]:
    clients = {str(client['index']): client for client in snapshot['clients']}
    streams = []
    for stream in snapshot['sink-inputs']:
        client = clients.get(str(stream.get('client')), {})
        properties = {**client.get('properties', {}), **stream.get('properties', {})}
        if emulator_process(properties):
            streams.append(stream)
    return streams


def preflight(output: Path, timeout: float = 15.0) -> dict:
    report = {'success': False, 'source': 'Actual PulseAudio host playback routing after Android app launch',
              'scope': 'Connection and sink routing; actual audibility is checked by emulator-audio-smoke.py'}
    try:
        server = local_server()
        expected = expected_environment(server)
        report['environment'] = {key: os.environ.get(key) for key in expected}
        if report['environment'] != expected:
            raise RuntimeError('Emulator and capture must inherit the configured explicit Pulse server and sink')
        deadline = time.monotonic() + timeout
        while True:
            snapshot = routing_snapshot()
            report['snapshot'] = snapshot
            sinks = [sink for sink in snapshot['sinks'] if sink.get('name') == SINK]
            if len(sinks) != 1:
                raise RuntimeError('Dedicated emulator Pulse sink is missing or ambiguous')
            sink = sinks[0]
            streams = emulator_streams(snapshot)
            report['emulator_stream_ids'] = [stream['index'] for stream in streams]
            routed = [stream for stream in streams if str(stream.get('sink')) == str(sink['index'])]
            if routed:
                if sink.get('mute') or any(stream.get('mute') for stream in routed):
                    raise RuntimeError('Emulator host playback is muted')
                if sink.get('sample_specification') != PCM_SPEC or any(
                        stream.get('sample_specification') != PCM_SPEC for stream in routed):
                    raise RuntimeError('Emulator playback and capture sink must both use s16le stereo 48 kHz')
                report['matched_sink_index'] = sink['index']
                report['matched_stream_ids'] = [stream['index'] for stream in routed]
                report['pcm_sample_specification'] = PCM_SPEC
                report['success'] = True
                return report
            if time.monotonic() >= deadline:
                raise RuntimeError('No emulator/QEMU playback stream reaches the dedicated Pulse sink; check host audio initialization')
            time.sleep(0.5)
    except Exception as error:
        report['error'] = str(error)
        raise
    finally:
        (output / 'pulse-routing.json').write_text(json.dumps(report, indent=2) + '\n')


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--configure', action='store_true')
    parser.add_argument('--out', required=True, type=Path)
    args = parser.parse_args()
    output = args.out.resolve()
    output.mkdir(parents=True, exist_ok=True)
    result = configure(output) if args.configure else preflight(output)
    print(json.dumps({'checkpoint': 'pulse-setup' if args.configure else 'emulator-host-routing',
                      'success': result['success']}), flush=True)


if __name__ == '__main__':
    main()
