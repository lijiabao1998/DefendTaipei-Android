#!/usr/bin/env python3
"""Capture actual application output from an isolated PulseAudio monitor.

The WAV is rendered output, not an offline mix of the source assets.  The
isolated sink prevents unrelated desktop sounds from satisfying assertions.
Used by the native and disposable-emulator audio smoke tests.
"""
from __future__ import annotations

import argparse
import array
import contextlib
import json
import math
import os
from pathlib import Path
import signal
import subprocess
import sys
import time
import uuid
import wave


RATE = 48000
CHANNELS = 2


def command(*args: str) -> str:
    return subprocess.check_output(args, text=True, stderr=subprocess.STDOUT).strip()


def measure_pcm(path: Path, start: float | None = None, end: float | None = None) -> dict:
    size = path.stat().st_size
    if size == 0 or size % (CHANNELS * 2) or size > RATE * CHANNELS * 2 * 180:
        raise ValueError(f"Invalid or excessive PCM capture length: {size}")
    values = array.array('h')
    values.frombytes(path.read_bytes())
    if sys.byteorder != 'little':
        values.byteswap()
    if start is not None or end is not None:
        first = max(0, round((start or 0.0) * RATE)) * CHANNELS
        last = len(values) if end is None else round(end * RATE) * CHANNELS
        values = values[first:last]
        if len(values) < RATE * CHANNELS // 2:
            raise ValueError('PCM analysis window shorter than half a second')
    n = len(values)
    peak = max(abs(v) for v in values) / 32768.0
    rms = math.sqrt(sum(float(v) * v for v in values) / n) / 32768.0
    return {'seconds': n / (RATE * CHANNELS), 'samples': n, 'peak': peak,
            'rms': rms, 'nonzero_samples': sum(v != 0 for v in values),
            'clipped_fraction': sum(abs(v) >= 32760 for v in values) / n}


def pcm_to_wav(raw: Path, output: Path) -> None:
    with wave.open(str(output), 'wb') as wav:
        wav.setnchannels(CHANNELS)
        wav.setsampwidth(2)
        wav.setframerate(RATE)
        wav.writeframes(raw.read_bytes())


