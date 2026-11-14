"""One synth instance: voices, parameters, LFOs, mod matrix and a command queue.

Everything the audio path of the old ``Engine`` owned for the synth lives here
(see the Engine docstring history: threading, control block, modulation units,
smoothing). ``Engine`` is now a host that sums instruments; this class renders
into a buffer it is given (``render_block``) and knows nothing about the master
stage or the transport.
"""

from __future__ import annotations

import math
import queue

import numpy as np

from nomad.engine.lfo import LFO
from nomad.engine.modmatrix import NUM_SLOTS, ModMatrix
from nomad.engine.sequencer import StepSequencer
from nomad.engine.clock import Clock
from nomad.engine.instance import CONTROL_BLOCK, InstrumentBase
from nomad.engine.params import REGISTRY, Smoother, clamp, from_norm, to_norm
from nomad.engine.voice_manager import VoiceManager, _note

__all__ = ["SynthInstrument", "CONTROL_BLOCK", "PITCH_SEMITONES"]

PITCH_SEMITONES = 12.0  # semitones per unit of matrix pitch modulation
_CTRL_SMOOTH_MS = 10.0
_IMMEDIATE_SUFFIX = ("_wave", "_mode", "_sync", "_phase", "_src", "_dst", "_attack", "_decay", "_sustain", "_release")
_SMOOTHED = tuple(
    n for n in REGISTRY
    if n not in ("polyphony", "master_volume") and not n.endswith(_IMMEDIATE_SUFFIX) and not (n.startswith("mod") and n.endswith("_amt"))
)
_CONTROLS = ("bend", "modwheel")


