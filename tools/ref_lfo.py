"""numpy Generator stages and LFO reference scenarios for the Rust tests."""

from __future__ import annotations

from pathlib import Path

import numpy as np

from nomad.engine.lfo import LFO
from tools.refio import BlobWriter

SR = 48000.0
SEEDS = [0, 1, 2, 12345, 2**40 + 3]  # the last one needs two u32 words
NAN, INF = float("nan"), float("inf")

LFO_CASES = {
    **{f"lfo/{w}": dict(seed=1, rate=3.7, wave=w, depth=0.8, offset=0.25, steps=[0.005] * 400)
       for w in range(5)},
    "lfo/seed2_sh": dict(seed=2, rate=11.0, wave=4, depth=0.8, offset=0.25, steps=[0.005] * 600),
    "lfo/nonfinite": dict(seed=1, rate=NAN, wave=0, depth=INF, offset=NAN,
                          steps=[NAN, -1.0, 0.01]),
}


def run_lfo(seed, rate, wave, depth, offset, steps):
    lfo = LFO(SR, seed)
    lfo.set(rate, wave, depth, offset)
    return np.array([lfo.advance(s) for s in steps]), lfo.phase


def write(out_dir: Path) -> None:
    w = BlobWriter("lfo")
    for seed in SEEDS:
        # Intermediate stages (ints, exact) to debug SeedSequence -> PCG64 piecewise.
        ss = np.random.SeedSequence(seed)
        st = np.random.PCG64(seed).state["state"]
        stages = {
            "pool": [int(x) for x in ss.pool],
            "words": [int(x) for x in ss.generate_state(4, np.uint64)],
            "state": str(st["state"]),
            "inc": str(st["inc"]),
            "raw": [int(x) for x in np.random.PCG64(seed).random_raw(5)],
        }
        rng = np.random.default_rng(seed)
        w.add(f"rng/{seed}", np.array([rng.random() for _ in range(1000)]), seed=seed, **stages)
    for label, case in LFO_CASES.items():
        values, phase = run_lfo(**case)
        w.add(label, values, phase_end=phase, **case)
    w.write(out_dir)
