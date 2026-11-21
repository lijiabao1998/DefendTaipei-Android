#!/usr/bin/env python3
"""Synthesize DefendTaipei's CC0 OGG sound effects.

Original work, public domain under CC0 1.0. No third-party samples.
Deterministic. Requires numpy and ffmpeg (libvorbis).

    python3 tools/generate_audio.py
"""

from __future__ import annotations

import json
import math
import subprocess
import sys
from pathlib import Path

import numpy as np

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "assets" / "audio"
SR = 22050


def lowpass(samples: np.ndarray, coef: float) -> np.ndarray:
    out = np.empty_like(samples)
    acc = 0.0
    c = float(coef)
    for i, sample in enumerate(samples):
        acc += c * (float(sample) - acc)
        out[i] = acc
    return out


def fade(samples: np.ndarray, fade_in: int, fade_out: int) -> np.ndarray:
    out = samples.copy()
    if fade_in > 0:
        out[:fade_in] *= np.linspace(0.0, 1.0, fade_in)
    if fade_out > 0:
        out[-fade_out:] *= np.linspace(1.0, 0.0, fade_out)
    return out


def normalize(samples: np.ndarray, peak: float) -> np.ndarray:
    m = float(np.max(np.abs(samples)))
    if m < 1e-8:
        return samples.astype(np.float32)
    return (samples * (peak / m)).astype(np.float32)


def write_ogg(path: Path, samples: np.ndarray, bitrate: str, title: str) -> None:
    if sys.byteorder != "little":
        raise RuntimeError("expected a little-endian host")
    path.parent.mkdir(parents=True, exist_ok=True)
    cmd = [
        "ffmpeg",
        "-y",
        "-v",
        "error",
        "-f",
        "f32le",
        "-ar",
        str(SR),
        "-ac",
        "1",
        "-i",
        "pipe:0",
        "-c:a",
        "libvorbis",
        "-b:a",
        bitrate,
        "-ar",
        str(SR),
        "-ac",
        "1",
        "-metadata",
        f"TITLE={title}",
        "-metadata",
        "ARTIST=DefendTaipei",
        "-metadata",
        "COMMENT=Original procedural audio. Dedicated to the public domain under CC0 1.0.",
        "-metadata",
        "LICENSE=CC0-1.0",
        str(path),
    ]
    proc = subprocess.run(cmd, input=np.ascontiguousarray(samples).tobytes(), capture_output=True)
    if proc.returncode != 0:
        raise RuntimeError(proc.stderr.decode()[:2000])


def synth_gunshot() -> np.ndarray:
    n = int(SR * 0.42)
    t = np.arange(n) / SR
    rng = np.random.default_rng(7)
    noise = rng.standard_normal(n)
    crack = noise - lowpass(noise, 0.22)
    body = lowpass(noise, 0.08)
    freq = 190.0 * np.exp(-t * 14.0) + 48.0
    phase = 2.0 * math.pi * np.cumsum(freq) / SR
    signal = 0.85 * np.sin(phase) * np.exp(-t * 22.0)
    signal += 0.62 * crack * np.exp(-t * 14.0) + 0.28 * body * np.exp(-t * 18.0)
    signal += 0.4 * (noise - lowpass(noise, 0.72)) * np.exp(-t * 90.0)
    for delay_ms, gain in ((23, 0.22), (41, 0.11), (68, 0.05)):
        delay = int(SR * delay_ms / 1000.0)
        signal[delay:] += gain * signal[:-delay]
    signal = np.tanh(signal * 1.35)
    return normalize(fade(signal, 8, int(SR * 0.03)), 0.9)


def synth_footstep(variant: int) -> np.ndarray:
    n = int(SR * 0.22)
    t = np.arange(n) / SR
    rng = np.random.default_rng(100 + variant * 17)
    noise = rng.standard_normal(n)
    if variant == 1:
        thump_hz, thump_decay, noise_decay, noise_mix, second = 92.0, 26.0, 20.0, 0.38, None
    elif variant == 2:
        thump_hz, thump_decay, noise_decay, noise_mix, second = 138.0, 34.0, 16.0, 0.72, None
    else:
        thump_hz, thump_decay, noise_decay, noise_mix, second = 78.0, 22.0, 18.0, 0.48, 0.07
    scrape = noise - lowpass(noise, 0.18)
    env = np.exp(-t * thump_decay)
    signal = 0.8 * np.sin(2.0 * math.pi * (thump_hz * t - 40.0 * t * t)) * env
    signal += noise_mix * scrape * np.exp(-t * noise_decay)
    signal += 0.15 * np.sin(2.0 * math.pi * thump_hz * t) * env
    if second is not None:
        start = int(SR * second)
        tap_t = t[: n - start]
        tap = np.sin(2.0 * math.pi * 160.0 * tap_t) * np.exp(-tap_t * 48.0)
        tap += 0.6 * scrape[: n - start] * np.exp(-tap_t * 30.0)
        signal[start:] += 0.55 * tap
    return normalize(fade(np.tanh(signal * 1.2), 4, int(SR * 0.02)), 0.82)


