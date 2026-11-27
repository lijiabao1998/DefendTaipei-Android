#!/usr/bin/env python3
"""Offline WebAudio synthesis and pinned CC0 recording import for Defend Taipei.

The original `src/audio/audio.ts` and `src/story/broadcasts/broadcast.ts` synthesize
these at runtime (noise bursts, oscillators, siren rotors, radio hiss). This script
renders procedural motifs to mono OGG and imports the pinned Web CC0 recordings.
See assets/audio/CREDITS.md for the original recording authors and source links.

    python3 tools/generate_audio.py
"""

from __future__ import annotations

import argparse
import hashlib
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


def write_ogg(path: Path, samples: np.ndarray, bitrate: str, title: str, *, artist: str = "DefendTaipei", recorded: bool = False) -> None:
    if sys.byteorder != "little":
        raise RuntimeError("little-endian host required")
    path.parent.mkdir(parents=True, exist_ok=True)
    cmd = [
        "ffmpeg", "-y", "-v", "error", "-f", "f32le", "-ar", str(SR), "-ac", "1", "-i", "pipe:0",
        "-c:a", "libvorbis", "-b:a", bitrate, "-ar", str(SR), "-ac", "1",
        "-fflags", "+bitexact", "-flags:a", "+bitexact",
        "-metadata", f"TITLE={title}",
        "-metadata", f"ARTIST={artist}",
        "-metadata", "COMMENT=" + ("Derived from pinned Web CC0 recording; see CREDITS.md." if recorded else "Original procedural audio, CC0-1.0, ported from the web synthesizer or native Foley."),
        "-metadata", "LICENSE=CC0-1.0",
        str(path),
    ]
    proc = subprocess.run(cmd, input=np.ascontiguousarray(samples).tobytes(), capture_output=True)
    if proc.returncode != 0:
        raise RuntimeError(proc.stderr.decode()[:1500])
    # Vorbis can overshoot short transients even when the input PCM is bounded.
    # Measure the actual decoder output, then re-encode with adequate headroom.
    for _ in range(3):
        decoded = subprocess.run(
            ["ffmpeg", "-v", "error", "-i", str(path), "-f", "f32le", "-ac", "1", "-ar", str(SR), "pipe:1"],
            capture_output=True, check=True,
        )
        peak = float(np.max(np.abs(np.frombuffer(decoded.stdout, dtype=np.float32))))
        if peak <= .86:
            break
        samples = samples * (.82 / peak)
        subprocess.run(cmd, input=np.ascontiguousarray(samples).tobytes(), capture_output=True, check=True)
    else:
        raise RuntimeError(f"Unable to bound decoded peak for {path}")


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
    rng = np.random.default_rng(int.from_bytes(hashlib.sha256(kind.encode()).digest()[:4], "little"))
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