class AudioCapture:
    """Own only this capture's sink, monitor process and temporary ALSA config."""
    def __init__(self, output: Path, existing_sink: str | None = None):
        self.output = output.resolve()
        self.output.mkdir(parents=True, exist_ok=True)
        self.sink = existing_sink or 'defendtaipei_qa_' + uuid.uuid4().hex[:12]
        self.existing_sink = existing_sink is not None
        self.module: str | None = None
        self.recorder: subprocess.Popen | None = None
        self.raw_file = None
        self.log_file = None
        self.raw_path: Path | None = None
        self.started_unix: float | None = None
        self.started_monotonic: float | None = None
        self.warmup_unix: float | None = None
        self.warmup_monotonic: float | None = None
        self.warmup_pcm_seconds: float | None = None
        self.startup_seconds: float | None = None
        self.env = dict(os.environ)

    def __enter__(self):
        try:
            command('pactl', 'info')
            if not self.existing_sink:
                self.module = command('pactl', 'load-module', 'module-null-sink',
                                      f'sink_name={self.sink}', f'rate={RATE}',
                                      f'channels={CHANNELS}', 'format=s16le')
            command('pactl', 'set-sink-volume', self.sink, '100%')
            command('pactl', 'set-sink-mute', self.sink, '0')
            config = self.output / 'alsa-capture.conf'
            config.write_text('</usr/share/alsa/alsa.conf>\n'
                              'pcm.!default { type pulse }\n'
                              'ctl.!default { type pulse }\n')
            self.env.update(PULSE_SINK=self.sink, ALSA_CONFIG_PATH=str(config))
            return self
        except BaseException:
            self.__exit__()
            raise

    def _close_files(self) -> None:
        for stream in (self.raw_file, self.log_file):
            if stream is not None:
                stream.close()
        self.raw_file = self.log_file = None

    def start(self, name: str) -> None:
        if self.recorder is not None:
            raise RuntimeError('Capture already running')
        self.raw_path = self.output / (name + '.pcm')
        self.started_unix = self.started_monotonic = None
        self.warmup_unix = self.warmup_monotonic = self.warmup_pcm_seconds = None
        self.startup_seconds = None
        try:
            self.raw_file = self.raw_path.open('wb')
            self.log_file = (self.output / (name + '-recorder.log')).open('wb')
            launched = time.monotonic()
            deadline = launched + 10.0
            self.recorder = subprocess.Popen(
                ['parec', '--raw', '--format=s16le', f'--rate={RATE}',
                 f'--channels={CHANNELS}', '--latency-msec=30',
                 '--device=' + self.sink + '.monitor'],
                stdout=self.raw_file, stderr=self.log_file, env=self.env)
            ready_frames = None
            while True:
                if self.recorder.poll() is not None:
                    raise RuntimeError('PulseAudio monitor failed to start')
                observed_monotonic = time.monotonic()
                if observed_monotonic >= deadline:
                    raise RuntimeError('PulseAudio monitor did not produce advancing PCM within 10 seconds')
                frames = self.raw_path.stat().st_size // (CHANNELS * 2)
                if ready_frames is not None and frames > ready_frames:
                    # PulseAudio can take seconds to start streaming silence. Do
                    # not launch the scene until at least half a second is on
                    # disk and a later observation proves the stream advances.
                    observed_unix = time.time()
                    observed_monotonic = time.monotonic()
                    self.warmup_pcm_seconds = frames / RATE
                    self.warmup_unix = observed_unix
                    self.warmup_monotonic = observed_monotonic
                    self.started_unix = observed_unix - self.warmup_pcm_seconds
                    self.started_monotonic = observed_monotonic - self.warmup_pcm_seconds
                    self.startup_seconds = observed_monotonic - launched
                    return
                if frames >= RATE // 2 and ready_frames is None:
                    ready_frames = frames
                time.sleep(0.05)
        except BaseException:
            if self.recorder is not None and self.recorder.poll() is None:
                self.recorder.kill()
                self.recorder.wait(timeout=5)
            self.recorder = None
            self._close_files()
            raise

    def stop(self, *, expect_audible: bool | None = None,
             windows: list[tuple[str, float, float, bool]] | None = None,
             scene_timing: dict | None = None) -> dict:
        if self.recorder is None or self.raw_path is None:
            raise RuntimeError('No capture running')
        time.sleep(0.3)
        stop_requested_unix = time.time()
        stop_requested_monotonic = time.monotonic()
        try:
            if self.recorder.poll() is not None:
                raise RuntimeError('Audio monitor exited early; partial capture is not proof')
            self.recorder.send_signal(signal.SIGINT)
            result = self.recorder.wait(timeout=5)
            stop_finished_unix = time.time()
            if result not in (0, -signal.SIGINT):
                raise RuntimeError(f'Audio monitor failed with status {result}')
        except subprocess.TimeoutExpired:
            self.recorder.kill()
            self.recorder.wait(timeout=5)
            raise RuntimeError('Audio monitor did not stop')
        finally:
            self._close_files()
            self.recorder = None
        stats = measure_pcm(self.raw_path)
        wav_path = self.raw_path.with_suffix('.wav')
        pcm_to_wav(self.raw_path, wav_path)
        stats['file'] = wav_path.name
        if stats['seconds'] < 1.0:
            raise AssertionError(f'Capture too short: {stats}')
        if expect_audible is True and stats['rms'] < 0.0001:
            raise AssertionError(f'Expected audible output, got silence: {stats}')
        if expect_audible is False and stats['peak'] > 2 / 32768:
            raise AssertionError(f'Expected silence, got output: {stats}')
        if stats['clipped_fraction'] > 0.001:
            raise AssertionError(f'Output is materially clipped: {stats}')
        anchored_windows = None
        if scene_timing is not None:
            if windows:
                raise ValueError('Use scene timing or end-relative windows, not both')
            anchored_windows, timing = self._scene_windows(
                scene_timing, stats['seconds'], stop_requested_unix,
                stop_finished_unix, stop_requested_monotonic)
            stats['timing'] = timing
        if windows or anchored_windows:
            stats['windows'] = []
            measured_windows = anchored_windows or [
                (label, stats['seconds'] + start, stats['seconds'] + end, audible)
                for label, start, end, audible in windows]
            for label, start, end, audible in measured_windows:
                section = measure_pcm(self.raw_path, start, end)
                section.update(name=label, start=start, end=end, expect_audible=audible)
                stats['windows'].append(section)
                if audible and section['rms'] < 0.0001:
                    raise AssertionError(f'Missing audible PCM during {label}: {section}')
                if not audible and section['peak'] > 2 / 32768:
                    raise AssertionError(f'Unexpected PCM during {label}: {section}')
        self.raw_path.unlink()
        return stats

    def _scene_windows(self, scene: dict, seconds: float, stop_requested: float,
                       stop_finished: float, stop_monotonic: float):
        """Anchor to actual scene markers, independent of application teardown.

        A 30 ms Pulse monitor is given 100 ms transport margin on both sides.
        The SIGINT acknowledgement must take <= 200 ms. Each selected PCM
        interval is trimmed by the resulting clock-origin uncertainty, so it
        lies wholly inside its scene phase even at either uncertainty bound.
        These checks apply only to native timestamped scenes, not Android or
        unit-test callers of the ordinary stop(expect_audible=...) API.
        """
        if any(value is None for value in (self.started_unix, self.started_monotonic,
                                          self.warmup_unix, self.warmup_monotonic,
                                          self.warmup_pcm_seconds)):
            raise ValueError('Timestamped scene requires an advancing PCM warmup anchor')
        start = float(scene['started_unix_seconds'])
        transition = float(scene['transition_unix_seconds'])
        finish = float(scene['finished_unix_seconds'])
        runtime = float(scene['runtime_seconds'])
        if not all(math.isfinite(v) for v in (start, transition, finish, runtime, seconds)):
            raise ValueError('Nonfinite audio scene timing')
        if not self.warmup_unix <= start < transition < finish <= stop_requested + 0.05:
            raise ValueError('Scene markers are outside the actual recording')
        if abs((finish - start) - runtime) > 0.05:
            raise ValueError('Scene wall and monotonic clocks disagree')
        if not 0.0 <= stop_finished - stop_requested <= 0.20:
            raise ValueError('Recorder stop timing is too uncertain for phase proof')
        wall_elapsed = stop_requested - self.warmup_unix
        mono_elapsed = stop_monotonic - self.warmup_monotonic
        if abs(wall_elapsed - mono_elapsed) > 0.05:
            raise ValueError('Capture wall clock changed during the scene')
        pcm_growth = seconds - self.warmup_pcm_seconds
        if pcm_growth <= 0.0 or not -0.20 <= wall_elapsed - pcm_growth <= 1.0:
            raise ValueError('Recorded PCM duration disagrees with capture clock')
        origin_low = stop_requested - seconds - 0.10
        origin_high = stop_finished - seconds + 0.10
        before = (max(start + 0.15, transition - 1.25), transition - 0.25)
        after = (transition + 0.70, finish - 0.25)
        resume = scene['scenario'] == 'resume'
        phases = [('before_transition', *before, not resume),
                  ('after_transition', *after, resume)]
        windows = []
        for label, phase_start, phase_end, audible in phases:
            first, last = phase_start - origin_low, phase_end - origin_high
            if first < 0.0 or last > seconds or last - first < 0.5:
                raise ValueError(f'Insufficient in-scene PCM for {label}')
            windows.append((label, first, last, audible))
        return windows, {'scene_started_unix_seconds': start,
                         'scene_transition_unix_seconds': transition,
                         'scene_finished_unix_seconds': finish,
                         'recorder_stop_requested_unix_seconds': stop_requested,
                         'recorder_stop_finished_unix_seconds': stop_finished,
                         'recorder_warmup_unix_seconds': self.warmup_unix,
                         'recorder_warmup_pcm_seconds': self.warmup_pcm_seconds,
                         'recorder_startup_seconds': self.startup_seconds,
                         'post_warmup_clock_seconds': wall_elapsed,
                         'post_warmup_pcm_seconds': pcm_growth,
                         'pcm_origin_unix_bounds': [origin_low, origin_high],
                         'transport_margin_seconds': 0.10,
                         'method': 'Absolute scene markers with bounded PCM clock origin; phase windows trimmed to remain inside the scene'}

    def __exit__(self, *_):
        if self.recorder is not None:
            with contextlib.suppress(Exception):
                self.stop()
        self._close_files()
        if self.module is not None:
            with contextlib.suppress(Exception):
                command('pactl', 'unload-module', self.module)
            self.module = None


