"""GUI-thread authority over the instrument graph.

Only this class mutates the graph (instances, clock sources). Every change is
published to the audio thread as an immutable ``Topology`` through
``Engine.publish``; the audio thread never sees a half-applied change.

Clock sources: any Internal instance may be followed by any number of
instances (one-to-many); chains are rejected. A follower that loses its source
(source closed, or user picks Internal) gets a *handover*: the audio thread
copies bpm/pos/running from the old source clock into the follower's own
clock when it adopts the new topology, so playback continues from the exact
musical position.
"""

from __future__ import annotations

from nomad.engine.instance import InstrumentBase
from nomad.engine.registry import INSTRUMENT_TYPES
from nomad.engine.topology import Topology

__all__ = ["InstrumentManager", "CLOSE_DELAY_MS"]

CLOSE_DELAY_MS = 100  # panic fade is 5 ms; keep the closing instance rendering a bit longer


class InstrumentManager:
    def __init__(self, engine, schedule=None) -> None:
        """``schedule(delay_ms, fn)`` defers the final drop of a closed instance (Tk ``after``)."""
        self.engine, self._schedule = engine, schedule
        self._insts: list[InstrumentBase] = list(engine.topology.insts)
        self._closing: list[InstrumentBase] = []
        self._next_id = max((i.id for i in self._insts), default=0) + 1
        self._counters = {k: sum(1 for i in self._insts if i.kind == k) for k in INSTRUMENT_TYPES}

    @property
    def instances(self) -> tuple:
        return tuple(self._insts)

    def followers(self, inst) -> list:
        return [i for i in self._insts if i.source is inst]

    def _publish(self, handovers=(), closing=()) -> None:
        self.engine.publish(Topology(self._insts + self._closing), handovers, closing)

    def add(self, kind: str) -> InstrumentBase:
        t = INSTRUMENT_TYPES[kind]  # KeyError for unknown kinds
        self._counters[kind] += 1
        inst = t.create(self.engine.sr, self._next_id, f"{t.label} {self._counters[kind]:02d}")
        self._next_id += 1
        self._insts.append(inst)
        self._publish()
        return inst

    def set_source(self, inst, src) -> bool:
        """``src=None`` = Internal. False (and no change) if the graph rules reject it."""
        if inst not in self._insts or src is inst:
            return False
        if src is not None and (src not in self._insts or src.source is not None):
            return False  # dead, or itself a follower: would be a chain
        if src is not None and self.followers(inst):
            return False  # inst is a source for others: it cannot become a follower
        if src is inst.source:
            return True
        old = inst.source
        inst.source = src
        self._publish([(inst.clock, old.clock)] if src is None else ())
        return True

    def remove(self, inst) -> None:
        if inst not in self._insts:
            raise ValueError(inst)
        handovers = []
        for f in self.followers(inst):
            f.source = None
            handovers.append((f.clock, inst.clock))
        inst.source = None
        self._insts.remove(inst)
        self._closing.append(inst)
        # the audio thread copies the handovers, THEN silences inst, in one message: no ordering race with
        # inst's own command queue (a separate inst.panic() could land first and rewind its clock)
        self._publish(handovers, closing=(inst,))
        drop = lambda: self._drop(inst)  # noqa: E731
        if self._schedule is not None:
            self._schedule(CLOSE_DELAY_MS, drop)
        else:
            drop()

    def _drop(self, inst) -> None:
        if inst in self._closing:
            self._closing.remove(inst)
            self._publish()

    # ---- per-instance controls (always commands) -----------------------------
    def start(self, inst) -> bool:
        return self._own(inst, inst.start)

    def stop(self, inst) -> bool:
        return self._own(inst, inst.stop)

    def set_bpm(self, inst, bpm: float) -> bool:
        return self._own(inst, lambda: inst.set_bpm(bpm))

    def set_mute(self, inst, muted: bool) -> None:
        inst.set_mute(muted)

    def set_level(self, inst, level: float) -> None:
        inst.set_level(level)

    @staticmethod
    def _own(inst, fn) -> bool:
        if inst.source is not None:
            return False  # a follower inherits transport and tempo from its source
        fn()
        return True

    def collect(self) -> list:
        return self.engine.collect()