def extended(kind: str) -> np.ndarray:
    """Web evening/shelter motifs plus native interaction Foley; no recordings."""
    seconds = {"garbage_truck": 6.8, "temple": 7.0, "firecrackers": 6.0,
               "thunder": 5.2, "jet": 4.5, "rotor": 1.5,
               "shelter_clock": 3.3, "water": 1.2, "cook": 1.5}.get(kind, 0.8)
    n = int(SR * seconds)
    seed = int.from_bytes(hashlib.sha256(kind.encode()).digest()[:4], "little")
    rng = np.random.default_rng(seed)
    sig = np.zeros(n)
    if kind == "garbage_truck":
        phrase = [(659.25,1),(622.25,1),(659.25,1),(622.25,1),(659.25,1),(493.88,1),
                  (587.33,1),(523.25,1),(440,2),(261.63,1),(329.63,1),(440,1),
                  (493.88,2),(329.63,1),(415.3,1),(493.88,1),(523.25,2),
                  (329.63,1),(659.25,1),(622.25,1),(659.25,1),(622.25,1),
                  (659.25,1),(493.88,1),(587.33,1),(523.25,1),(440,2)]
        at = 0.0
        for hz, beats in phrase:
            sig += tone(n, hz, hz, .19 * beats * .45, at)
            sig += .16 * tone(n, hz*3, hz*3, .19 * beats * .45, at)
            at += .19 * beats
    elif kind == "temple":
        for hz, gain in [(196,.6),(523,.3),(851,.18),(1210,.1)]:
            sig += gain * tone(n, hz, hz*.995, 2.0)
        for i in range(8):
            sig += .22 * tone(n, 900, 830, .055, 1.2+i*.45)
        for hz in (110,165):
            sig += .07 * tone(n, hz, hz, 3.0, .8)
    elif kind == "firecrackers":
        at = .1
        for _ in range(46):
            at += .03 + rng.random()**2*.22
            sig += noise_burst(rng,n,.45,.025,at) * (.4+rng.random()*.6)
    elif kind == "thunder":
        sig = lowpass(rng.standard_normal(n),.04) * env_burst(n,.05,1.3)
        sig += lowpass(rng.standard_normal(n),.012) * env_burst(n,.3,2.0,.4)
    elif kind == "jet":
        t = np.arange(n)/SR
        roar = lowpass(rng.standard_normal(n),.22)
        sig = roar * np.exp(-((t-1.3)/1.15)**2)
        sig += .15*tone(n,220,90,3.2)
    elif kind == "rotor":
        for i in range(7):
            sig += lowpass(rng.standard_normal(n),.04)*env_burst(n,.01,.07,i/5.2)
    elif kind == "shelter_clock":
        for i in range(4):
            sig += noise_burst(rng,n,.7,.008,i)
    elif kind == "shelter_cup":
        sig = tone(n,3100,3000,.18)
    elif kind == "shelter_chair":
        sig = tone(n,240,180,.3)+.2*noise_burst(rng,n,.1,.25)
    elif kind == "shelter_pot":
        sig = tone(n,900,880,.3)+.5*tone(n,1210,1190,.25)
    elif kind == "door_open":
        sig = .45*tone(n,180,380,.35)+.3*noise_burst(rng,n,.08,.4)
        sig += .7*tone(n,1100,600,.04,.04)
    elif kind == "door_close":
        sig = tone(n,95,40,.16,.05)+.5*noise_burst(rng,n,.12,.08,.05)
        sig += .25*tone(n,1700,900,.045,.2)
    elif kind in ("barricade", "build"):
        for i in range(3):
            sig += tone(n,230,80,.065,i*.2)+noise_burst(rng,n,.3,.035,i*.2)
    elif kind == "unboard":
        sig = .6*tone(n,320,160,.25)+noise_burst(rng,n,.15,.22)
        sig += tone(n,2200,1300,.06,.4)
    elif kind in ("pickup", "equip", "heal", "reload"):
        count = 3 if kind == "reload" else 2
        for i in range(count):
            sig += noise_burst(rng,n,.45,.06,.08+i*.18)
            sig += (.7 if kind == "equip" else .25)*tone(n,1400,600,.05,.1+i*.18)
    elif kind in ("drink", "water", "cook"):
        sig = lowpass(rng.standard_normal(n),.16)*env_burst(n,.1,.4)
        for i in range(6):
            sig += .4*tone(n,320+i*60,140+i*20,.045,i*.11)
    elif kind == "eat":
        for i in range(4):
            sig += noise_burst(rng,n,.22,.06,i*.15)
    elif kind == "melee_blunt":
        sig = tone(n,140,60,.12)+.45*lowpass(rng.standard_normal(n),.08)*env_burst(n,.003,.09)
    elif kind == "swish_heavy":
        sig = lowpass(rng.standard_normal(n),.18)*env_burst(n,.06,.12)
    elif kind == "ui_confirm":
        sig = tone(n,660,660,.055)+.6*tone(n,880,880,.07,.08)
    elif kind == "ui_open":
        sig = tone(n,560,470,.045)
    elif kind == "denied":
        sig = tone(n,220,180,.08)+.6*tone(n,180,140,.08,.15)
    elif kind == "dry_fire":
        sig = noise_burst(rng,n,.65,.012)+.25*tone(n,1500,800,.02)
    else:
        raise ValueError(kind)
    return norm(fade_ends(sig, 8, min(int(SR*.06),n//4)), .72)


def import_recordings(web: Path) -> list[dict]:
    """Import the pinned Web recordings, preserving source hashes and credits."""
    result = []
    for family in ("gun", "boom"):
        files = sorted((web / "public/sfx" / family).glob("*.mp3"))
        expected = 12 if family == "gun" else 3
        if len(files) != expected:
            raise RuntimeError(f"Expected {expected} pinned {family} samples, found {len(files)}")
        for source in files:
            pcm = subprocess.run(["ffmpeg","-v","error","-i",str(source),"-f","f32le",
                                  "-ar",str(SR),"-ac","1","pipe:1"],capture_output=True,check=True)
            dry = np.frombuffer(pcm.stdout,dtype=np.float32).copy()
            for room in ("dry", "room", "far"):
                samples = dry
                if room != "dry":
                    coef = .35 if room == "room" else .12
                    samples = lowpass(dry,coef)
                    tail = .5 if room == "room" else .8
                    padded = np.pad(samples,(0,int(SR*tail)))
                    for delay,gain in ([(.045,.35),(.09,.12),(.135,.04)] if room == "room"
                                       else [(.2,.25),(.36,.11),(.56,.04)]):
                        at = int(SR*delay)
                        padded[at:at+len(samples)] += samples*gain
                    samples = padded
                name = f"rec_{source.stem}_{room}.ogg"
                write_ogg(OUT/name,norm(fade_ends(samples,4,int(SR*.025)),.78),"56k",f"Web {source.stem} {room}", artist=("Ben Jaszczak, Brian Nelson, Kevin Heras, Matthew Nanney" if family == "gun" else "rubberduck"), recorded=True)
                result.append({"path":f"audio/{name}","source":str(source.relative_to(web)),
                               "source_sha256":hashlib.sha256(source.read_bytes()).hexdigest(),
                               "license":"CC0-1.0", "variant":room})
    return result


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--web-root",type=Path,default=ROOT.parent/"defendtaipei-web")
    args = parser.parse_args()
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
    for kind in ("garbage_truck","temple","firecrackers","thunder","jet","rotor",
                 "shelter_clock","shelter_cup","shelter_chair","shelter_pot",
                 "door_open","door_close","barricade","unboard","pickup","equip",
                 "heal","reload","drink","water","cook","eat","build",
                 "melee_blunt","swish_heavy","ui_confirm","ui_open","denied","dry_fire"):
        clips.append((kind+".ogg",extended(kind),"40k",kind.replace("_"," ")))
    clips.append(("rain_indoor.ogg",norm(lowpass(bed("rain"),.09),.5),"28k","Indoor rain"))
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
    recorded = import_recordings(args.web_root)
    source_by_path = {entry["path"]:entry for entry in recorded}
    manifest = []
    for path in sorted(OUT.glob("*.ogg")):
        decoded = subprocess.run(["ffmpeg","-v","error","-i",str(path),"-f","f32le","-ac","1",
                                  "-ar",str(SR),"pipe:1"],capture_output=True,check=True)
        pcm = np.frombuffer(decoded.stdout,dtype=np.float32)
        if len(pcm) == 0 or not np.isfinite(pcm).all() or np.max(np.abs(pcm)) < .001:
            raise RuntimeError(f"Invalid/silent decoded clip: {path.name}")
        entry = {"path":"audio/"+path.name,"duration":len(pcm)/SR,"bytes":path.stat().st_size,
                 "sha256":hashlib.sha256(path.read_bytes()).hexdigest(),"sample_rate":SR,
                 "channels":1,"decoded_peak":float(np.max(np.abs(pcm))),
                 "decoded_rms":float(np.sqrt(np.mean(pcm**2))),"license":"CC0-1.0"}
        entry.update(source_by_path.get(entry["path"],{"source":"tools/generate_audio.py"}))
        manifest.append(entry)
    total = sum(entry["bytes"] for entry in manifest)
    (OUT/"manifest.json").write_text(json.dumps({"web_commit":"b9b0a2736642b37dd45925a4719dfef08b8f36b4",
                        "sample_rate":SR,"clips":manifest},ensure_ascii=False,indent=2)+"\n")
    subprocess.run([sys.executable, str(Path(__file__).with_name("generate_armor_engine.py"))], check=True)
    manifest = json.loads((OUT / "manifest.json").read_text())["clips"]
    total = sum(entry["bytes"] for entry in manifest)
    if total > 5 * 1024 * 1024:
        raise RuntimeError(f"audio {total} bytes exceeds 5 MB")
    print(f"total_bytes {total} files {len(manifest)}")


if __name__ == "__main__":
    main()
