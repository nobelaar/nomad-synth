"""PolyBLEP oscillator with sine, triangle, saw and square waveforms.

This is the hottest block of the engine: eight voices render two oscillators
each, in grains of sixteen samples. :meth:`Oscillator.render_into` therefore
works on whole numpy arrays (no per-sample Python loop) and *adds* its output
into a buffer owned by the caller, so the voice layer can mix several
oscillators into the same grain without extra copies.

The phase lives in cycles, ``[0, 1)``, and is public so the voice layer can
keep it across grains; :meth:`render_into` advances it and returns the new
value. All four waveforms are derived from that single accumulator:

* ``0`` sine, computed directly from the phase;
* ``1`` triangle, the integral of the band-limited square, which gives it the
  same anti-aliasing as the square for free;
* ``2`` saw, ``2t - 1`` with a PolyBLEP correction at the wrap;
* ``3`` square, two PolyBLEP corrections (one per edge) with duty ``pw``.

Every discontinuous waveform is corrected with PolyBLEP, so a 5.2 kHz saw
keeps its aliases below -30 dB.
"""

from __future__ import annotations

import math

import numpy as np

__all__ = ["Oscillator"]

#: Waveform selectors accepted by :attr:`Oscillator.wave`.
WAVES = (0, 1, 2, 3)

#: Highest phase increment the PolyBLEP correction can handle. Above 0.5 the
#: two halves of the correction overlap, so the frequency is clamped just
#: below Nyquist instead of producing a broken waveform.
MAX_INC = 0.499


def _blep(t, dt):
    """PolyBLEP correction; ``t`` is the phase in [0, 1), ``dt`` the increment.

    The scalar formula from the plan is evaluated elementwise so a whole grain
    can be corrected at once. A non-positive ``dt`` means the oscillator is
    frozen (frequency 0), where there is no edge to correct.
    """
    if dt <= 0.0:
        return 0.0
    x = t / dt
    rising = x + x - x * x - 1.0
    y = (t - 1.0) / dt
    falling = y * y + y + y + 1.0
    return np.where(t < dt, rising, np.where(t > 1.0 - dt, falling, 0.0))


def _square(t: np.ndarray, inc: float, pw: float) -> np.ndarray:
    """Band-limited square from the phase array ``t``, duty cycle ``pw``."""
    sq = np.where(t < pw, 1.0, -1.0)
    sq += _blep(t, inc)
    edge = t - pw
    edge -= np.floor(edge)
    sq -= _blep(edge, inc)
    return sq


class Oscillator:
    """One oscillator with a public waveform selector and phase accumulator."""

    def __init__(self, sample_rate: float) -> None:
        sr = float(sample_rate)
        if not sr > 0.0:
            raise ValueError("sample_rate must be positive")
        self.sample_rate = sr
        self.wave = 2
        self.phase = 0.0
        # Integrator state of the triangle, in units of the square wave's
        # phase integral. It is carried across grains so the triangle stays
        # continuous; the DC-free integration below keeps it bounded.
        self._tri_state = 0.0
        # Cached sample offsets for the current grain size. The render path
        # must not allocate with np.arange/np.empty, so the ramp is created
        # here (or when a longer grain shows up) and only sliced afterwards.
        self._ramp = np.arange(0, dtype=np.float64)

    def _phase_ramp(self, n: int) -> np.ndarray:
        """Return ``np.arange(n)``, growing the cached ramp only if needed."""
        if n > self._ramp.shape[0]:
            self._ramp = np.arange(n, dtype=np.float64)
        return self._ramp[:n]

    def render_into(
        self,
        out: np.ndarray,
        freq: float,
        pw: float = 0.5,
        sync_master_phase: np.ndarray | None = None,
    ) -> float:
        """Add one grain of the current wave into ``out`` and advance phase.

        ``freq`` is the frequency in Hz (0 is allowed and holds a constant
        waveform), ``pw`` the square's duty cycle. ``sync_master_phase`` is
        part of the hard-sync interface of the voice layer and is not used by
        this oscillator yet. Returns the phase after the grain, in ``[0, 1)``.
        """
        wave = self.wave
        if wave not in WAVES:
            raise ValueError(f"unknown wave {wave!r}, expected 0..3")
        wave = int(wave)

        n = out.shape[0]
        if n == 0:
            return self.phase

        sr = self.sample_rate
        f = float(freq)
        if not math.isfinite(f):
            inc = 0.0
        else:
            inc = min(abs(f), MAX_INC * sr) / sr

        phase = self.phase
        t = self._phase_ramp(n) * inc
        t += phase
        t %= 1.0

        if wave == 0:
            t *= 2.0 * np.pi
            np.sin(t, out=t)
            out += t
        elif wave == 1:
            sq = _square(t, inc, pw)
            # Triangle = 4 * integral(square) - 1. Integrating the square's
            # DC-free form (its mean 2*pw - 1 removed) keeps the carried state
            # bounded instead of ramping away, which matters because grains are
            # only 16 samples long and the state has to survive them. The *4
            # gives a peak-to-peak of 2 and the -1 offset centres it near +-1.
            sq -= 2.0 * pw - 1.0
            phi = np.cumsum(sq)
            phi *= inc
            phi += self._tri_state
            self._tri_state = float(phi[-1])
            phi *= 4.0
            phi -= 1.0
            out += phi
        elif wave == 2:
            saw = t + t
            saw -= 1.0
            saw -= _blep(t, inc)
            out += saw
        else:
            sq = _square(t, inc, pw)
            out += sq

        self.phase = (phase + n * inc) % 1.0
        return self.phase
