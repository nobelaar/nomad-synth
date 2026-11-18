"""Voice allocation and stealing over a fixed pool of ``Voice`` objects.

Allocation: a note already sounding is retriggered on its own voice (via the
click-free steal fade); else a free voice if fewer than ``polyphony`` voices
are active; else steal: oldest voice in release, else oldest held voice.

``age`` is a monotonic stamp taken at every note_on (smaller = older); a
retrigger refreshes it. Simplest reading of the brief's "age increments".

``active_notes()`` lists keys that are HELD (gated), not voices merely fading
out in release. A pending steal reports the new key.

Lowering polyphony releases (note_off) the oldest held voices beyond the new
limit, so no cut and no click. Releasing voices may still ring on.

``render(out, p, mods, resolve=None)``: each active voice adds into ``out``;
no scratch beyond what Voice owns. Per-voice velocity/env2 (R2): if
``resolve`` is given, each voice gets ``resolve(voice, mods)`` instead of
``mods`` (the result is consumed immediately, so a shared dict is fine).
"""

from __future__ import annotations

import math
import operator

from nomad.engine.voice import STEAL_FADE_SEC, Voice  # noqa: F401  (re-exported)

__all__ = ["VoiceManager", "STEAL_FADE_SEC"]


def _note(n):
    try:
        n = operator.index(n)
    except TypeError:
        if not isinstance(n, float) or not math.isfinite(n) or n != int(n):
            return None
        n = int(n)
    return n if 0 <= n <= 127 else None


class VoiceManager:
    def __init__(self, sample_rate: float, max_voices: int = 8) -> None:
        self.max_voices = max(1, int(max_voices))
        self.voices = [Voice(sample_rate, i) for i in range(self.max_voices)]
        self._held = [False] * self.max_voices
        self._stamp = [0] * self.max_voices
        self._clock = 0
        self.polyphony = self.max_voices
        self.glide = 0.0  # ms; > 0 makes new voices slide in from ``_last`` (and mono legato)
        self._last = None  # last note played: where a fresh voice starts its glide

    def set_polyphony(self, n) -> None:
        try:
            n = int(n)
        except (TypeError, ValueError, OverflowError):
            return  # NaN / junk: keep the current setting
        self.polyphony = min(max(n, 1), self.max_voices)
        held = sorted(self._held_slots(), key=self._stamp.__getitem__)
        for i in held[: max(0, len(held) - self.polyphony)]:
            self._release(i)

    def set_glide(self, ms) -> None:
        try:
            ms = float(ms)
        except (TypeError, ValueError):
            return
        self.glide = ms if math.isfinite(ms) and ms > 0.0 else 0.0

    def _held_slots(self) -> list[int]:
        return [i for i, v in enumerate(self.voices) if self._held[i] and v.is_active()]

    def _release(self, i: int) -> None:
        self._held[i] = False
        self.voices[i].note_off()

    def note_on(self, note, velocity) -> None:
        note = _note(note)
        if note is None:
            return
        self._clock += 1
        active = [i for i, v in enumerate(self.voices) if v.is_active()]
        if self.glide > 0.0 and self.polyphony == 1:
            held = self._held_slots()
            if held:  # mono legato: the held voice just glides to the new key, envelopes keep running
                i = held[0]
                if self.voices[i].note != note:
                    self.voices[i].set_target(note)
                self._stamp[i], self.voices[i].age, self._last = self._clock, self._clock, note
                return
        same = [i for i in active if self.voices[i].note == note]
        if same:
            i = min(same, key=self._stamp.__getitem__)
            steal = True
        elif len(active) < self.polyphony and len(active) < self.max_voices:
            i = next(j for j, v in enumerate(self.voices) if not v.is_active())
            steal = False
        else:
            releasing = [j for j in active if not self._held[j]]
            i = min(releasing or active, key=self._stamp.__getitem__)
            steal = True
        v = self.voices[i]
        if steal:
            v.begin_steal(note, velocity)
        else:
            v.note_on(note, velocity, self._last)
        self._last = note
        self._held[i], self._stamp[i] = True, self._clock
        v.age = self._clock
        # a steal can leave more held voices than polyphony only via set_polyphony; handled there

    def note_off(self, note) -> None:
        note = _note(note)
        if note is None:
            return
        for i, v in enumerate(self.voices):
            if self._held[i] and v.note == note and v.is_active():
                self._release(i)

    def active_notes(self) -> list[int]:
        return [self.voices[i].note for i in self._held_slots()]

    def panic(self) -> None:
        """Release everything: each active voice fades to silence over the 5 ms steal fade."""
        for i, v in enumerate(self.voices):
            if v.is_active():
                v.begin_steal(v.note, 0.0)
                v.note_off()  # cancels the retrigger; the fade ends in silence
            self._held[i] = False

    def render(self, out, p, mods, resolve=None) -> None:
        for v in self.voices:
            if v.is_active():
                v.render(out, p, mods if resolve is None else resolve(v, mods))
