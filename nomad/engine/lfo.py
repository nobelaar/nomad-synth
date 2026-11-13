"""Low-frequency oscillator: sine, triangle, saw, square and sample & hold.

Per R6 the LFO is advanced once per control block (<= 256 frames), so
:meth:`LFO.advance` is scalar, takes the elapsed time in seconds and returns
the value *after* advancing, already multiplied by ``depth``. Nothing is
allocated except one scalar rng draw per sample-and-hold cycle.
"""

from __future__ import annotations

import math

import numpy as np

__all__ = ["LFO"]

_TWO_PI = 2.0 * math.pi
_NUM_WAVES = 5  # 0 sine, 1 triangle, 2 saw, 3 square, 4 sample & hold


def _finite(x: float, lo: float, hi: float) -> float:
    x = float(x)
    return lo if not math.isfinite(x) else min(hi, max(lo, x))


class LFO:
    """Bipolar LFO; ``phase`` is the base phase in cycles, ``[0, 1)``."""

    def __init__(self, sample_rate: float, seed: int = 0) -> None:
        self.sr = float(sample_rate)  # kept for interface symmetry; time-based
        self._rng = np.random.default_rng(seed)
        self.phase = 0.0
        self._rate = 1.0
        self._wave = 0
        self._depth = 1.0
        self._offset = 0.0
        self._held = self._draw()

    def _draw(self) -> float:
        return float(self._rng.random()) * 2.0 - 1.0

    def set(self, rate_hz, wave, depth, phase_offset) -> None:
        w = int(wave)
        if not 0 <= w < _NUM_WAVES:
            raise ValueError(f"wave must be in 0..{_NUM_WAVES - 1}, got {wave!r}")
        self._wave = w
        self._rate = _finite(rate_hz, 0.0, math.inf)
        self._depth = _finite(depth, 0.0, 1.0)
        off = float(phase_offset)
        self._offset = off % 1.0 if math.isfinite(off) else 0.0

    def advance(self, seconds: float) -> float:
        """Advance by ``seconds`` and return the new value, ``|v| <= depth``."""
        dt = _finite(seconds, 0.0, math.inf)
        step = self._rate * dt
        if not math.isfinite(step):
            step = 0.0
        new = self.phase + step
        if new >= 1.0:
            new %= 1.0
            if self._wave == 4:
                self._held = self._draw()
        self.phase = new
        p = (new + self._offset) % 1.0
        w = self._wave
        if w == 0:
            v = math.sin(_TWO_PI * p)
        elif w == 1:
            v = 4.0 * p - 1.0 if p < 0.5 else 3.0 - 4.0 * p
        elif w == 2:
            v = 2.0 * p - 1.0
        elif w == 3:
            v = 1.0 if p < 0.5 else -1.0
        else:
            v = self._held
        return v * self._depth