class SynthInstrument(InstrumentBase):
    kind = "synth"

    def __init__(self, sample_rate: float = 48000, max_voices: int = 8, name: str = "Synth 01",
                 clock: Clock | None = None, iid: int = 0) -> None:
        super().__init__(sample_rate, name, iid, clock)
        self.vm = VoiceManager(self.sr, max_voices)
        self._live = bytearray(128)  # keys held via note_on/note_off events (not the sequencer)
        self.sequencer = StepSequencer(self, self.sr)
        self.matrix = ModMatrix()
        self.lfos = (LFO(self.sr, seed=1), LFO(self.sr, seed=2))
        self._values = {n: s.default for n, s in REGISTRY.items() if n != "master_volume"}  # master lives in Engine; caller-side targets (snapshot)
        self._ctl = {"bend": 0.0, "modwheel": 0.0}
        self._p = dict(self._values)  # what voices read (smoothed)
        self._smoothers = {n: Smoother(to_norm(n, REGISTRY[n].default), self.sr, REGISTRY[n].smooth_ms) for n in _SMOOTHED}
        self._csm = {c: Smoother(0.0, self.sr, _CTRL_SMOOTH_MS) for c in _CONTROLS}
        self._slots = [[0, 0, 0.0] for _ in range(NUM_SLOTS)]
        self._q: queue.SimpleQueue = queue.SimpleQueue()
        self._src = {"lfo1": 0.0, "lfo2": 0.0, "modwheel": 0.0, "velocity": 0.0, "env2": 0.0}
        self._mods = dict.fromkeys(("pitch", "pitch1", "pitch2", "amp", "cutoff", "pw1", "pw2", "bend"), 0.0)
        self.errors = 0  # events dropped by the audio-thread handler (diagnostics)

    # ---- caller-thread API -------------------------------------------------
    def note_on(self, note, velocity) -> None:
        self._q.put((0, note, velocity))

    def note_off(self, note) -> None:
        self._q.put((1, note, None))

    def set_param(self, name: str, value: float) -> None:
        if name == "master_volume":
            raise KeyError(name)  # the mix volume belongs to Engine; this instance has ``level``
        v = clamp(name, value)  # KeyError for unknown names, NaN -> default
        self._values[name] = v
        self._q.put((2, name, v))

    def set_control(self, name: str, value: float) -> None:
        if name not in _CONTROLS:
            raise KeyError(name)
        v = float(value)
        v = min(max(v, -1.0), 1.0) if math.isfinite(v) else 0.0
        self._ctl[name] = v
        self._q.put((3, name, v))

    def panic(self) -> None:
        self._q.put((4, None, None))
        self.command("stop")

    def snapshot(self) -> dict:
        """Copy of all param targets + ``bend``/``modwheel`` + ``active_notes`` (held keys).

        Params/controls reflect calls immediately; active_notes is audio-thread
        state read without a lock (benign race, may lag one render).
        """
        s = dict(self._values)
        s.update(self._ctl)
        s["active_notes"] = self.vm.active_notes()
        return s

    # ---- audio thread ------------------------------------------------------
    def _handle(self, kind, a, b) -> None:
        if kind == 0:
            self.vm.note_on(a, b)
            n = _note(a)
            if n is not None:
                self._live[n] = 1
        elif kind == 1:
            self.vm.note_off(a)
            n = _note(a)
            if n is not None:
                self._live[n] = 0
        elif kind == 2:
            if a in self._smoothers:
                self._smoothers[a].set(to_norm(a, b))
            else:
                self._p[a] = b
            if a == "polyphony":
                self.vm.set_polyphony(int(b))
            elif a.startswith("mod"):
                i, field = int(a[3 : a.index("_")]), a[a.index("_") + 1 :]
                slot = self._slots[i]
                slot[("src", "dst", "amt").index(field)] = b if field == "amt" else int(round(b))
                self.matrix.set_slot(i, slot[0], slot[1], slot[2])
        elif kind == 3:
            self._csm[a].set(b)
        else:
            self._live[:] = bytes(128)
            self.vm.panic()
            self.sequencer.release()

    def close_now(self) -> None:
        super().close_now()
        self._handle(4, None, None)  # vm.panic + release the sequencer note + forget live keys

    def drain(self) -> None:
        self.drain_commands()
        q = self._q
        while True:
            try:
                ev = q.get_nowait()
            except queue.Empty:
                return
            try:
                self._handle(*ev)
            except Exception:  # a bad event must never kill the audio callback
                self.errors += 1

    def _resolve(self, voice, _mods) -> dict:
        src = self._src
        src["velocity"], src["env2"] = voice.velocity, voice.env2_level
        r = self.matrix.resolve(src)
        m = self._mods
        m["pitch"] = r["pitch"] * PITCH_SEMITONES
        m["pitch1"] = r["pitch1"] * PITCH_SEMITONES
        m["pitch2"] = r["pitch2"] * PITCH_SEMITONES
        m["amp"], m["cutoff"], m["pw1"], m["pw2"] = r["amp"], r["cutoff"], r["pw1"], r["pw2"]
        return m  # bend is set once per block, not per voice

    def render_block(self, b: np.ndarray, n: int) -> None:
        """Advance control state by one block of ``n`` frames and ADD the voices into ``b``."""
        p = self._p
        for name, sm in self._smoothers.items():
            if sm.value != sm.target:
                p[name] = from_norm(name, sm.advance(n))
        for sm in self._csm.values():
            if sm.value != sm.target:
                sm.advance(n)
        sec = n / self.sr
        for k, lfo in enumerate(self.lfos, 1):
            lfo.set(p[f"lfo{k}_rate"], p[f"lfo{k}_wave"], p[f"lfo{k}_depth"], p[f"lfo{k}_phase"])
            self._src[f"lfo{k}"] = lfo.advance(sec)
        self._src["modwheel"] = self._csm["modwheel"].value
        self._mods["bend"] = self._csm["bend"].value * p["bend_range"]
        self.vm.render(b, p, self._mods, self._resolve)

    # ---- clock hooks (audio thread) ----
    def poll(self) -> None:
        self.sequencer.poll()

    def frames_to_event(self) -> int:
        return self.sequencer.frames_to_event()

    def advance(self, n: int) -> None:
        self.sequencer.advance(n)
