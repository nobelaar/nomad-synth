"""16-step monophonic note sequencer: an event source for one ``SynthInstrument``.

It never produces audio: on each clock step it calls the synth's own
``VoiceManager`` (same path as a played key) and releases the note after
``gate`` x step length. The GUI writes ``on/note/vel/gate/enabled`` with plain
atomic stores; everything else runs on the audio thread (no allocations).

shortcut: a sequencer note-off is skipped when the same key is held live, so
the player's key is never cut; a voice-origin tag would be the full fix.
"""

from __future__ import annotations

import math

import numpy as np

from nomad.engine.clock import FAR, STEPS, Clock, StepGrid, StepTracker

__all__ = ["StepSequencer"]



class StepSequencer:
    def __init__(self, synth, sample_rate: float) -> None:
        self.synth, self.sr = synth, float(sample_rate)
        self.grid = StepGrid(0.25, STEPS, 0.0)
        self.tracker = StepTracker(self.grid)
        self.on = np.zeros(STEPS, dtype=np.uint8)
        self.note = np.full(STEPS, 60, dtype=np.uint8)
        self.vel = np.full(STEPS, 100, dtype=np.uint8)
        self.enabled = False
        self._gate = 0.5
        self._held = -1
        self._left = FAR
        self.rec = False  # audio-thread owned: set through the synth's command queue
        self.rec_step = 0  # step-record cursor

    @property
    def clock(self) -> Clock:
        return self.synth.eclock  # the effective clock: own if Internal, else the source's

    @property
    def gate(self) -> float:
        return self._gate

    @gate.setter
    def gate(self, v) -> None:
        v = float(v)
        self._gate = min(max(v, 0.05), 1.0) if math.isfinite(v) else 0.5

    # ---- caller thread -----------------------------------------------------
    def set_step(self, step: int, on: bool, note: int | None = None, vel: int | None = None) -> None:
        if not 0 <= step < STEPS:
            return
        if note is not None:
            self.note[step] = min(max(int(note), 0), 127)
        if vel is not None:
            self.vel[step] = min(max(int(vel), 1), 127)
        self.on[step] = 1 if on else 0

    def clear(self) -> None:
        self.on.fill(0)

    # ---- audio thread ------------------------------------------------------
    def record(self, note: int, vel: int) -> None:
        """Clock running: write at the nearest step (real-time). Stopped: write at the cursor and advance (step)."""
        if self.clock.running:
            g, pos = self.grid, self.clock.pos
            k = g.step_at(pos)
            if pos - g.step_start(k) > g.step_start(k + 1) - pos:
                k += 1
            self.set_step(g.pattern_step(k), True, note, max(vel, 1))
        else:
            self.set_step(self.rec_step, True, note, max(vel, 1))
            self.rec_step = (self.rec_step + 1) % self.grid.length

    def rest(self) -> None:
        if not self.clock.running:
            self.on[self.rec_step] = 0
            self.rec_step = (self.rec_step + 1) % self.grid.length

    def on_step(self, pos: int) -> None:
        if not self.enabled:
            if self._held >= 0:
                self.release()
            return
        self._off()
        if self.on[pos]:
            n = int(self.note[pos])
            self.synth.vm.note_on(n, int(self.vel[pos]) / 127.0)
            self._held = n
            self._left = max(1, int(self._gate * self.grid.step_beats(pos) * 60.0 * self.sr / self.clock.bpm))

    @property
    def position(self) -> int:
        return self.grid.pattern_step(self.grid.step_at(self.clock.pos)) if self.clock.running else 0

    def poll(self) -> None:
        step = self.tracker.poll(self.clock)
        if not self.clock.running:
            self.release()  # clock stopped: never leave a hung note
        elif step is not None:
            self.on_step(step)

    def frames_to_event(self) -> int:
        n = self.frames_to_off()
        return min(n, self.tracker.frames_to_event(self.clock)) if self.enabled else n

    def frames_to_off(self) -> int:
        return self._left if self._held >= 0 else FAR

    def advance(self, n: int) -> None:
        if self._held < 0:
            return
        if not self.enabled:
            self.release()
            return
        self._left -= n
        if self._left <= 0:
            self._off()

    def _off(self) -> None:
        if self._held >= 0:
            if not self.synth._live[self._held]:
                self.synth.vm.note_off(self._held)
            self._held, self._left = -1, FAR

    def release(self) -> None:
        self._off()  # honours live-held keys; panic clears them first
