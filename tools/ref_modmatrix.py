"""Modulation matrix reference scenarios for the Rust tests."""

from __future__ import annotations

from pathlib import Path

import numpy as np

from nomad.engine.modmatrix import DESTINATIONS, SOURCES, ModMatrix
from tools.refio import BlobWriter

NAN, INF = float("nan"), float("inf")
SRC = [0.3, -0.2, 0.5, 0.9, 0.4]  # lfo1, lfo2, modwheel, velocity, env2

CASES = {
    "m/empty": ([], SRC),
    "m/one_per_dst": ([[i, i % 5 + 1, i + 1, 0.1 * (i + 1) - 0.35] for i in range(7)], SRC),
    "m/sum_same_dst": ([[0, 1, 5, 0.5], [3, 4, 5, -0.25], [7, 5, 5, 1.0]], SRC),
    "m/amount_clamped": ([[0, 1, 1, NAN], [1, 2, 2, 5.0], [2, 3, 3, -5.0], [3, 4, 4, INF]], SRC),
    "m/off_routes": ([[0, 0, 1, 1.0], [1, 3, 0, 1.0], [2, 5, 6, 0.6]], SRC),
    "m/negative_sources": ([[i, i % 5 + 1, 7 - i % 7, 0.9] for i in range(8)],
                           [-1.0, -0.5, -0.25, -0.75, -0.1]),
}


def resolve(slots, sources):
    m = ModMatrix()
    for i, s, d, a in slots:
        m.set_slot(i, s, d, a)
    out = m.resolve(dict(zip(SOURCES[1:], sources)))
    return np.array([out[name] for name in DESTINATIONS[1:]])


def write(out_dir: Path) -> None:
    w = BlobWriter("modmatrix")
    for label, (slots, sources) in CASES.items():
        w.add(label, resolve(slots, sources), slots=slots, sources=sources)
    w.write(out_dir)
