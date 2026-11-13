"""Temporary listening aid for the oscillators built so far.

This is NOT part of the engine: it hardcodes a few voices with a fake
envelope so the waveforms can be heard before the voice/engine layers
exist. Task 18's offline render harness replaces it.

Usage:  ./.venv/bin/python tools/preview.py [out.wav]
"""

from __future__ import annotations

import sys
import wave
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from nomad.engine.oscillator import Oscillator

SR = 48000.0
BLOCK = 256


def amp_env(n: int, attack: float, decay: float, sustain: float,
            release: float, hold: float) -> np.ndarray:
    t = np.arange(n) / SR
    rise = np.clip(t / max(attack, 1e-6), 0.0, 1.0)
    fall = np.clip(1.0 - (1.0 - sustain) * (t - attack) / max(decay, 1e-6), sustain, 1.0)
    body = np.where(t < attack, rise, fall)
    return body * (1.0 - np.clip((t - hold) / max(release, 1e-6), 0.0, 1.0))


def voice(freq: float, duration: float, hold: float, oscs: list[dict]) -> np.ndarray:
    n = int(duration * SR)
    mix = np.zeros(n)
    for spec in oscs:
        o = Oscillator(SR)
        o.wave = spec["wave"]
        buf = np.zeros(n)
        for i in range(0, n, BLOCK):
            chunk = buf[i:i + BLOCK]
            pw = spec["pw"](i / SR) if callable(spec.get("pw")) else spec.get("pw", 0.5)
            sync = None
            if spec.get("sync_ratio"):
                inc = freq / SR
                master = (inc * np.arange(len(chunk)) + (i * inc)) % 1.0
                sync = master
            o.render_into(chunk, freq * 2 ** (spec.get("cents", 0.0) / 1200.0),
                          pw=pw, sync_master_phase=sync)
        mix += buf * spec.get("level", 0.5)
    return mix * amp_env(n, 0.008, 0.35, 0.75, 0.55, hold)


def section(title: str) -> None:
    print(f"  {title}")


def main(path: str) -> None:
    print("NOMAD SYNTH — preview de osciladores")
    pieces = []

    section("1. acorde de La mayor: sierra + cuadrada")
    for freq in (110.0, 164.81, 220.0, 277.18):
        pieces.append(voice(freq, 2.6, 2.0, [
            {"wave": 2, "level": 0.30, "cents": -4.0},
            {"wave": 3, "level": 0.18, "cents": 4.0},
        ]))
    gap(0.25, pieces)

    section("2. batido: dos sierras a 10 cents")
    pieces.append(voice(220.0, 3.0, 2.4, [
        {"wave": 2, "level": 0.35, "cents": 0.0},
        {"wave": 2, "level": 0.35, "cents": 10.0},
    ]))
    gap(0.25, pieces)

    section("3. ancho de pulso barriendo")
    pieces.append(voice(146.83, 3.0, 2.4, [
        {"wave": 3, "level": 0.5, "pw": lambda t: 0.15 + 0.7 * min(t / 2.4, 1.0)},
    ]))
    gap(0.25, pieces)

    section("4. hard sync (esclava a 1.5x)")
    pieces.append(voice(110.0, 2.6, 2.0, [
        {"wave": 2, "level": 0.35},
        {"wave": 2, "level": 0.30, "cents": 702.0, "sync_ratio": 1.5},
    ]))
    gap(0.25, pieces)

    out = np.concatenate(pieces)
    out = np.tanh(out * 0.9) / np.tanh(0.9)
    peak = float(np.abs(out).max())
    if peak > 0:
        out *= 0.89 / peak

    pcm = (out * 32767).astype("<i2")
    with wave.open(path, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(int(SR))
        w.writeframes(pcm.tobytes())
    print(f"\nescrito: {path}  ({len(out) / SR:.1f} s, pico {peak:.3f})")


def gap(seconds: float, pieces: list[np.ndarray]) -> None:
    pieces.append(np.zeros(int(seconds * SR)))


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "/tmp/opencode/nomad-preview.wav")
