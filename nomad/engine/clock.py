"""Beat-based clock shared by reference, plus the per-instrument step grid.

``Clock`` only knows time: a position in beats (float64), a tempo and a
running flag. Its position does not depend on the tempo, so a bpm jump is
correct without extra logic. Steps are NOT part of the clock: every
instrument owns a ``StepGrid`` (division, length, swing) and a ``StepTracker``
and derives its steps from ``clock.pos``, so followers of one clock can play
patterns of different lengths and subdivisions and stay aligned.

Threading: the audio thread is the only writer of ``pos`` once a clock is live
(it advances each clock exactly once per render chunk). ``start``/``stop`` and
``bpm`` writes from other threads are only used before the clock is published
to the engine (standalone use and tests); the engine routes them through its
command queue.
"""

from __future__ import annotations

import math

__all__ = ["Clock", "StepGrid", "StepTracker", "STEPS", "FAR"]

STEPS = 16
FAR = 1 << 30
EPS = 1e-9  # beats: float tolerance when a position lands on a boundary


class Clock:
    def __init__(self, sample_rate: float = 48000.0) -> None:
        self.sr = float(sample_rate)
        self.bpm = 110.0
        self.pos = 0.0
        self.running = False

    def start(self) -> None:
        if self.running:
            return
        self.pos = 0.0
        self.running = True

    def stop(self) -> None:
        self.running = False
        self.pos = 0.0

    def toggle(self) -> None:
        self.stop() if self.running else self.start()

    def advance(self, frames: int) -> None:
        if self.running:
            self.pos += frames * self.bpm / (60.0 * self.sr)

    def frames_to(self, beat: float) -> int:
        """Frames until ``pos`` reaches ``beat`` (>= 1); ``FAR`` when stopped."""
        if not self.running:
            return FAR
        return max(1, int(math.ceil((beat - self.pos) * 60.0 * self.sr / self.bpm - 1e-6)))


class StepGrid:
    """Closed-form step positions: even steps last ``d*(1+swing)``, odd ``d*(1-swing)`` beats."""

    def __init__(self, division: float = 0.25, length: int = STEPS, swing: float = 0.0) -> None:
        self.division, self.length, self.swing = division, length, swing

    def step_beats(self, k: int) -> float:
        return self.division * (1.0 + self.swing if k % 2 == 0 else 1.0 - self.swing)

    def step_start(self, k: int) -> float:
        return (k // 2) * 2.0 * self.division + (k % 2) * self.division * (1.0 + self.swing)

    def step_at(self, pos: float) -> int:
        """Absolute step index at ``pos`` (a position within EPS of a boundary counts as past it)."""
        d2 = 2.0 * self.division
        p = pos + EPS
        pair = int(p // d2)
        return 2 * pair + (0 if p - pair * d2 < self.division * (1.0 + self.swing) else 1)

    def next_boundary(self, pos: float) -> float:
        return self.step_start(self.step_at(pos) + 1)

    def pattern_step(self, k: int) -> int:
        return k % self.length


class StepTracker:
    """Turns clock positions into 'fire pattern step N' events, each exactly once."""

    def __init__(self, grid: StepGrid) -> None:
        self.grid, self.last, self._last_pos, self._clock, self._joined = grid, -1, 0.0, None, False

    def poll(self, clock: Clock) -> int | None:
        if clock is not self._clock:  # source switched: join the new clock mid-run, never restart the pattern
            self._joined = self._clock is not None  # a step on the very boundary was already played by the old clock
            self._clock, self.last = clock, -1
        if not clock.running:
            self.last, self._last_pos, self._joined = -1, 0.0, False
            return None
        k = self.grid.step_at(clock.pos)
        restarted = clock.pos < self._last_pos - EPS  # the clock went back to the start
        self._last_pos = clock.pos
        if k == self.last:
            return None
        if k < self.last and not restarted:
            return None  # grid changed under us (swing drag): never repeat a step
        fresh = self.last < 0 or restarted  # started, joined mid-run, or restarted
        joined, self._joined = self._joined, False
        self.last = k
        if fresh and (joined or abs(clock.pos - self.grid.step_start(k)) > EPS):
            return None  # joined mid-step (or at a handover): wait for the next boundary
        return self.grid.pattern_step(k)

    def frames_to_event(self, clock: Clock) -> int:
        return clock.frames_to(self.grid.next_boundary(clock.pos))
