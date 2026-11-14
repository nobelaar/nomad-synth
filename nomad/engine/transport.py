"""Global sample-accurate step clock shared by every instrument.

``run`` is the only driver: it splits a render of ``n`` frames at step
boundaries (and at ``limit()`` frames, for sub-step events such as note
gates) and reports them. Phase is the fraction of the current step already
played, so a bpm/swing jump mid-step never skips or repeats a step.
"""

from __future__ import annotations

import math

__all__ = ["Transport", "STEPS"]

STEPS = 16
_FAR = 1 << 30


class Transport:
    def __init__(self, sample_rate: float = 48000.0) -> None:
        self.sr = float(sample_rate)
        self.bpm = 110.0
        self.swing = 0.0
        self.playing = False
        self.position = 0
        self.phase = 0.0
        self._pending_start = False

    # ---- caller thread -----------------------------------------------------
    def start(self) -> None:
        if self.playing:
            return
        self.playing = True
        self.position = 0
        self.phase = 0.0
        self._pending_start = True

    def stop(self) -> None:
        self.playing = False
        self.position = 0
        self.phase = 0.0
        self._pending_start = False

    def toggle(self) -> None:
        self.stop() if self.playing else self.start()

    # ---- audio thread ------------------------------------------------------
    def step_frames(self, step: int) -> float:
        base = self.sr * 60.0 / self.bpm / 4.0
        return base * (1.0 + self.swing) if step % 2 == 0 else base * (1.0 - self.swing)

    def frames_to_step(self) -> int:
        if not self.playing:
            return _FAR
        return max(1, int(math.ceil((1.0 - self.phase) * self.step_frames(self.position))))

    def run(self, n, render_span, on_step, limit=None) -> None:
        i = 0
        while i < n:
            if self._pending_start:
                self._pending_start = False
                on_step(0)
            chunk = min(n - i, self.frames_to_step())
            if limit is not None:
                chunk = min(chunk, max(1, limit()))
            dur = self.step_frames(self.position)
            render_span(i, chunk)
            i += chunk
            if self.playing:
                self.phase += chunk / dur
                if self.phase >= 1.0 - 1e-9:
                    self.phase = max(0.0, self.phase - 1.0)
                    self.position = (self.position + 1) % STEPS
                    on_step(self.position)
