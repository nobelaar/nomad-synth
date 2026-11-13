"""PolyBLEP oscillator with sine, triangle, saw and square waveforms.

This is the hottest block of the engine: eight voices render two oscillators
each, advanced once per control block of ``min(256, remaining)`` frames (only
the filter splits a block into 16-sample sub-grains).
:meth:`Oscillator.render_into` therefore works on whole numpy arrays -- no
per-sample Python loop -- and *adds* its output into a buffer owned by the
caller, so the voice layer can mix several oscillators into the same block
without extra copies.

The phase lives in cycles, ``[0, 1)``, and is public so the voice layer can
keep it across blocks; :meth:`render_into` advances it and returns the new
value. All four waveforms are derived from that single accumulator:

* ``0`` sine, computed directly from the phase;
* ``1`` triangle, the exact integral of the square, normalized to ``+-1`` and
  evaluated in closed form from the phase;
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

#: Largest control block the engine renders in one call. The phase ramp is
#: preallocated to this length so that even the first ``render_into`` of a
#: freshly created oscillator (note-on inside the audio callback) allocates
#: nothing; a longer block simply grows the ramp once.
MAX_BLOCK = 2048


def _blep(t, dt):
    """PolyBLEP correction; ``t`` is the phase in [0, 1), ``dt`` the increment.

    The scalar formula from the plan is evaluated elementwise so a whole block
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
        # Cached sample offsets for the current block size. The render path
        # must not allocate with np.arange/np.empty, and a note-on builds
        # oscillators inside the audio callback, so the ramp is preallocated to
        # the largest block the engine asks for and only grown if that is
        # exceeded.
        self._ramp = np.arange(MAX_BLOCK, dtype=np.float64)

    def _phase_ramp(self, n: int) -> np.ndarray:
        """Return the first ``n`` sample offsets, growing the cache if needed."""
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
        """Add one block of the current wave into ``out`` and advance phase.

        ``freq`` is the frequency in Hz (0 is allowed and holds a constant
        waveform), ``pw`` the square's duty cycle. ``sync_master_phase`` is
        part of the hard-sync interface of the voice layer and is not used by
        this oscillator yet. Returns the phase after the block, in ``[0, 1)``.
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
        if not math.isfinite(pw):
            # A bad duty cycle must not turn the square or the triangle into
            # NaN, so fall back to the neutral 50 %.
            pw = 0.5

        phase = self.phase
        t = self._phase_ramp(n) * inc
        t += phase
        t %= 1.0

        if wave == 0:
            t *= 2.0 * np.pi
            np.sin(t, out=t)
            out += t
        elif wave == 1:
            # Triangle = 4 * phase_integral(square) - 1, evaluated in closed
            # form. For +1 below pw and -1 above it, the DC-free integral is
            #   Phi(p) = min(p, 2*pw - p) - (2*pw - 1) * p
            # so the triangle is exactly centred and normalized to +-1 at any
            # pitch. Accumulating the sampled square instead is a half-sample
            # (left/right Riemann) approximation: at 2 kHz that is 2*inc of DC
            # and a positive peak short by 4*inc, and the error grows with the
            # note. The closed form also needs no drift state, so block
            # boundaries cannot bend it.
            phi = np.minimum(t, 2.0 * pw - t)
            phi -= (2.0 * pw - 1.0) * t
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
