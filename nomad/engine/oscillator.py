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
keeps its aliases below -30 dB. Only the square reacts to ``pw``: the other
three have no duty cycle, so the argument must not move them at all.

Hard sync is exposed through ``sync_master_phase``: the local phase restarts
wherever the caller's master phase wraps, and each reset is band-limited with
a PolyBLEP scaled by the sub-sample position of the wrap, so a reset that does
not land on a sample boundary does not alias.
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


def _blep_at(t: float, dt: float) -> float:
    """Scalar PolyBLEP for the two samples around a hard-sync reset.

    The same two polynomials as :func:`_blep`, selected with a plain ``if``: a
    reset is a handful of samples, and one numpy dispatch per sample would cost
    more than the arithmetic. ``dt`` is positive on this path.
    """
    if t < dt:
        x = t / dt
        return x + x - x * x - 1.0
    if t > 1.0 - dt:
        y = (t - 1.0) / dt
        return y * y + y + y + 1.0
    return 0.0


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
        # Scratch for the reset positions of the hard-sync ramp, sized like the
        # ramp so the sync path never regrows a buffer in the audio callback.
        self._sync_rst = np.empty(MAX_BLOCK, dtype=np.float64)

    def _phase_ramp(self, n: int) -> np.ndarray:
        """Return the first ``n`` sample offsets, growing the caches if needed."""
        if n > self._ramp.shape[0]:
            self._ramp = np.arange(n, dtype=np.float64)
            self._sync_rst = np.empty(n, dtype=np.float64)
        return self._ramp[:n]

    def _synced_ramp(
        self,
        n: int,
        inc: float,
        phase: float,
        master: np.ndarray,
    ) -> tuple[np.ndarray, np.ndarray, np.ndarray]:
        """Phase ramp restarted at every wrap of the master phase.

        ``master`` holds the master oscillator's phase for the same samples, in
        cycles. Any decrease between consecutive samples is a wrap, and the
        slave restarts at the master's zero crossing inside that interval --
        which usually falls *between* two samples. Returns the ramp, the
        indices of the samples that follow a reset and, for each of them, the
        fractional position of the reset inside the preceding interval.

        The block is all this method sees, so a wrap that falls exactly between
        two calls is not visible: callers must keep the master phase contiguous
        from one block to the next, and a wrap that lands on the boundary of a
        block goes undetected. A master that jitters (e.g. phase-modulated) is
        treated as wrapping wherever it goes backwards.
        """
        mp = np.asarray(master, dtype=np.float64)
        if mp.shape[0] != n:
            raise ValueError(
                f"sync_master_phase has {mp.shape[0]} samples, expected {n}"
            )
        # Grow both caches before slicing the scratch buffer.
        ramp = self._phase_ramp(n)
        prev = mp[:-1]
        idx = np.flatnonzero(mp[1:] < prev) + 1
        if idx.size == 0:
            # The common case is a block without a wrap: the synced ramp is
            # then exactly the free-running one, with no reset bookkeeping.
            t = ramp * inc
            t += phase
            t %= 1.0
            return t, idx, prev[:0]
        a = prev[idx - 1]
        b = mp[idx]
        # The master ran from ``a`` to ``1 + b``; it crossed 1.0 a fraction
        # ``(1 - a) / (1 + b - a)`` of the way into the interval.
        frac = (1.0 - a) / (1.0 + b - a)
        # Two ufuncs instead of np.clip: the wrapper costs more than the work
        # on a handful of resets.
        np.maximum(frac, 0.0, out=frac)
        np.minimum(frac, 1.0, out=frac)
        # ``rst`` is where the last reset happened, in samples, for every sample
        # of the block. Before the first reset it is the reset that started the
        # current cycle, ``phase / inc`` samples before sample 0.
        rst = self._sync_rst[:n]
        rst.fill(-phase / inc)
        rst[idx] = (idx - 1) + frac
        np.maximum.accumulate(rst, out=rst)
        t = ramp - rst
        t *= inc
        t %= 1.0
        return t, idx, frac

    @staticmethod
    def _value_at(t: float, wave: int, pw: float) -> float:
        """Ideal waveform value at one phase, mirroring the render formulas."""
        if wave == 0:
            return math.sin(2.0 * math.pi * t)
        if wave == 1:
            return 4.0 * min(t, 1.0 - t) - 1.0
        if wave == 2:
            return 2.0 * t - 1.0
        return 1.0 if t < pw else -1.0

    @staticmethod
    def _edge_at(t: float, inc: float, wave: int) -> float:
        """BLEP term the waveform applies for its own edge at phase 0."""
        if wave == 2:
            return -_blep_at(t, inc)
        if wave == 3:
            return _blep_at(t, inc)
        return 0.0

    def _sync_correction(
        self,
        out: np.ndarray,
        t: np.ndarray,
        idx: np.ndarray,
        frac: np.ndarray,
        wave: int,
        inc: float,
        pw: float,
    ) -> None:
        """Band-limit the phase resets with a PolyBLEP scaled by their position.

        A reset of height ``jump`` between samples ``i - 1`` and ``i`` is
        corrected on both sides with the PolyBLEP residuals of a step that falls
        ``frac`` of a sample after ``i - 1``: the sample before it carries
        ``(1 - frac)**2`` of the edge, the one after it carries ``-frac**2``.
        Those residuals replace the waveform's own edge term around phase 0,
        which assumed the height of a regular wrap instead of the actual jump.
        A block has at most a few resets, so this loops over them, never over
        samples.
        """
        for i, f in zip(idx.tolist(), frac.tolist()):
            t_new = float(t[i])
            t_old = float(t[i - 1])
            jump = 0.5 * (
                self._value_at(t_new, wave, pw)
                - self._value_at(t_old, wave, pw)
            )
            out[i] += -jump * f * f - self._edge_at(t_new, inc, wave)
            out[i - 1] += (
                jump * (1.0 - f) * (1.0 - f)
                - self._edge_at(t_old, inc, wave)
            )

    def render_into(
        self,
        out: np.ndarray,
        freq: float,
        pw: float = 0.5,
        sync_master_phase: np.ndarray | None = None,
    ) -> float:
        """Add one block of the current wave into ``out`` and advance phase.

        ``freq`` is the frequency in Hz (0 is allowed and holds a constant
        waveform), ``pw`` the square's duty cycle, clamped to ``0.05..0.95``.
        ``sync_master_phase`` optionally carries the master oscillator's phase
        for these samples, in cycles; the local phase then restarts at every
        master wrap. Returns the phase after the block, in ``[0, 1)``.
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
            # A bad duty cycle must not turn the square into NaN, so fall back
            # to the neutral 50 %.
            pw = 0.5
        else:
            # Outside this range an edge of the square would sit under the
            # other one and the two BLEP corrections would cancel the pulse.
            pw = min(max(pw, 0.05), 0.95)

        phase = self.phase
        if sync_master_phase is None or inc <= 0.0:
            # A frozen oscillator has no edges of its own to reset.
            t = self._phase_ramp(n) * inc
            t += phase
            t %= 1.0
            reset_idx = None
        else:
            t, reset_idx, reset_frac = self._synced_ramp(
                n, inc, phase, sync_master_phase
            )
            if reset_idx.size:
                self._sync_correction(
                    out, t, reset_idx, reset_frac, wave, inc, pw
                )
        last_phase = t[n - 1]

        if wave == 0:
            t *= 2.0 * np.pi
            np.sin(t, out=t)
            out += t
        elif wave == 1:
            # Triangle = 4 * phase_integral(square) - 1, evaluated in closed
            # form. For +1 below the edge and -1 above it, the DC-free integral
            # is Phi(p) = min(p, 1 - p), so the triangle is exactly centred and
            # normalized to +-1 at any pitch. Accumulating the sampled square
            # instead is a half-sample (left/right Riemann) approximation: at
            # 2 kHz that is 2*inc of DC and a positive peak short by 4*inc, and
            # the error grows with the note. The closed form also needs no
            # drift state, so block boundaries cannot bend it. A triangle is
            # symmetric by definition, so ``pw`` deliberately does not reach it.
            phi = np.minimum(t, 1.0 - t)
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

        if reset_idx is not None and reset_idx.size:
            # After a reset the free-running phase is meaningless: the last
            # sample of the synced ramp is the only honest starting point.
            self.phase = (last_phase + inc) % 1.0
        else:
            self.phase = (phase + n * inc) % 1.0
        return self.phase
