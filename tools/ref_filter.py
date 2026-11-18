"""Ladder filter reference scenarios for the Rust tests."""

from __future__ import annotations

from itertools import cycle
from pathlib import Path

import numpy as np

from nomad.engine import _kernels
from nomad.engine.filter import K_MAX, Ladder4Pole
from tools.refio import BlobWriter

SR = 48000.0
FCS = [50.0, 1000.0, 8000.0, 20000.0]
RESS = [0.0, 0.5, 1.0]


def grains(total, pattern):
    out, left = [], total
    for g in cycle(pattern):
        if left <= 0:
            return out
        out.append(min(g, left))
        left -= out[-1]


def run(fc, res, x, sizes):
    f = Ladder4Pole(SR)
    f.set(fc, res)
    buf = np.array(x, dtype=np.float64)
    pos = 0
    for n in sizes:
        f.process(buf[pos:pos + n])
        pos += n
    return buf


def write(out_dir: Path) -> None:
    w = BlobWriter("filter")
    for fc in FCS:
        for res in RESS:
            b, a = np.empty(5), np.empty(5)
            _kernels.ladder_coeffs_into(b, a, fc, res * K_MAX, SR)
            w.add(f"coeffs/{fc}/{res}", np.concatenate([b, a]), fc=fc, k=res * K_MAX)
    noise = np.random.default_rng(7).standard_normal(1024) * 0.3
    w.add("input/noise", noise)
    mixed = grains(1024, [64, 16])
    for fc in FCS:
        for res in RESS:
            w.add(f"run/{fc}/{res}", run(fc, res, noise, mixed), fc=fc, res=res,
                  input="input/noise", grains=mixed)
    impulse = np.zeros(4096)
    impulse[0] = 1.0
    w.add("input/impulse", impulse)
    g16 = grains(4096, [16])
    w.add("run/self_osc", run(1000.0, 1.0, impulse, g16), fc=1000.0, res=1.0,
          input="input/impulse", grains=g16)
    nan = float("nan")
    w.add("set/nan", run(nan, nan, noise, mixed), fc=nan, res=nan, input="input/noise",
          grains=mixed)
    for fc in (1.0, 40000.0):
        w.add(f"set/extremes/{fc}", run(fc, 0.5, noise, mixed), fc=fc, res=0.5,
              input="input/noise", grains=mixed)
    w.write(out_dir)
