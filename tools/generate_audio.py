#!/usr/bin/env python3
"""CC0 stand-ins for Defend Taipei's procedural WebAudio cues.

The original `src/audio/audio.ts` and `src/story/broadcasts/broadcast.ts` synthesize
these at runtime (noise bursts, oscillators, siren rotors, radio hiss). This script
renders the same cues to small mono OGG files so Bevy can play them. No third-party
samples. Public domain under CC0 1.0.

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


def lowpass(x: np.ndarray, coef: float) -> np.ndarray:
    y = np.empty_like(x)
    acc = 0.0
    c = float(coef)
    for i, s in enumerate(x):
        acc += c * (float(s) - acc)
        y[i] = acc
    return y


def env_burst(n: int, attack: float, decay: float, delay: float = 0.0) -> np.ndarray:
    t = np.arange(n) / SR
    e = np.zeros(n)
    on = t >= delay
    u = np.clip((t - delay) / max(attack, 1e-4), 0.0, 1.0)
    e[on] = np.minimum(u[on], np.exp(-(t[on] - delay) / max(decay, 1e-3)))
    return e


def fade_ends(x: np.ndarray, a: int, b: int) -> np.ndarray:
    y = x.copy()
    if a:
        y[:a] *= np.linspace(0.0, 1.0, a)
    if b:
        y[-b:] *= np.linspace(1.0, 0.0, b)
    return y


def norm(x: np.ndarray, peak: float) -> np.ndarray:
    m = float(np.max(np.abs(x))) or 1.0
    return (x * (peak / m)).astype(np.float32)


def tone(n: int, f0: float, f1: float, decay: float, delay: float = 0.0) -> np.ndarray:
    t = np.arange(n) / SR
    f = f0 * (f1 / max(f0, 1.0)) ** np.clip((t - delay) / max(decay, 1e-3), 0.0, 1.0)
    phase = 2.0 * math.pi * np.cumsum(f) / SR
    return np.sin(phase) * env_burst(n, 0.004, decay, delay)


def noise_burst(rng: np.random.Generator, n: int, hz_coef: float, decay: float, delay: float = 0.0) -> np.ndarray:
    raw = rng.standard_normal(n)
    band = raw - lowpass(raw, hz_coef)
    return band * env_burst(n, 0.003, decay, delay)


def write_ogg(path: Path, samples: np.ndarray, bitrate: str, title: str) -> None:
    if sys.byteorder != "little":
        raise RuntimeError("little-endian host required")
    path.parent.mkdir(parents=True, exist_ok=True)
    cmd = [
        "ffmpeg", "-y", "-v", "error", "-f", "f32le", "-ar", str(SR), "-ac", "1", "-i", "pipe:0",
        "-c:a", "libvorbis", "-b:a", bitrate, "-ar", str(SR), "-ac", "1",
        "-metadata", f"TITLE={title}",
        "-metadata", "ARTIST=DefendTaipei",
        "-metadata", "COMMENT=Original procedural audio, CC0-1.0, ported from the web build's synthesizer.",
        "-metadata", "LICENSE=CC0-1.0",
        str(path),
    ]
    proc = subprocess.run(cmd, input=np.ascontiguousarray(samples).tobytes(), capture_output=True)
    if proc.returncode != 0:
        raise RuntimeError(proc.stderr.decode()[:1500])


def loopify(src: np.ndarray, fade_n: int) -> np.ndarray:
    n = len(src) - fade_n
    y = src[:n].copy()
    y[:fade_n] = src[:fade_n] * np.linspace(0.0, 1.0, fade_n) + src[n : n + fade_n] * np.linspace(1.0, 0.0, fade_n)
    return y


def gun(kind: str) -> np.ndarray:
    # Crack / body / duration from audio.ts gunshot() fallback table.
    spec = {
        "pistol": (2600, 160, 0.12, 0.55),
        "rifle": (2000, 120, 0.20, 0.75),
        "sniper": (1500, 80, 0.45, 1.0),
        "mg": (1800, 110, 0.16, 0.70),
        "shotgun": (900, 90, 0.30, 0.90),
        "at": (500, 50, 0.90, 1.0),
    }[kind]
    crack, body, dur, vol = spec
    n = int(SR * (dur * 1.8 + 0.08))
    rng = np.random.default_rng(abs(hash(kind)) % 10_000)
    sig = 0.9 * noise_burst(rng, n, min(0.55, 800 / crack), dur * 0.5)
    sig += 0.7 * noise_burst(rng, n, 0.12, dur * 1.6)
    sig += 0.8 * vol * tone(n, body * 1.8, body * 0.5, dur)
    return norm(fade_ends(np.tanh(sig), 4, int(SR * 0.02)), 0.9)


def step(surface: str) -> np.ndarray:
    n = int(SR * 0.14)
    rng = np.random.default_rng({"tile": 1, "asphalt": 2, "wood": 3, "metro": 4}[surface])
    if surface == "tile":
        sig = noise_burst(rng, n, 0.55, 0.04)
    elif surface == "asphalt":
        sig = noise_burst(rng, n, 0.18, 0.07)
    elif surface == "wood":
        sig = 0.8 * tone(n, 190, 120, 0.08) + 0.4 * noise_burst(rng, n, 0.3, 0.03)
    else:
        sig = noise_burst(rng, n, 0.62, 0.12) + 0.5 * noise_burst(rng, n, 0.5, 0.04)
    return norm(fade_ends(sig, 2, int(SR * 0.015)), 0.75)


def hurt() -> np.ndarray:
    n = int(SR * 0.28)
    rng = np.random.default_rng(11)
    sig = 0.8 * tone(n, 120, 55, 0.18) + 0.25 * noise_burst(rng, n, 0.28, 0.22)
    return norm(fade_ends(sig, 3, int(SR * 0.02)), 0.8)


def death() -> np.ndarray:
    n = int(SR * 0.55)
    rng = np.random.default_rng(12)
    sig = tone(n, 100, 40, 0.35)
    sig += 0.6 * noise_burst(rng, n, 0.1, 0.3, 0.22)
    sig += 0.15 * noise_burst(rng, n, 0.5, 0.1, 0.3)
    return norm(fade_ends(sig, 4, int(SR * 0.03)), 0.85)


def swish() -> np.ndarray:
    n = int(SR * 0.2)
    rng = np.random.default_rng(13)
    sig = noise_burst(rng, n, 0.35, 0.16)
    return norm(fade_ends(sig, 8, int(SR * 0.02)), 0.6)


def melee() -> np.ndarray:
    n = int(SR * 0.16)
    rng = np.random.default_rng(14)
    sig = tone(n, 180, 60, 0.12) + 0.5 * noise_burst(rng, n, 0.22, 0.09)
    return norm(fade_ends(sig, 3, int(SR * 0.02)), 0.8)


def radio() -> np.ndarray:
    n = int(SR * 0.7)
    rng = np.random.default_rng(15)
    sig = noise_burst(rng, n, 0.45, 0.12)
    sig += 0.35 * tone(n, 1250, 1250, 0.06, 0.14)
    sig += 0.35 * tone(n, 1550, 1550, 0.06, 0.24)
    return norm(fade_ends(sig, 4, int(SR * 0.04)), 0.55)


def clink() -> np.ndarray:
    n = int(SR * 0.22)
    sig = tone(n, 2600, 1900, 0.07) + 0.5 * tone(n, 2300, 1700, 0.05, 0.12)
    return norm(fade_ends(sig, 2, int(SR * 0.02)), 0.55)


def rummage() -> np.ndarray:
    n = int(SR * 0.85)
    rng = np.random.default_rng(16)
    sig = np.zeros(n)
    for i in range(6):
        sig += 0.4 * noise_burst(rng, n, 0.4, 0.07, i * 0.12)
    return norm(fade_ends(sig, 4, int(SR * 0.03)), 0.45)


def alert_test() -> np.ndarray:
    n = int(SR * 1.6)
    sig = np.zeros(n)
    for i, hz in enumerate((880.0, 660.0, 880.0)):
        sig += tone(n, hz, hz, 0.4, i * 0.45)
    return norm(fade_ends(sig, 8, int(SR * 0.05)), 0.45)


def siren_loop() -> np.ndarray:
    """Two detuned saw-like rotors, seamless so the 萬安 schedule can loop it."""
    dur = 2.0
    n = int(SR * dur)
    extra = int(SR * 0.05)
    t = np.arange(n + extra) / SR
    # One rise across the loop, integer cycles of the vibrato.
    rise = 110 * (560 / 110) ** ((t % dur) / dur)
    vib = 8 * np.sin(2 * math.pi * (4 / dur) * t)
    ph1 = 2 * math.pi * np.cumsum(rise + vib) / SR
    ph2 = 2 * math.pi * np.cumsum(rise * 1.19) / SR
    sig = np.tanh(1.4 * np.sin(ph1) + 0.9 * np.sin(ph2))
    y = loopify(sig, extra)
    return norm(y, 0.7)


def all_clear() -> np.ndarray:
    n = int(SR * 8.0)
    sig = tone(n, 520, 140, 7.5)
    return norm(fade_ends(sig, int(SR * 0.4), int(SR * 0.8)), 0.55)


def whistle() -> np.ndarray:
    n = int(SR * 1.6)
    t = np.arange(n) / SR
    f = 1700 * np.exp(-t * 0.55)
    ph = 2 * math.pi * np.cumsum(f + 14 * np.sin(2 * math.pi * 7 * t)) / SR
    sig = np.sin(ph) * np.sin(np.pi * np.clip(t / 1.6, 0, 1))
    return norm(fade_ends(sig, 20, int(SR * 0.05)), 0.45)


def boom() -> np.ndarray:
    n = int(SR * 1.8)
    rng = np.random.default_rng(17)
    sig = noise_burst(rng, n, 0.08, 1.4) + 0.8 * tone(n, 90, 30, 1.2)
    return norm(fade_ends(np.tanh(sig), 6, int(SR * 0.08)), 0.9)


def amb_explosion() -> np.ndarray:
    n = int(SR * 2.2)
    rng = np.random.default_rng(18)
    sig = noise_burst(rng, n, 0.06, 2.0) + 0.7 * tone(n, 50, 28, 1.6)
    return norm(fade_ends(sig, 8, int(SR * 0.1)), 0.7)


def amb_dog() -> np.ndarray:
    n = int(SR * 1.1)
    sig = np.zeros(n)
    for i in range(3):
        sig += tone(n, 520, 300, 0.12, i * 0.32)
    return norm(fade_ends(sig, 4, int(SR * 0.03)), 0.45)


def amb_scooter() -> np.ndarray:
    n = int(SR * 3.4)
    t = np.arange(n) / SR
    f = np.interp(t, [0, 1.6, 3.4], [95, 130, 80])
    ph = 2 * math.pi * np.cumsum(f) / SR
    g = np.exp(-((t - 1.6) ** 2) / 0.8)
    sig = np.tanh(1.5 * np.sin(ph)) * g
    return norm(fade_ends(sig, 10, int(SR * 0.08)), 0.4)


def amb_chime() -> np.ndarray:
    n = int(SR * 1.4)
    sig = np.zeros(n)
    for i, f in enumerate((784.0, 659.0, 523.0)):
        sig += tone(n, f, f, 0.45, i * 0.35)
    return norm(fade_ends(sig, 6, int(SR * 0.05)), 0.4)


def bed(kind: str) -> np.ndarray:
    dur = 4.0
    n = int(SR * dur)
    extra = int(SR * 0.08)
    rng = np.random.default_rng({"rain": 21, "room": 22, "radio": 23}[kind])
    white = rng.standard_normal(n + extra)
    if kind == "rain":
        sig = lowpass(white, 0.35) - lowpass(white, 0.04)
    elif kind == "room":
        t = np.arange(n + extra) / SR
        sig = 0.2 * np.sin(2 * math.pi * 58 * t) + (lowpass(white, 0.12) - lowpass(white, 0.04))
    else:
        sig = lowpass(white, 0.45) - lowpass(white, 0.08)
    return norm(loopify(sig, extra), 0.5)


def main() -> None:
    clips: list[tuple[str, np.ndarray, str, str]] = []
    for kind in ("pistol", "rifle", "sniper", "mg", "shotgun", "at"):
        clips.append((f"gun_{kind}.ogg", gun(kind), "48k", f"Gun {kind}"))
    for surface in ("tile", "asphalt", "wood", "metro"):
        clips.append((f"step_{surface}.ogg", step(surface), "32k", f"Step {surface}"))
    clips += [
        ("hurt.ogg", hurt(), "32k", "Hurt"),
        ("death.ogg", death(), "40k", "Death"),
        ("swish.ogg", swish(), "32k", "Swish"),
        ("melee.ogg", melee(), "32k", "Melee"),
        ("radio.ogg", radio(), "32k", "Radio"),
        ("clink.ogg", clink(), "32k", "Clink"),
        ("rummage.ogg", rummage(), "32k", "Rummage"),
        ("alert_test.ogg", alert_test(), "40k", "Alert test"),
        ("siren.ogg", siren_loop(), "48k", "Siren rotor"),
        ("all_clear.ogg", all_clear(), "40k", "All clear"),
        ("whistle.ogg", whistle(), "32k", "Shell whistle"),
        ("boom.ogg", boom(), "48k", "Shell blast"),
        ("amb_explosion.ogg", amb_explosion(), "40k", "Distant blast"),
        ("amb_dog.ogg", amb_dog(), "32k", "Dog"),
        ("amb_scooter.ogg", amb_scooter(), "32k", "Scooter"),
        ("amb_chime.ogg", amb_chime(), "32k", "Loudspeaker chime"),
        ("rain.ogg", bed("rain"), "28k", "Rain"),
        ("room.ogg", bed("room"), "24k", "Room tone"),
        ("radio_bed.ogg", bed("radio"), "24k", "Radio bed"),
    ]
    # Drop files from the previous pack that this set replaces.
    keep = {name for name, _, _, _ in clips}
    if OUT.exists():
        for old in OUT.glob("*.ogg"):
            if old.name not in keep:
                old.unlink()
    total = 0
    for name, samples, bitrate, title in clips:
        path = OUT / name
        write_ogg(path, samples, bitrate, title)
        total += path.stat().st_size
        print(f"{name} {path.stat().st_size}")
    decoded = subprocess.run(
        ["ffmpeg", "-v", "error", "-i", str(OUT / "siren.ogg"), "-f", "f32le", "-ac", "1", "-ar", str(SR), "pipe:1"],
        capture_output=True,
        check=True,
    )
    pcm = np.frombuffer(decoded.stdout, dtype=np.float32)
    jump = abs(float(pcm[0] - pcm[-1]))
    if jump > 0.08:
        raise RuntimeError(f"siren loop step {jump}")
    if total > 3 * 1024 * 1024:
        raise RuntimeError(f"audio {total} bytes exceeds 3 MB")
    print(f"total_bytes {total} files {len(clips)}")


if __name__ == "__main__":
    main()
