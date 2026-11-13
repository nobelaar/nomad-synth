import time

import numpy as np

from nomad.engine._kernels import HAVE_NUMBA
from nomad.engine.engine import Engine


def test_five_seconds_of_eight_voices_faster_than_realtime():
    e = Engine(48000)
    e.set_param("filt_cutoff", 800.0); e.set_param("filt_reso", 0.7)
    e.set_param("filt_env_amt", 0.6); e.set_param("lfo1_depth", 0.4)
    e.set_param("lfo1_rate", 5.0)
    e.set_param("mod0_src", 1); e.set_param("mod0_dst", 5); e.set_param("mod0_amt", 0.5)
    for n in (48, 52, 55, 60, 64, 67, 71, 72): e.note_on(n, 0.9)
    out = np.zeros(48000 * 5)
    e.render(out[:256])  # first block pays JIT cache loading, not steady state
    t0 = time.perf_counter()
    for i in range(256, len(out), 256): e.render(out[i:i+256])
    elapsed = time.perf_counter() - t0
    assert np.isfinite(out).all()
    assert np.abs(out).max() <= 1.0
    # with numba the load is ~25%; the scipy fallback is ~50% and may be tight on slow CI
    assert elapsed < 5.0 * (0.5 if HAVE_NUMBA else 0.9)
