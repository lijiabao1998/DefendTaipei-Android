#!/usr/bin/env python3
"""Regenerate one periodic armor.ts engine motif without touching existing audio."""
from pathlib import Path
import hashlib
import json
import math
import subprocess
import numpy as np
from generate_audio import SR, OUT, write_ogg, norm

# Web Audio's default low-pass Q is 1/sqrt(2). Warm the biquad on an integer
# number of periods before retaining a 2-second loop (38 and 57 Hz both divide).
duration = 2
t = np.arange(SR * 4) / SR
saw = 2.0 * ((t * 38.0) % 1.0) - 1.0
square = np.where(np.sin(2.0 * math.pi * 57.0 * t) >= 0.0, 1.0, -1.0)
x = saw + square
w = 2.0 * math.pi * 220.0 / SR
alpha = math.sin(w) / (2.0 / math.sqrt(2.0))
a0 = 1.0 + alpha
b0 = (1.0 - math.cos(w)) / 2.0 / a0
b1 = (1.0 - math.cos(w)) / a0
b2 = b0
a1 = -2.0 * math.cos(w) / a0
a2 = (1.0 - alpha) / a0
y = np.zeros_like(x)
for i in range(2, len(x)):
    y[i] = b0*x[i] + b1*x[i-1] + b2*x[i-2] - a1*y[i-1] - a2*y[i-2]
samples = norm(y[-duration*SR:], 0.72)
path = OUT / "armor_engine.ogg"
write_ogg(path, samples, "40k", "Armored vehicle engine")
pcm = np.frombuffer(subprocess.run(["ffmpeg", "-v", "error", "-i", str(path), "-f", "f32le", "-ac", "1", "-ar", str(SR), "pipe:1"], capture_output=True, check=True).stdout, dtype=np.float32)
if abs(float(pcm[0]-pcm[-1])) > 0.08:
    raise RuntimeError("Decoded loop seam exceeds the existing 0.08 continuity limit")
entry = {"path": "audio/armor_engine.ogg", "duration": len(pcm)/SR,
         "bytes": path.stat().st_size, "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
         "sample_rate": SR, "channels": 1, "decoded_peak": float(np.max(np.abs(pcm))),
         "decoded_rms": float(np.sqrt(np.mean(pcm*pcm))), "license": "CC0-1.0",
         "source": "tools/generate_armor_engine.py; Web src/systems/armor.ts engine()"}
manifest_path = OUT / "manifest.json"
manifest = json.loads(manifest_path.read_text())
manifest["clips"] = sorted([e for e in manifest["clips"] if e["path"] != entry["path"]]+[entry], key=lambda e: e["path"])
manifest_path.write_text(json.dumps(manifest, indent=2)+"\n")
print(json.dumps({**entry, "loop_boundary_difference": abs(float(pcm[0]-pcm[-1]))}))
