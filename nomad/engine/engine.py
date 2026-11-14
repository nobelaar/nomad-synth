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

import numpy as np

from nomad.engine.drums import DrumMachine
from nomad.engine.instrument import CONTROL_BLOCK, PITCH_SEMITONES, SynthInstrument
from nomad.engine.transport import Transport

__all__ = ["Engine", "CONTROL_BLOCK", "PITCH_SEMITONES"]


class Engine:
    """Host: sums instruments and the drum machine, applies the master limiter.

    The caller-thread API delegates to the default synth (``self.synth``); the
    synth-specific docs (threading, control block, smoothing) live in
    ``nomad.engine.instrument``.
    """

    def __init__(self, sample_rate: float = 48000, max_voices: int = 8) -> None:
        self.sr = float(sample_rate)
        self.transport = Transport(self.sr)
        self.synth = SynthInstrument(self.sr, max_voices, transport=self.transport)
        self.instruments = [self.synth]
        self.drums = DrumMachine(self.sr, transport=self.transport)
        self._was_playing = False
        self._out = None
        self._buf = np.zeros(CONTROL_BLOCK)
        self._gain = np.zeros(CONTROL_BLOCK)
        self._ramp = np.arange(1, CONTROL_BLOCK + 1, dtype=np.float64)
        self._master = self.synth._p["master_volume"]

    # aliases kept for callers/tests that reach into the default synth
    vm = property(lambda self: self.synth.vm)
    matrix = property(lambda self: self.synth.matrix)
    lfos = property(lambda self: self.synth.lfos)
    errors = property(lambda self: self.synth.errors)
    _p = property(lambda self: self.synth._p)
    _mods = property(lambda self: self.synth._mods)
    _resolve = property(lambda self: self.synth._resolve)

    # ---- caller-thread API -------------------------------------------------
    def note_on(self, note, velocity) -> None:
        self.synth.note_on(note, velocity)

    def note_off(self, note) -> None:
        self.synth.note_off(note)

    def set_param(self, name: str, value: float) -> None:
        self.synth.set_param(name, value)

    def set_control(self, name: str, value: float) -> None:
        self.synth.set_control(name, value)

    def panic(self) -> None:
        self.synth.panic()
        self.drums.stop_all()

    def play(self) -> None:
        self.transport.start()

    def stop_transport(self) -> None:
        self.transport.stop()

    def toggle_transport(self) -> None:
        self.transport.toggle()

    def snapshot(self) -> dict:
        return self.synth.snapshot()

    # ---- audio thread ------------------------------------------------------
    def _limit(self) -> int:
        n = self.drums.frames_to_event()
        for inst in self.instruments:
            n = min(n, inst.frames_to_event())
        return n

    def _on_step(self, pos: int) -> None:
        self.drums.on_step(pos)
        for inst in self.instruments:
            inst.on_step(pos)

    def _span(self, start: int, chunk: int) -> None:
        """Render ``chunk`` frames (no step boundary inside) in control-block slices."""
        buf, out = self._buf, self._out
        end = start + chunk
        for s in range(start, end, CONTROL_BLOCK):
            n = min(CONTROL_BLOCK, end - s)
            b = buf[:n]
            b[:] = 0.0
            for inst in self.instruments:
                inst.render_block(b, n)
            self.drums.render_chunk(b, 0, n)
            g0, g1 = self._master, self.synth._p["master_volume"]
            g = self._gain[:n]
            np.multiply(self._ramp[:n], (g1 - g0) / n, out=g)
            g += g0
            b *= g
            np.tanh(b, out=b)
            out[s : s + n] = b
            self._master = g1
        for inst in self.instruments:
            inst.advance(chunk)

    def render(self, out: np.ndarray) -> None:
        for inst in self.instruments:
            inst.drain()
        if self._was_playing and not self.transport.playing:
            for inst in self.instruments:
                inst.on_stop()
        self._was_playing = self.transport.playing
        self.drums._ensure(out.shape[0])
        self._out = out
        self.transport.run(out.shape[0], self._span, self._on_step, self._limit)
