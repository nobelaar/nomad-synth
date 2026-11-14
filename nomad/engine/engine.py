"""Engine facade: command queue, control-block loop, modulation and master limiter.

Threading: ``note_on/note_off/set_param/set_control/panic`` may be called from
any thread; they validate their arguments on the CALLER thread (unknown param
-> KeyError, NaN -> default/0, ranges clamped) and push a tuple on a
``queue.SimpleQueue``. ``render`` (audio thread) drains the queue first, inside
a try/except so a bad event can never kill the audio callback.

The Engine is a mixer: it owns an immutable ``Topology`` (the instrument
instances) that the audio thread adopts atomically from a FIFO command queue,
mixes every instance (own buffer x ramped gain = ``level``/mute) and applies
the master stage. ``master_volume`` is the MIX volume and lives here; each
instance has its own ``level`` (0..1, default 1.0 = unity). Each root clock
(clock of an Internal instance) advances exactly once per render chunk.

Caller-thread API (``note_on`` ... ``snapshot``) targets the default synth;
synth-specific docs (threading, control block, smoothing) live in
``nomad.engine.instrument``.
"""

from __future__ import annotations

import queue

import numpy as np

from nomad.engine.clock import Clock
from nomad.engine.drums import DrumMachine
from nomad.engine.instance import CONTROL_BLOCK
from nomad.engine.instrument import PITCH_SEMITONES, SynthInstrument
from nomad.engine.params import REGISTRY, Smoother, clamp, from_norm, to_norm
from nomad.engine.topology import Topology

__all__ = ["Engine", "CONTROL_BLOCK", "PITCH_SEMITONES"]


class Engine:
    def __init__(self, sample_rate: float = 48000, max_voices: int = 8) -> None:
        self.sr = float(sample_rate)
        self.synth = SynthInstrument(self.sr, max_voices, "Synth 01", iid=1)
        self.drums = DrumMachine(self.sr, name="Machine Drum 01", iid=2)
        for inst in (self.synth, self.drums):
            inst.live = True
        self.topology = Topology([self.synth, self.drums])
        self._q: queue.SimpleQueue = queue.SimpleQueue()
        self._grave: queue.SimpleQueue = queue.SimpleQueue()  # old topologies go back to the GUI thread
        spec = REGISTRY["master_volume"]
        self._master_value = spec.default  # caller-side target (snapshot)
        self._master_sm = Smoother(to_norm("master_volume", spec.default), self.sr, spec.smooth_ms)
        self._master_p = self._master = spec.default
        self._buf = np.zeros(CONTROL_BLOCK)
        self._gain = np.zeros(CONTROL_BLOCK)
        self._ramp = np.arange(1, CONTROL_BLOCK + 1, dtype=np.float64)

    # aliases kept for callers/tests that reach into the default instances
    vm = property(lambda self: self.synth.vm)
    matrix = property(lambda self: self.synth.matrix)
    lfos = property(lambda self: self.synth.lfos)
    errors = property(lambda self: self.synth.errors)
    _p = property(lambda self: self.synth._p)
    _mods = property(lambda self: self.synth._mods)
    _resolve = property(lambda self: self.synth._resolve)
    clock = property(lambda self: self.drums.clock)
    seq_clock = property(lambda self: self.synth.clock)
    instruments = property(lambda self: self.topology.insts)

    # ---- caller-thread API -------------------------------------------------
    def note_on(self, note, velocity) -> None:
        self.synth.note_on(note, velocity)

    def note_off(self, note) -> None:
        self.synth.note_off(note)

    def set_param(self, name: str, value: float) -> None:
        if name == "master_volume":
            v = clamp(name, value)
            self._master_value = v
            self._q.put(("master", v))
        else:
            self.synth.set_param(name, value)

    def set_control(self, name: str, value: float) -> None:
        self.synth.set_control(name, value)

    def panic(self) -> None:
        for inst in self.topology.insts:
            inst.panic()

    def play(self) -> None:
        self.drums.start()

    def stop_transport(self) -> None:
        self.drums.stop()

    def toggle_transport(self) -> None:
        self.drums.toggle()

    def snapshot(self) -> dict:
        s = self.synth.snapshot()
        s["master_volume"] = self._master_value
        return s

    def publish(self, topology: Topology, handovers=(), closing=()) -> None:
        """GUI thread: hand a new immutable topology to the audio thread.

        ``handovers`` = ``(dst_clock, src_clock)`` pairs; the audio thread copies
        bpm/pos/running src -> dst once, on adoption, so continuity is exact. ``closing`` instances are
        silenced by the audio thread right after that copy (never before: their clock is still the source).
        """
        for inst in topology.insts:
            inst.live = True
        self._q.put(("topo", topology, tuple(handovers), tuple(closing)))

    def collect(self) -> list:
        """GUI thread: take back topologies the audio thread has replaced (last refs die here)."""
        out = []
        while True:
            try:
                out.append(self._grave.get_nowait())
            except queue.Empty:
                return out

    # ---- audio thread ------------------------------------------------------
    def _drain(self) -> None:
        q = self._q
        while True:
            try:
                kind, a, *rest = q.get_nowait()
            except queue.Empty:
                return
            if kind == "master":
                self._master_sm.set(to_norm("master_volume", a))
            elif kind == "topo":
                for dst, src in rest[0]:
                    dst.bpm, dst.pos, dst.running = src.bpm, src.pos, src.running
                for inst in rest[1]:
                    inst.close_now()
                for inst, eclock in zip(a.insts, a.eff):
                    inst.eclock = eclock
                self._grave.put(self.topology)
                self.topology = a

    @staticmethod
    def _limit(insts) -> int:
        n = 1 << 30
        for inst in insts:
            n = min(n, inst.frames_to_event())
        return n

    def _span(self, insts, out, start: int, chunk: int) -> None:
        """Render ``chunk`` frames (no event inside) in control-block slices."""
        buf = self._buf
        end = start + chunk
        for s in range(start, end, CONTROL_BLOCK):
            n = min(CONTROL_BLOCK, end - s)
            b = buf[:n]
            b[:] = 0.0
            for inst in insts:
                ib = inst.buf[:n]
                ib[:] = 0.0
                inst.render_block(ib, n)
                g0, g1 = inst._igain, inst.gain_target
                if g0 != 1.0 or g1 != 1.0:
                    g = self._gain[:n]
                    np.multiply(self._ramp[:n], (g1 - g0) / n, out=g)
                    g += g0
                    ib *= g
                inst._igain = g1
                b += ib
            sm = self._master_sm
            if sm.value != sm.target:
                self._master_p = from_norm("master_volume", sm.advance(n))
            g0, g1 = self._master, self._master_p
            g = self._gain[:n]
            np.multiply(self._ramp[:n], (g1 - g0) / n, out=g)
            g += g0
            b *= g
            np.tanh(b, out=b)
            out[s : s + n] = b
            self._master = g1
        for inst in insts:
            inst.advance(chunk)

    def render(self, out: np.ndarray) -> None:
        self._drain()
        topo = self.topology
        insts = topo.insts
        for inst in insts:
            inst.drain()
        total, i = out.shape[0], 0
        while i < total:
            for inst in insts:
                inst.poll()
            chunk = min(total - i, self._limit(insts))
            self._span(insts, out, i, chunk)
            for clock in topo.roots:
                clock.advance(chunk)
            i += chunk
        for inst in insts:
            inst.poll()  # events landing exactly on the buffer edge fire now, not one render later
