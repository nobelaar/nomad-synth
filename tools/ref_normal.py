"""numpy standard_normal reference: first 10000 per seed and tail samples."""

from __future__ import annotations

from pathlib import Path

import numpy as np

from tools.refio import BlobWriter

SEEDS = [1, 2, 3, 4, 5, 6]
TAIL_SEED, TAIL_N, TAIL_HITS = 3, 2_000_000, 20


def write(out_dir: Path) -> None:
    for seed in SEEDS:
        w = BlobWriter(f"normal_{seed}")
        w.add("normal", np.random.default_rng(seed).standard_normal(10000), seed=seed)
        w.write(out_dir)
    x = np.random.default_rng(TAIL_SEED).standard_normal(TAIL_N)
    idx = np.flatnonzero(np.abs(x) > 3.65)[:TAIL_HITS]
    w = BlobWriter("normal_tail")
    w.add("tail_hits", x[idx], seed=TAIL_SEED, n=TAIL_N, indices=[int(i) for i in idx])
    w.write(out_dir)
