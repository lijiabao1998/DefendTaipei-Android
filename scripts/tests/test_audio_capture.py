"""PCM acceptance-gate regressions; no audio server or subprocess is started."""

import importlib.util
from pathlib import Path
import signal
import struct
import tempfile
import unittest
from unittest import mock
import wave


SCRIPT = Path(__file__).resolve().parents[1] / "audio_capture.py"
SPEC = importlib.util.spec_from_file_location("audio_capture_tests_subject", SCRIPT)
audio = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(audio)


def pcm(left=0, right=0, seconds=1.0):
    return struct.pack("<hh", left, right) * round(48_000 * seconds)


class AudioCaptureTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.directory = Path(temporary.name)
        sleeper = mock.patch.object(audio.time, "sleep")
        sleeper.start()
        self.addCleanup(sleeper.stop)

    def write_pcm(self, name, payload):
        path = self.directory / (name + ".pcm")
        path.write_bytes(payload)
        return path

    def capture_with_pcm(self, name, payload, exited=None):
        capture = audio.AudioCapture(self.directory, existing_sink="unused-unit-test-sink")
        capture.raw_path = self.write_pcm(name, payload)
        capture.raw_file = capture.raw_path.open("ab")
        capture.log_file = (self.directory / (name + ".log")).open("wb")
        self.addCleanup(capture.raw_file.close)
        self.addCleanup(capture.log_file.close)
        recorder = mock.Mock(spec=["poll", "send_signal", "wait", "kill"])
        recorder.poll.return_value = exited
        recorder.wait.return_value = 0
        capture.recorder = recorder
        return capture, recorder

    def test_metrics_distinguish_silence_and_opposite_polarity_channels(self):
        silent = audio.measure_pcm(self.write_pcm("silence", pcm()))
        self.assertEqual(silent["seconds"], 1.0)
        self.assertEqual(silent["samples"], 96_000)
        for field in ("peak", "rms", "nonzero_samples", "clipped_fraction"):
            self.assertEqual(silent[field], 0, field)

        # The channels cancel if incorrectly averaged before energy analysis.
        signal_stats = audio.measure_pcm(self.write_pcm("signal", pcm(8192, -8192)))
        self.assertEqual(signal_stats["peak"], 0.25)
        self.assertEqual(signal_stats["rms"], 0.25)
        self.assertEqual(signal_stats["nonzero_samples"], 96_000)
        self.assertEqual(signal_stats["clipped_fraction"], 0)

    def test_full_scale_signed_pcm_is_reported_as_clipped(self):
        stats = audio.measure_pcm(self.write_pcm("clipped", pcm(32767, -32768)))
        self.assertEqual(stats["peak"], 1.0)
        self.assertGreater(stats["rms"], 0.999)
        self.assertEqual(stats["clipped_fraction"], 1.0)

    def test_empty_and_incomplete_stereo_frames_are_rejected(self):
        for name, payload in (
            ("empty", b""),
            ("partial_sample", b"\0"),
            ("missing_channel", struct.pack("<h", 8192)),
            ("truncated_sample", pcm(8192, -8192)[:-1]),
            ("truncated_frame", pcm(8192, -8192)[:-2]),
        ):
            with self.subTest(name=name), self.assertRaises(ValueError):
                audio.measure_pcm(self.write_pcm(name, payload))

    def test_exact_half_second_windows_measure_only_the_requested_segment(self):
        path = self.write_pcm("transition", pcm(seconds=0.5) + pcm(8192, -8192, 0.5))
        before = audio.measure_pcm(path, 0.0, 0.5)
        after = audio.measure_pcm(path, 0.5, 1.0)
        self.assertEqual(before["seconds"], 0.5)
        self.assertEqual(before["rms"], 0)
        self.assertEqual(after["seconds"], 0.5)
        self.assertEqual(after["samples"], 48_000)
        self.assertEqual(after["rms"], 0.25)

    def test_short_or_missing_windows_cannot_count_as_silence(self):
        path = self.write_pcm("window-bounds", pcm())
        for start, end in (
            (0.0, 0.5 - 1 / 48_000),  # Exactly one stereo frame below the minimum.
            (0.75, 1.5),             # Requested duration is long; recorded portion is not.
            (1.0, 2.0),              # No recorded samples in the requested interval.
            (0.5, 0.25),             # Reversed bounds must not be accepted as silence.
        ):
            with self.subTest(start=start, end=end), self.assertRaises(ValueError):
                audio.measure_pcm(path, start, end)

    def test_stop_accepts_matching_silence_and_signal_and_writes_actual_pcm(self):
        for name, payload, audible in (
            ("accepted-silence", pcm(), False),
            ("accepted-signal", pcm(8192, -8192), True),
        ):
            with self.subTest(name=name):
                capture, recorder = self.capture_with_pcm(name, payload)
                stats = capture.stop(expect_audible=audible)
                self.assertEqual(stats["seconds"], 1.0)
                recorder.send_signal.assert_called_once_with(signal.SIGINT)
                self.assertIsNone(capture.recorder)
                with wave.open(str(self.directory / stats["file"]), "rb") as output:
                    self.assertEqual((output.getframerate(), output.getnchannels(), output.getsampwidth()),
                                     (48_000, 2, 2))
                    self.assertEqual(output.readframes(output.getnframes()), payload)

    def test_stop_rejects_wrong_audibility_and_material_clipping(self):
        for name, payload, audible, reason in (
            ("missing-audio", pcm(), True, "Expected audible output"),
            ("mute-leak", pcm(8192, -8192), False, "Expected silence"),
            ("clipping", pcm(32767, -32768), True, "materially clipped"),
        ):
            with self.subTest(name=name):
                capture, _ = self.capture_with_pcm(name, payload)
                with self.assertRaisesRegex(AssertionError, reason):
                    capture.stop(expect_audible=audible)

    def test_short_but_aligned_audible_recording_is_not_accepted(self):
        capture, _ = self.capture_with_pcm("too-short", pcm(8192, -8192, 0.5))
        with self.assertRaisesRegex(AssertionError, "Capture too short"):
            capture.stop(expect_audible=True)

    def test_already_dead_recorder_cannot_pass_with_plausible_partial_pcm(self):
        for name, payload, audible, status in (
            ("dead-silent", pcm(), False, 0),
            ("dead-audible", pcm(8192, -8192), True, 0),
            ("failed-audible", pcm(8192, -8192), True, 2),
        ):
            with self.subTest(name=name):
                capture, recorder = self.capture_with_pcm(name, payload, exited=status)
                raw_stream, log_stream = capture.raw_file, capture.log_file
                with self.assertRaisesRegex(RuntimeError, "exited early"):
                    capture.stop(expect_audible=audible)
                recorder.send_signal.assert_not_called()
                self.assertIsNone(capture.recorder)
                self.assertTrue(raw_stream.closed)
                self.assertTrue(log_stream.closed)
                self.assertTrue(capture.raw_path.exists())
                self.assertFalse(capture.raw_path.with_suffix(".wav").exists())

    def fake_monitor(self, byte_count, dead_after=None):
        capture = audio.AudioCapture(self.directory, existing_sink="unused-unit-test-sink")
        elapsed = [0.0]
        recorder = mock.Mock(spec=["poll", "send_signal", "wait", "kill"])
        recorder.poll.side_effect = lambda: (
            2 if dead_after is not None and elapsed[0] >= dead_after else None)
        recorder.wait.return_value = 0

        def advance(seconds):
            self.assertLessEqual(seconds, 0.05)
            elapsed[0] += seconds
            capture.raw_file.truncate(byte_count(elapsed[0]))

        spawn = self.enterContext(mock.patch.object(audio.subprocess, "Popen", return_value=recorder))
        self.enterContext(mock.patch.object(audio.time, "monotonic", side_effect=lambda: 50.0 + elapsed[0]))
        self.enterContext(mock.patch.object(audio.time, "time", side_effect=lambda: 1000.0 + elapsed[0]))
        self.enterContext(mock.patch.object(audio.time, "sleep", side_effect=advance))
        self.addCleanup(capture._close_files)
        return capture, recorder, spawn

    def test_start_waits_for_advancing_silence_and_calibrates_the_pcm_clock(self):
        capture, recorder, _ = self.fake_monitor(
            lambda elapsed: max(0, round((elapsed - 2.0) * 48_000)) * 4)
        capture.start("delayed-stream")
        self.assertGreater(capture.startup_seconds, 2.5)
        self.assertLess(capture.startup_seconds, 2.7)
        self.assertGreater(capture.warmup_pcm_seconds, 0.5)
        self.assertAlmostEqual(capture.started_unix, 1002.0, places=5)
        self.assertAlmostEqual(capture.started_monotonic, 52.0, places=5)
        self.assertAlmostEqual(capture.warmup_unix - capture.started_unix,
                               capture.warmup_pcm_seconds)
        self.assertAlmostEqual(capture.warmup_monotonic - capture.started_monotonic,
                               capture.warmup_pcm_seconds)
        self.assertEqual(audio.measure_pcm(capture.raw_path)["nonzero_samples"], 0)
        recorder.kill.assert_not_called()

    def test_start_rejects_missing_frozen_and_incomplete_frame_only_growth(self):
        for name, sizes in (
            ("no-pcm", lambda _: 0),
            ("frozen", lambda elapsed: 96_000 if elapsed >= 0.1 else 0),
            ("partial-frame-growth", lambda elapsed: (
                96_002 if elapsed >= 0.2 else 96_000 if elapsed >= 0.1 else 0)),
        ):
            with self.subTest(name=name):
                capture, recorder, spawn = self.fake_monitor(sizes)
                with self.assertRaisesRegex(RuntimeError, "advancing PCM within 10 seconds"):
                    capture.start(name)
                recorder.kill.assert_called_once()
                recorder.wait.assert_called_once_with(timeout=5)
                self.assertIsNone(capture.recorder)
                self.assertIsNone(capture.started_unix)
                self.assertIsNone(capture.warmup_unix)
                self.assertTrue(spawn.call_args.kwargs["stdout"].closed)
                self.assertTrue(spawn.call_args.kwargs["stderr"].closed)

    def test_start_rejects_a_recorder_that_dies_during_warmup(self):
        capture, recorder, spawn = self.fake_monitor(lambda _: 0, dead_after=0.2)
        with self.assertRaisesRegex(RuntimeError, "monitor failed to start"):
            capture.start("dead-during-warmup")
        self.assertIsNone(capture.recorder)
        self.assertIsNone(capture.started_unix)
        self.assertIsNone(capture.warmup_unix)
        self.assertTrue(spawn.call_args.kwargs["stdout"].closed)
        self.assertTrue(spawn.call_args.kwargs["stderr"].closed)
        recorder.kill.assert_not_called()

    def timed_scene(self):
        capture = audio.AudioCapture(self.directory, existing_sink="unused-unit-test-sink")
        capture.started_unix = 1000.0
        capture.started_monotonic = 50.0
        capture.warmup_unix = 1000.5
        capture.warmup_monotonic = 50.5
        capture.warmup_pcm_seconds = 0.5
        scene = {"scenario": "resume", "started_unix_seconds": 1001.0,
                 "transition_unix_seconds": 1002.5, "finished_unix_seconds": 1005.5,
                 "runtime_seconds": 4.5}
        return capture, scene

    def test_phase_windows_stay_inside_the_scene_despite_five_second_teardown(self):
        capture, scene = self.timed_scene()
        for scenario in ("resume", "live_muted"):
            scene["scenario"] = scenario
            previous = None
            for teardown in (0.0, 5.0):
                with self.subTest(scenario=scenario, teardown=teardown):
                    windows, timing = capture._scene_windows(
                        scene, 5.3 + teardown, 1005.5 + teardown,
                        1005.6 + teardown, 55.5 + teardown)
                    self.assertEqual([entry[3] for entry in windows],
                                     [scenario != "resume", scenario == "resume"])
                    for index, (_, first, last, _) in enumerate(windows):
                        self.assertGreaterEqual(last - first, 0.5)
                        lower, upper = ((1001.0, 1002.5) if index == 0 else (1002.5, 1005.5))
                        for origin in timing["pcm_origin_unix_bounds"]:
                            self.assertLess(lower, origin + first)
                            self.assertLess(origin + first, origin + last)
                            self.assertLess(origin + last, upper)
                        if previous is not None:
                            self.assertAlmostEqual(first, previous[index][1])
                            self.assertAlmostEqual(last, previous[index][2])
                    previous = windows

    def test_uncertain_stop_and_clock_drift_cannot_prove_scene_phases(self):
        capture, scene = self.timed_scene()
        for name, seconds, finished, monotonic, runtime, reason in (
            ("slow-stop", 10.3, 1010.701, 60.5, 4.5, "too uncertain"),
            ("capture-drift", 10.3, 1010.6, 60.6, 4.5, "wall clock changed"),
            ("scene-drift", 10.3, 1010.6, 60.5, 4.2, "wall and monotonic"),
            ("missing-pcm", 8.0, 1010.6, 60.5, 4.5, "duration disagrees"),
        ):
            with self.subTest(name=name), self.assertRaisesRegex(ValueError, reason):
                capture._scene_windows({**scene, "runtime_seconds": runtime}, seconds,
                                       1010.5, finished, monotonic)

    def test_teardown_silence_cannot_replace_an_insufficient_in_scene_window(self):
        capture, scene = self.timed_scene()
        scene.update(finished_unix_seconds=1003.3, runtime_seconds=2.3)
        # Five more recorded seconds after the scene cannot validate its short phase.
        with self.assertRaisesRegex(ValueError, "Insufficient in-scene PCM"):
            capture._scene_windows(scene, 8.1, 1008.3, 1008.4, 58.3)


if __name__ == "__main__":
    unittest.main()
