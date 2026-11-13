"""Offline render harness: the same Engine, 256-frame blocks, mono 48 kHz WAV.

Output is 16-bit PCM (stdlib ``wave`` cannot write float; the engine's tanh
already bounds the signal to [-1, 1], so clipping here is only a safety net).
Usage: python -m nomad.render --demo NAME out.wav [--seconds S]
"""
from __future__ import annotations

import argparse
import sys
import wave
from pathlib import Path

import numpy as np

from nomad.engine.engine import Engine

SR = 48000
BLOCK = 256
TAIL_S = 1.0  # extra time after the last note-off so releases ring out


def _chord(eng, t, seconds):
    if t == 0:
        for n in (48, 55, 60, 64, 67):
            eng.note_on(n, 0.8)
    return (48, 55, 60, 64, 67)


def _sweep(eng, t, seconds):
    if t == 0:
        eng.set_param("filt_reso", 0.6)
        eng.note_on(45, 0.9)
        eng.note_on(52, 0.9)
    eng.set_param("filt_cutoff", 100.0 * (150.0 ** min(t / seconds, 1.0)))  # 100 Hz -> 15 kHz, exp
    return (45, 52)


def _lfo(eng, t, seconds):
    if t == 0:
        eng.set_param("lfo1_rate", 4.0)
        eng.set_param("lfo1_depth", 1.0)
        eng.set_param("filt_cutoff", 900.0)
        eng.set_param("filt_reso", 0.4)
        eng.set_param("mod0_src", 1)  # lfo1
        eng.set_param("mod0_dst", 5)  # cutoff
        eng.set_param("mod0_amt", 0.9)
        eng.note_on(40, 0.9)
        eng.note_on(47, 0.9)
    return (40, 47)


def _poly(eng, t, seconds):
    # 8 notes, then two more at 60 % of the duration to force voice stealing
    notes = tuple(range(60, 70))
    for i, n in enumerate(notes):
        when = i * 0.08 if i < 8 else 0.6 * seconds + (i - 8) * 0.1
        if t <= when < t + BLOCK / SR:
            eng.note_on(n, 0.8)
    return notes


DEMOS = {"chord": _chord, "sweep": _sweep, "lfo": _lfo, "poly": _poly}


def render_demo(name: str, path, seconds: float = 4.0) -> Path:
    if name not in DEMOS:
        raise ValueError(f"unknown demo {name!r}; available: {', '.join(sorted(DEMOS))}")
    if not seconds > 0:
        raise ValueError("seconds must be > 0")
    eng = Engine(SR)
    eng.set_param("polyphony", 8)
    play = DEMOS[name]
    body = int(seconds * SR)
    total = body + int(TAIL_S * SR)
    out = np.zeros(total, dtype=np.float32)
    notes: tuple = ()
    for s in range(0, total, BLOCK):
        t = s / SR
        if s < body:
            notes = play(eng, t, seconds)
        elif s - BLOCK < body:  # first block past the end: release everything
            for n in notes:
                eng.note_off(n)
        n = min(BLOCK, total - s)
        eng.render(out[s : s + n])
    if not np.isfinite(out).all():
        raise RuntimeError(f"demo {name!r} produced non-finite samples")
    pcm = (np.clip(out, -1.0, 1.0) * 32767.0).astype("<i2")
    path = Path(path)
    with wave.open(str(path), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(SR)
        w.writeframes(pcm.tobytes())
    return path


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(prog="python -m nomad.render", description="Render a NOMAD demo to a WAV file.")
    ap.add_argument("--demo", required=True, help="one of: " + ", ".join(sorted(DEMOS)))
    ap.add_argument("out", help="output .wav path")
    ap.add_argument("--seconds", type=float, default=4.0)
    a = ap.parse_args(argv)
    try:
        render_demo(a.demo, a.out, a.seconds)
    except (ValueError, OSError) as e:
        print(f"error: {e}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main())
