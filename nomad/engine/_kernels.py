"""Optional numba kernels (R9). Without numba (or with NOMAD_NO_NUMBA=1) ``jit`` is the
identity: the envelope loop then runs as plain Python and the filter keeps its scipy path."""

from __future__ import annotations

import os

import numpy as np

try:
    if os.environ.get("NOMAD_NO_NUMBA"):
        raise ImportError
    import numba

    jit = numba.njit(cache=True, fastmath=False)
    HAVE_NUMBA = True
except ImportError:
    HAVE_NUMBA = False

    def jit(f):
        return f


@jit
def ladder_coeffs_into(b, a, fc, k, sr):
    """Closed-form ladder coefficients written into b[5], a[5] (no allocation)."""
    g = np.tan(np.pi * fc / sr)
    pa, pb = 1.0 + g, g - 1.0
    g4 = g**4
    binom = (1.0, 4.0, 6.0, 4.0, 1.0)
    a0 = pa**4 + k * g4
    for j in range(5):
        c = binom[j]
        a[j] = (c * pa ** (4 - j) * pb**j + k * g4 * c) / a0
        b[j] = g4 * c / a0


@jit
def ladder_run(buf, b, a, zi, clamp):
    """Direct-form-II-transposed IIR in place (same recursion as lfilter), then the state clamp.
    ``clamp`` <= 0 disables it; otherwise |zi| is limited to ``clamp``."""
    for n in range(buf.shape[0]):
        x = buf[n]
        y = b[0] * x + zi[0]
        zi[0] = b[1] * x + zi[1] - a[1] * y
        zi[1] = b[2] * x + zi[2] - a[2] * y
        zi[2] = b[3] * x + zi[3] - a[3] * y
        zi[3] = b[4] * x - a[4] * y
        buf[n] = y
    if clamp > 0.0:
        m = 0.0
        for j in range(4):
            m = max(m, abs(zi[j]))
        if m > clamp:
            for j in range(4):
                zi[j] *= clamp / m


@jit
def env_run(out, v, c, s, st, dcoef):
    """Envelope sample loop. Stages: 1 attack, 2 decay, 3 sustain, 4 release, 0 idle.
    Returns (value, coef, stage)."""
    for i in range(out.shape[0]):
        if st == 1:
            v = 1.3 + (v - 1.3) * c
            if v >= 1.0:
                v = 1.0
                st = 2
                c = dcoef
        elif st == 2:
            v = s + (v - s) * c
            if abs(v - s) < 1e-5:
                v = s
                st = 3
        elif st == 4:
            v *= c
            if v < 1e-4:
                v = 0.0
                st = 0
        out[i] = v
    return v, c, st
