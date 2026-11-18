"""Drum one-shot synthesis reference: butter/lfilter pieces and every shot.

Shots are long (up to 1.5 s), so each entry keeps the first HEAD samples in full
and then one sample every DECIM, plus checksums of the whole shot in meta.
"""

from __future__ import annotations

import math
from pathlib import Path

import numpy as np
from scipy.signal import butter, lfilter

from nomad.engine.drums import INSTRUMENTS, render_one_shot
from tools.refio import BlobWriter

HEAD, DECIM = 256, 64
COMBOS = [(0.0, 0.5), (-1.0, 0.0), (1.0, 1.0), (0.37, 0.81)]
BANDS = [(0.01, 0.2), (0.05, 0.5), (0.001, 0.9), (0.2, 0.25), (0.3, 0.9), (0.0125, 0.1)]


def shot_entry(w, label, kind, sr, tune, decay):
    y = render_one_shot(kind, sr, tune, decay).astype(np.float64)
    w.add(label, np.concatenate([y[:HEAD], y[HEAD::DECIM]]), kind=INSTRUMENTS.index(kind), sr=sr,
          tune=tune, decay=decay, len=int(y.size), head=HEAD, decim=DECIM, peak=float(np.abs(y).max()),
          sum=math.fsum(y), sumsq=math.fsum(y * y))


def write(out_dir: Path) -> None:
    w = BlobWriter("drum_shots")
    for lo, hi in BANDS:
        b, a = butter(1, [lo, hi], btype="bandpass")
        w.add(f"butter/{lo}_{hi}", np.concatenate([b, a]), low=lo, high=hi)
    x = np.random.default_rng(11).standard_normal(200)
    b, a = butter(1, [0.05, 0.4], btype="bandpass")
    w.add("lfilter/input", x)
    w.add("lfilter/basic", lfilter(b, a, x), b=b.tolist(), a=a.tolist())
    w.write(out_dir)
    for kind in INSTRUMENTS:  # one blob per instrument, for the size limit
        w = BlobWriter(f"drum_shots_{kind}")
        for tune, decay in COMBOS:
            shot_entry(w, f"shot/{kind}/{tune}/{decay}", kind, 48000.0, tune, decay)
        shot_entry(w, f"shot/{kind}/nonfinite", kind, 48000.0, float("nan"), float("inf"))
        w.write(out_dir)
    w = BlobWriter("drum_shots_44k")
    for kind in INSTRUMENTS:
        shot_entry(w, f"shot/{kind}/sr44100", kind, 44100.0, 0.0, 0.5)
    w.write(out_dir)
