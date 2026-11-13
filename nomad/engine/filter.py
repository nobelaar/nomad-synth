"""4-pole ladder filter, closed-form coefficients per grain.

Four identical bilinear one-poles G(z) = g(1+z^-1)/((1+g)-(1-g)z^-1) in a
feedback loop of gain k give H = G^4 / (1 + k G^4). The engine calls
:meth:`Ladder4Pole.process` in grains of <=64 samples (16 when k > 3.2) per R6.
"""

from __future__ import annotations

import math

import numpy as np
from scipy.signal import lfilter

__all__ = ["Ladder4Pole", "ladder_coeffs"]

MAX_GRAIN = 64
K_MAX = 4.3  # resonance 1.0
_K_CLAMP = 3.2  # above this the state is limited to keep self-oscillation bounded
_ZI_LIMIT = 0.8
_MIN_FC = 5.0


def ladder_coeffs(fc_hz: float, k: float, sample_rate: float):
    g = math.tan(math.pi * fc_hz / sample_rate)
    p = np.array([1.0 + g, -(1.0 - g)])
    v = np.array([1.0, 1.0])
    p4 = np.convolve(np.convolve(p, p), np.convolve(p, p))
    g4v4 = g**4 * np.convolve(np.convolve(v, v), np.convolve(v, v))
    den = p4 + k * g4v4
    return g4v4 / den[0], den / den[0]


class Ladder4Pole:
    def __init__(self, sample_rate: float) -> None:
        self.sr = float(sample_rate)
        self._zi = np.zeros(4)
        self._k = 0.0
        self.set(1000.0, 0.0)

    def set(self, fc_hz: float, resonance: float) -> None:
        fc, r = float(fc_hz), float(resonance)
        fc = 0.45 * self.sr if fc != fc else min(max(fc, _MIN_FC), 0.45 * self.sr)
        r = 0.0 if r != r else min(max(r, 0.0), 1.0)
        self._k = r * K_MAX
        self._b, self._a = ladder_coeffs(fc, self._k, self.sr)

    def reset(self) -> None:
        self._zi[:] = 0.0

    def process(self, buf: np.ndarray) -> None:
        if len(buf) > MAX_GRAIN:
            raise ValueError(f"grain of {len(buf)} samples exceeds {MAX_GRAIN}")
        # shortcut: lfilter allocates its output and new zi each call, fine at grain rate
        buf[:], self._zi = lfilter(self._b, self._a, buf, zi=self._zi)
        if self._k > _K_CLAMP:
            m = np.abs(self._zi).max()
            if m > _ZI_LIMIT:
                self._zi *= _ZI_LIMIT / m