def synth_enemy_death() -> np.ndarray:
    """Short falling thud, distinct from the gunshot crack."""
    n = int(SR * 0.48)
    t = np.arange(n) / SR
    rng = np.random.default_rng(21)
    noise = rng.standard_normal(n)
    freq = 240.0 * np.exp(-t * 6.5) + 55.0
    phase = 2.0 * math.pi * np.cumsum(freq) / SR
    body = np.sin(phase) * np.exp(-t * 7.0)
    grit = (noise - lowpass(noise, 0.3)) * np.exp(-t * 11.0)
    signal = 0.75 * body + 0.35 * grit + 0.2 * np.sin(phase * 0.5) * np.exp(-t * 5.0)
    return normalize(fade(np.tanh(signal * 1.4), 6, int(SR * 0.04)), 0.85)


def synth_player_hurt() -> np.ndarray:
    n = int(SR * 0.18)
    t = np.arange(n) / SR
    rng = np.random.default_rng(33)
    noise = rng.standard_normal(n)
    tick = np.sin(2.0 * math.pi * 520.0 * t) * np.exp(-t * 40.0)
    smack = (noise - lowpass(noise, 0.4)) * np.exp(-t * 28.0)
    signal = 0.7 * tick + 0.55 * smack
    return normalize(fade(np.tanh(signal * 1.3), 3, int(SR * 0.02)), 0.8)


def synth_player_death() -> np.ndarray:
    n = int(SR * 0.8)
    t = np.arange(n) / SR
    rng = np.random.default_rng(44)
    noise = rng.standard_normal(n)
    freq = 360.0 * np.exp(-t * 2.4) + 70.0
    phase = 2.0 * math.pi * np.cumsum(freq) / SR
    tone = np.sin(phase) * np.exp(-t * 2.2)
    wobble = np.sin(phase * 1.5) * np.exp(-t * 3.0) * 0.35
    air = lowpass(noise, 0.12) * np.exp(-t * 3.5) * 0.25
    signal = tone + wobble + air
    return normalize(fade(np.tanh(signal * 1.2), 8, int(SR * 0.08)), 0.85)


def integer_hz(freq: float, duration: float) -> float:
    return max(1, round(freq * duration)) / duration


def synth_ambient() -> np.ndarray:
    duration = 12.0
    n = int(SR * duration)
    t = np.arange(n) / SR
    fade_n = int(SR * 0.18)
    tail = int(SR * 0.35)
    rng = np.random.default_rng(42)
    white = rng.standard_normal(n + tail)
    low = lowpass(white, 0.015)
    mid = lowpass(white, 0.20) - lowpass(white, 0.05)
    high = white - lowpass(white, 0.45)

    def loop_noise(src: np.ndarray) -> np.ndarray:
        y = src[:n].copy()
        y[:tail] = src[:tail] * np.linspace(0.0, 1.0, tail) + src[n : n + tail] * np.linspace(1.0, 0.0, tail)
        return y

    bed = 0.55 * loop_noise(low) + 0.28 * loop_noise(mid) + 0.05 * loop_noise(high)
    window = np.ones(n)
    window[:fade_n] = np.linspace(0.15, 1.0, fade_n)
    window[-fade_n:] = np.linspace(1.0, 0.15, fade_n)
    bed *= window

    def tone(freq: float, phase: float = 0.0) -> np.ndarray:
        return np.sin(2.0 * math.pi * integer_hz(freq, duration) * t + phase)

    rumble = 0.55 * tone(46.0) + 0.35 * tone(63.0, 0.6) + 0.22 * tone(81.0, 1.3)
    trem = 0.78 + 0.22 * tone(0.25, 0.4)

    def bump(center: float, width: float, freq: float) -> np.ndarray:
        env = np.exp(-0.5 * ((t - center) / width) ** 2)
        return env * tone(freq, phase=center)

    horns = 0.55 * bump(3.2, 0.16, 311.0) + 0.35 * bump(7.7, 0.20, 247.0) + 0.22 * bump(10.4, 0.14, 196.0)
    signal = bed * 0.22 + rumble * trem * 0.16 + horns * 0.05
    rms = float(np.sqrt(np.mean(signal**2)))
    signal *= 0.07 / max(rms, 1e-8)
    peak = float(np.max(np.abs(signal)))
    if peak > 0.4:
        signal *= 0.4 / peak
    return signal.astype(np.float32)


