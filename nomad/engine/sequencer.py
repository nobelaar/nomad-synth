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

__all__ = ["StepSequencer", "EXTRA", "NONE"]

EXTRA = 5  # notes a step holds besides ``note``: a chord is up to 6 voices (the synth has 8; the rest is release tail)
NONE = 255


class StepSequencer:
    def __init__(self, synth, sample_rate: float) -> None:
        self.synth, self.sr = synth, float(sample_rate)
        self.grid = StepGrid(0.25, STEPS, 0.0)
        self.tracker = StepTracker(self.grid)
        self.on = np.zeros(STEPS, dtype=np.uint8)
        self.note = np.full(STEPS, 60, dtype=np.uint8)
        self.vel = np.full(STEPS, 100, dtype=np.uint8)
        self.extra = np.full((STEPS, EXTRA), NONE, dtype=np.uint8)  # chord notes beyond ``note`` (NONE = empty)
        self.enabled = False
        self._gate = 0.5
        self._held_n = np.full(EXTRA + 1, NONE, dtype=np.uint8)  # notes the sequencer is sounding
        self._nheld = 0
        self._rec_down = 0  # step record: keys of the current chord still down
        self._rt_last = -1  # real-time record: step the last note went to
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
        if not on:
            self.extra[step] = NONE

    def chord(self, step: int) -> list[int]:
        return [int(self.note[step])] + [int(n) for n in self.extra[step] if n != NONE]

    def add_note(self, step: int, n: int) -> None:
        """Add ``n`` to the step's chord (no-op when full or already there)."""
        n = min(max(int(n), 0), 127)
        if n == self.note[step] or n in self.extra[step]:
            return
        free = np.flatnonzero(self.extra[step] == NONE)
        if len(free):
            self.extra[step, free[0]] = n

    def transpose(self, step: int, delta: int) -> None:
        """Move the whole chord, clamped so no note leaves 0..127."""
        notes = self.chord(step)
        delta = max(-min(notes), min(delta, 127 - max(notes)))
        self.note[step] = int(self.note[step]) + delta
        for i in range(EXTRA):
            if self.extra[step, i] != NONE:
                self.extra[step, i] = int(self.extra[step, i]) + delta

    def clear(self) -> None:
        self.on.fill(0)
        self.extra.fill(NONE)

    # ---- audio thread ------------------------------------------------------
    def record(self, note: int, vel: int) -> None:
        """Clock running: write at the nearest step (real-time). Stopped: write at the cursor and advance (step)."""
        if self.clock.running:
            self._rec_down = 0  # the clock started while keys were down
            g, pos = self.grid, self.clock.pos
            k = g.step_at(pos)
            if pos - g.step_start(k) > g.step_start(k + 1) - pos:
                k += 1
            step = g.pattern_step(k)
            if k == self._rt_last:  # same step as the last note: a chord
                self.add_note(step, note)
            else:
                self.set_step(step, True, note, max(vel, 1))
                self.extra[step] = NONE
                self._rt_last = k
        else:
            self._rt_last = -1
            step = self.rec_step
            if self._rec_down == 0:  # first key of a chord: rewrite the step
                self.set_step(step, True, note, max(vel, 1))
                self.extra[step] = NONE
            else:
                self.add_note(step, note)
            self._rec_down += 1

    def record_off(self) -> None:
        """A key was released: when the last key of a chord comes up, the step cursor advances."""
        if self._rec_down > 0 and not self.clock.running:
            self._rec_down -= 1
            if self._rec_down == 0:
                self.rec_step = (self.rec_step + 1) % self.grid.length

    def arm(self, on: bool) -> None:
        self.rec, self.rec_step, self._rec_down, self._rt_last = bool(on), 0, 0, -1

    def rest(self) -> None:
        if not self.clock.running and self._rec_down == 0:
            self.set_step(self.rec_step, False)
            self.rec_step = (self.rec_step + 1) % self.grid.length

    def on_step(self, pos: int) -> None:
        if not self.enabled:
            if self._nheld:
                self.release()
            return
        self._off()
        if self.on[pos]:
            vel = int(self.vel[pos]) / 127.0
            self._sound(int(self.note[pos]), vel)
            for n in self.extra[pos]:
                if n != NONE:
                    self._sound(int(n), vel)
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
        return self._left if self._nheld else FAR

    def advance(self, n: int) -> None:
        if not self._nheld:
            return
        if not self.enabled:
            self.release()
            return
        self._left -= n
        if self._left <= 0:
            self._off()

    def _sound(self, n: int, vel: float) -> None:
        self.synth.vm.note_on(n, vel)
        if self._nheld < len(self._held_n):
            self._held_n[self._nheld] = n
            self._nheld += 1

    def _off(self) -> None:
        if self._nheld:
            for i in range(self._nheld):
                n = int(self._held_n[i])
                if not self.synth._live[n]:
                    self.synth.vm.note_off(n)
            self._nheld, self._left = 0, FAR

    def release(self) -> None:
        self._off()  # honours live-held keys; panic clears them first
