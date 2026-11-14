"""16-step monophonic note sequencer: an event source for one ``SynthInstrument``.

It never produces audio: on each transport step it calls the synth's own
``VoiceManager`` (same path as a played key) and releases the note after
``gate`` x step length. The GUI writes ``on/note/vel/gate/enabled`` with plain
atomic stores; everything else runs on the audio thread (no allocations).

shortcut: a sequencer note-off is skipped when the same key is held live, so
the player's key is never cut; a voice-origin tag would be the full fix.
"""

from __future__ import annotations

import math

import numpy as np

from nomad.engine.transport import STEPS, Transport

__all__ = ["StepSequencer"]

_FAR = 1 << 30


class StepSequencer:
    def __init__(self, synth, sample_rate: float, transport: Transport) -> None:
        self.synth, self.sr, self.transport = synth, float(sample_rate), transport
        self.on = np.zeros(STEPS, dtype=np.uint8)
        self.note = np.full(STEPS, 60, dtype=np.uint8)
        self.vel = np.full(STEPS, 100, dtype=np.uint8)
        self.enabled = False
        self._gate = 0.5
        self._held = -1
        self._left = _FAR

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
            self._left = max(1, int(self._gate * self.transport.step_frames(pos)))

    def frames_to_off(self) -> int:
        return self._left if self._held >= 0 else _FAR

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
            self._held, self._left = -1, _FAR

    def release(self) -> None:
        self._off()  # honours live-held keys; panic clears them first
