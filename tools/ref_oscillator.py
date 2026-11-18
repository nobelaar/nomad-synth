"""Oscillator reference scenarios (free running) for the Rust tests."""

from __future__ import annotations

from pathlib import Path

import numpy as np

from nomad.engine.oscillator import Oscillator
from tools.refio import BlobWriter

SR = 48000.0
FREE_BLOCKS = [256, 100, 1, 17]
WAVES = [0, 1, 2, 3]
FREQS = [55.0, 440.0, 5200.0, 12000.0, 21000.0, 0.0]
PWS = [0.5, 0.2]
EDGES = {  # label: (wave, freq, pw); square at 440 Hz unless the edge is the frequency
    "edge/nan_freq": (3, float("nan"), 0.5),
    "edge/neg_freq": (3, -440.0, 0.5),
    "edge/pw_nan": (3, 440.0, float("nan")),
    "edge/pw_0": (3, 440.0, 0.0),
    "edge/pw_1": (3, 440.0, 1.0),
}


def render_blocks(wave, freq, pw, blocks, sync=None):
    """Render contiguous blocks (each from zeros) and return (samples, end phase)."""
    osc = Oscillator(SR)
    osc.wave = wave
    out = []
    for n in blocks:
        buf = np.zeros(n)
        osc.render_into(buf, freq, pw)
        out.append(buf)
    return np.concatenate(out), osc.phase


def write(out_dir: Path) -> None:
    w = BlobWriter("oscillator")
    for wave in WAVES:
        for freq in FREQS:
            for pw in PWS:
                data, ph = render_blocks(wave, freq, pw, FREE_BLOCKS)
                w.add(f"free/{wave}/{freq}/{pw}", data, wave=wave, freq=freq, pw=pw,
                      blocks=FREE_BLOCKS, phase_end=ph)
    for label, (wave, freq, pw) in EDGES.items():
        data, ph = render_blocks(wave, freq, pw, [256])
        w.add(label, data, wave=wave, freq=freq, pw=pw, blocks=[256], phase_end=ph)
    w.write(out_dir)
