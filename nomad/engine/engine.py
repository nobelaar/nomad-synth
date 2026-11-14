"""Engine facade: command queue, control-block loop, modulation and master limiter.

Threading: ``note_on/note_off/set_param/set_control/panic`` may be called from
any thread; they validate their arguments on the CALLER thread (unknown param
-> KeyError, NaN -> default/0, ranges clamped) and push a tuple on a
``queue.SimpleQueue``. ``render`` (audio thread) drains the queue first, inside
a try/except so a bad event can never kill the audio callback.

Control block (R6): ``render`` splits ``len(out)`` into ``min(256, remaining)``
frame blocks. LFOs, mod matrix, smoothers and voices advance once per block;
the filter inside Voice subdivides itself. Callback sizes that are not
multiples of 256 give different (but equally valid) control timing.

Modulation units: the matrix amount (-1..1) times the source is scaled here:
pitch/pitch1/pitch2 x ``PITCH_SEMITONES`` (12: amount 1 with a full-scale
source = one octave); cutoff/pw/amp are scaled by Voice (x2 octaves, x0.45,
gain 1+amp). ``bend`` = smoothed bend control x ``bend_range`` semitones.
``env2`` as a source is the voice's filter envelope at the end of its previous
block (one block lag).

Smoothing: continuous params go through a one-pole ``Smoother`` per control
block, in the 0..1 knob domain (so exp params glide exponentially); the master
gain is additionally interpolated linearly across each block, so a knob jump
never steps the output. Envelope times/sustain, waves, modes, sync, polyphony,
LFO wave/phase and mod-slot params apply immediately (mod-slot amount steps).

Master: ``tanh(x * master_volume)`` (soft limiter; resonance can peak ~1.8).
No per-voice 1/sqrt(n) compensation: tanh handles chords (simplest reading).

Allocation: render uses preallocated scratch only. Known temporaries remain
inside scipy ``lfilter`` (Voice filter), ``Envelope.process`` and the oscillator
sync path; none go through the np.empty/zeros/... constructors.
"""

from __future__ import annotations

import math
import queue

import numpy as np

from nomad.engine.drums import DrumMachine
from nomad.engine.lfo import LFO
from nomad.engine.modmatrix import NUM_SLOTS, ModMatrix
from nomad.engine.params import REGISTRY, Smoother, clamp, from_norm, to_norm
from nomad.engine.voice_manager import VoiceManager

__all__ = ["Engine", "CONTROL_BLOCK", "PITCH_SEMITONES"]

CONTROL_BLOCK = 256  # frames; see R6 in the ledger
PITCH_SEMITONES = 12.0  # semitones per unit of matrix pitch modulation
_CTRL_SMOOTH_MS = 10.0

_IMMEDIATE_SUFFIX = ("_wave", "_mode", "_sync", "_phase", "_src", "_dst", "_attack", "_decay", "_sustain", "_release")
_SMOOTHED = tuple(
    n for n in REGISTRY
    if n != "polyphony" and not n.endswith(_IMMEDIATE_SUFFIX) and not (n.startswith("mod") and n.endswith("_amt"))
)
_CONTROLS = ("bend", "modwheel")


class Engine:
    def __init__(self, sample_rate: float = 48000, max_voices: int = 8) -> None:
        self.sr = float(sample_rate)
        self.vm = VoiceManager(self.sr, max_voices)
        self.drums = DrumMachine(self.sr)
        self.matrix = ModMatrix()
        self.lfos = (LFO(self.sr, seed=1), LFO(self.sr, seed=2))
        self._values = {n: s.default for n, s in REGISTRY.items()}  # caller-side targets (snapshot)
        self._ctl = {"bend": 0.0, "modwheel": 0.0}
        self._p = dict(self._values)  # what voices read (smoothed)
        self._smoothers = {n: Smoother(to_norm(n, REGISTRY[n].default), self.sr, REGISTRY[n].smooth_ms) for n in _SMOOTHED}
        self._csm = {c: Smoother(0.0, self.sr, _CTRL_SMOOTH_MS) for c in _CONTROLS}
        self._slots = [[0, 0, 0.0] for _ in range(NUM_SLOTS)]
        self._q: queue.SimpleQueue = queue.SimpleQueue()
        self._src = {"lfo1": 0.0, "lfo2": 0.0, "modwheel": 0.0, "velocity": 0.0, "env2": 0.0}
        self._mods = dict.fromkeys(("pitch", "pitch1", "pitch2", "amp", "cutoff", "pw1", "pw2", "bend"), 0.0)
        self._buf = np.zeros(CONTROL_BLOCK)
        self._gain = np.zeros(CONTROL_BLOCK)
        self._ramp = np.arange(1, CONTROL_BLOCK + 1, dtype=np.float64)
        self._master = self._p["master_volume"]
        self.errors = 0  # events dropped by the audio-thread handler (diagnostics)

    # ---- caller-thread API -------------------------------------------------
    def note_on(self, note, velocity) -> None:
        self._q.put((0, note, velocity))

    def note_off(self, note) -> None:
        self._q.put((1, note, None))

    def set_param(self, name: str, value: float) -> None:
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
        elif kind == 1:
            self.vm.note_off(a)
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
            self.vm.panic()
            self.drums.stop_all()

    def _drain(self) -> None:
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

    def render(self, out: np.ndarray) -> None:
        self._drain()
        total = out.shape[0]
        p, buf = self._p, self._buf
        for s in range(0, total, CONTROL_BLOCK):
            n = min(CONTROL_BLOCK, total - s)
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

            b = buf[:n]
            b[:] = 0.0
            self.vm.render(b, p, self._mods, self._resolve)
            self.drums.render(b, n)
            g0, g1 = self._master, p["master_volume"]
            g = self._gain[:n]
            np.multiply(self._ramp[:n], (g1 - g0) / n, out=g)
            g += g0
            b *= g
            np.tanh(b, out=b)
            out[s : s + n] = b
            self._master = g1