def synth_bgm() -> np.ndarray:
    """Eight-second pentatonic loop. Every tone completes whole cycles."""
    duration = 8.0
    n = int(SR * duration)
    t = np.arange(n) / SR
    # A minor pentatonic, quiet and sparse.
    notes = (220.0, 261.63, 293.66, 329.63, 392.0)
    pattern = (0, 2, 4, 2, 1, 3, 2, 0)
    signal = np.zeros(n)
    step = n // len(pattern)
    for i, degree in enumerate(pattern):
        start = i * step
        end = n if i == len(pattern) - 1 else (i + 1) * step
        local = np.arange(end - start) / SR
        freq = integer_hz(notes[degree], duration)
        env = np.sin(np.pi * np.clip(local / 0.7, 0.0, 1.0)) * np.exp(-local * 2.2)
        signal[start:end] += 0.55 * np.sin(2.0 * math.pi * freq * (t[start:end])) * env
        signal[start:end] += 0.18 * np.sin(2.0 * math.pi * freq * 2.0 * (t[start:end])) * env
    drone_f = integer_hz(110.0, duration)
    signal += 0.16 * np.sin(2.0 * math.pi * drone_f * t)
    # Equal-power dip at the seam so the lossy encode does not click.
    seam = int(SR * 0.04)
    signal[:seam] *= np.linspace(0.0, 1.0, seam)
    signal[-seam:] *= np.linspace(1.0, 0.0, seam)
    return normalize(signal, 0.35)


def probe(path: Path):
    meta = subprocess.run(
        [
            "ffprobe",
            "-v",
            "error",
            "-select_streams",
            "a:0",
            "-show_entries",
            "stream=codec_name,channels,sample_rate,duration",
            "-of",
            "json",
            str(path),
        ],
        capture_output=True,
        check=True,
    )
    stream = json.loads(meta.stdout)["streams"][0]
    decoded = subprocess.run(
        ["ffmpeg", "-v", "error", "-i", str(path), "-f", "f32le", "-ac", "1", "-ar", str(SR), "pipe:1"],
        capture_output=True,
        check=True,
    )
    if decoded.stderr:
        raise RuntimeError(decoded.stderr.decode()[:800])
    pcm = np.frombuffer(decoded.stdout, dtype=np.float32).copy()
    if pcm.size == 0 or not np.isfinite(pcm).all() or float(np.max(np.abs(pcm))) < 0.02:
        raise RuntimeError(f"{path.name} did not decode")
    if stream["codec_name"] != "vorbis" or int(stream["channels"]) != 1:
        raise RuntimeError(stream)
    return stream, pcm


def check_loop(pcm: np.ndarray, label: str) -> None:
    jump = abs(float(pcm[0] - pcm[-1]))
    typical = float(np.median(np.abs(np.diff(pcm.astype(np.float64)))))
    if jump > max(0.02, typical * 12.0):
        raise RuntimeError(f"{label} loop step {jump:.5f} vs typical {typical:.5f}")


def main() -> None:
    sounds = {
        "gunshot.ogg": (synth_gunshot(), "48k", "Gunshot"),
        "footstep_01.ogg": (synth_footstep(1), "32k", "Footstep 1"),
        "footstep_02.ogg": (synth_footstep(2), "32k", "Footstep 2"),
        "footstep_03.ogg": (synth_footstep(3), "32k", "Footstep 3"),
        "enemy_death.ogg": (synth_enemy_death(), "40k", "Enemy death"),
        "player_hurt.ogg": (synth_player_hurt(), "32k", "Player hurt"),
        "player_death.ogg": (synth_player_death(), "40k", "Player death"),
        "ambient_city.ogg": (synth_ambient(), "40k", "City ambience"),
        "bgm.ogg": (synth_bgm(), "32k", "Background music"),
    }
    decoded = {}
    total = 0
    for name, (samples, bitrate, title) in sounds.items():
        path = OUT / name
        write_ogg(path, samples, bitrate, title)
        stream, pcm = probe(path)
        if name in ("ambient_city.ogg", "bgm.ogg"):
            check_loop(pcm, name)
        decoded[name] = pcm
        total += path.stat().st_size
        print(f"{name} {path.stat().st_size}B {float(stream['duration']):.2f}s")
    n = min(len(decoded["footstep_01.ogg"]), len(decoded["footstep_02.ogg"]), int(SR * 0.18))
    corr = abs(float(np.corrcoef(decoded["footstep_01.ogg"][:n], decoded["footstep_02.ogg"][:n])[0, 1]))
    if corr > 0.85:
        raise RuntimeError(f"footsteps too similar: {corr}")
    if total > 3 * 1024 * 1024:
        raise RuntimeError(f"audio is {total} bytes")
    print(f"total_bytes {total}")


if __name__ == "__main__":
    main()
