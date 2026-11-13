"""ADSR envelope with exponential segments.

Per R6 the coefficients are computed once per stage change (control rate), and
:meth:`Envelope.process` only does multiply-adds per sample on a caller-owned
buffer, so the render path allocates nothing. Release starts from the current
``value``, and a retrigger attacks from it, so neither jumps.
"""

from __future__ import annotations

import math

import numpy as np

__all__ = ["Envelope"]

MIN_TIME = 0.001  # clamp for every stage time, in seconds
_OVERSHOOT = 1.3  # attack aims above 1 so the exponential actually crosses it
_IDLE, _ATTACK, _DECAY, _SUSTAIN, _RELEASE = range(5)
_FLOOR = 1e-4  # release ends (and the voice goes idle) below this level


def _time(x: float) -> float:
    x = float(x)
    return MIN_TIME if not math.isfinite(x) or x < MIN_TIME else x


class Envelope:
    """ADSR generator; ``value`` is the last emitted level."""

    def __init__(self, sample_rate: float) -> None:
        self.sr = float(sample_rate)
        self.value = 0.0
        self._stage = _IDLE
        self._coef = 0.0
        self._a = self._d = self._r = MIN_TIME
        self._s = 0.0
        self.set_params(0.01, 0.1, 0.7, 0.2)

    def set_params(self, attack_s, decay_s, sustain, release_s) -> None:
        old = (self._a, self._d, self._s, self._r)
        self._a, self._d, self._r = _time(attack_s), _time(decay_s), _time(release_s)
        s = float(sustain)
        self._s = min(1.0, max(0.0, s)) if math.isfinite(s) else 0.0
        # Re-enter only when the running stage's own parameter changed: callers
        # push params every control block and re-entering would restart the
        # attack timer forever.
        a, d, sus, r = old
        if self._stage == _ATTACK and self._a != a:
            self._enter(_ATTACK)
        elif self._stage in (_DECAY, _SUSTAIN) and (self._d != d or self._s != sus):
            self._enter(_DECAY)
        elif self._stage == _RELEASE and self._r != r:
            self._enter(_RELEASE)

    def _enter(self, stage: int) -> None:
        self._stage = stage
        if stage == _ATTACK and self.value >= 1.0:
            return self._enter(_DECAY)
        if stage == _ATTACK:
            n = self._a * self.sr
            self._coef = ((_OVERSHOOT - 1.0) / (_OVERSHOOT - self.value)) ** (1.0 / n)
        elif stage == _DECAY:
            self._coef = 1e-3 ** (1.0 / (self._d * self.sr))
        elif stage == _RELEASE:
            self._coef = _FLOOR ** (1.0 / (self._r * self.sr))

    def note_on(self) -> None:
        self._enter(_ATTACK)

    def note_off(self) -> None:
        if self._stage != _IDLE:
            self._enter(_RELEASE)

    def is_idle(self) -> bool:
        return self._stage == _IDLE

    def process(self, out: np.ndarray) -> None:
        """Fill ``out`` with one envelope sample per frame."""
        v, c, s, st = self.value, self._coef, self._s, self._stage
        for i in range(len(out)):
            if st == _ATTACK:
                v = _OVERSHOOT + (v - _OVERSHOOT) * c
                if v >= 1.0:
                    v = 1.0
                    st = self._stage = _DECAY
                    c = self._coef = 1e-3 ** (1.0 / (self._d * self.sr))
            elif st == _DECAY:
                v = s + (v - s) * c
                if abs(v - s) < 1e-5:
                    v = s
                    st = self._stage = _SUSTAIN
            elif st == _RELEASE:
                v *= c
                if v < _FLOOR:
                    v = 0.0
                    st = self._stage = _IDLE
            out[i] = v
        self.value = v