SCENARIOS = {
    'showcase': True, 'muted': False, 'effects_zero': False,
    'ambience': True, 'ambience_zero': False, 'paused': False,
    'resume': True, 'title_preview': True, 'radio': True, 'radio_zero': False,
    'live_muted': True, 'live_paused': True, 'live_background': True,
    'armor_engine': True, 'armor_engine_zero': False,
    'armor_engine_live_muted': True, 'armor_engine_live_paused': True,
    'armor_engine_live_background': True,
}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', required=True, type=Path)
    parser.add_argument('--out', required=True, type=Path)
    args = parser.parse_args()
    binary = args.binary.resolve()
    report = {'success': False, 'source': 'Actual native Bevy audio output through PulseAudio monitor',
              'rate': RATE, 'channels': CHANNELS, 'scenarios': []}
    args.out.mkdir(parents=True, exist_ok=True)
    try:
        with AudioCapture(args.out) as capture:
            for name, audible in SCENARIOS.items():
                scene = args.out.resolve() / name
                scene.mkdir(exist_ok=True)
                env = dict(capture.env, DEFEND_TAIPEI_AUDIO_QA_SCENARIO=name,
                           DEFEND_TAIPEI_AUDIO_QA_OUT=str(scene))
                capture.start(name)
                with (scene / 'runtime.log').open('wb') as log:
                    subprocess.run([str(binary), '--audio-qa'], env=env, stdout=log,
                                   stderr=subprocess.STDOUT, timeout=60, check=True)
                runtime = json.loads((scene / 'audio-scene.json').read_text())
                if not runtime['success']:
                    raise AssertionError(runtime)
                timing = runtime if name.startswith(('live_', 'armor_engine_live_')) or name == 'resume' else None
                stats = capture.stop(expect_audible=audible, scene_timing=timing)
                report['scenarios'].append({'name': name, 'expect_audible': audible,
                                             'pcm': stats, 'runtime': runtime})
                print(json.dumps({'scenario': name, 'pcm': stats}), flush=True)
        report['success'] = True
    except Exception as error:
        report['error'] = str(error)
        raise
    finally:
        (args.out / 'audio-output-report.json').write_text(json.dumps(report, indent=2) + '\n')


if __name__ == '__main__':
    main()
