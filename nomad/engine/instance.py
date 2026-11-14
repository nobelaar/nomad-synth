"""Common base of every instrument instance hosted by the ``Engine``.

State ownership (one writer per field):
  * audio-owned: ``clock.pos/running/bpm`` once published, ``_igain`` and the
    effective ``_level``/``_muted`` (changed only by ``_apply`` on the audio thread);
  * commands: anything the GUI wants to change goes through ``command()``. A
    *published* instance (``live``) queues it and the audio thread applies it at
    the start of the next render; an unpublished one (tests, offline use)
    applies it immediately.
"""

from __future__ import annotations

import math
import queue

import numpy as np

from nomad.engine.clock import Clock

__all__ = ["InstrumentBase", "CONTROL_BLOCK"]

CONTROL_BLOCK = 256  # frames; see R6 in the ledger


class InstrumentBase:
    kind = "base"

    def __init__(self, sample_rate: float, name: str = "", iid: int = 0, clock: Clock | None = None) -> None:
        self.sr = float(sample_rate)
        self.name, self.id = name, iid
        self.clock = clock if clock is not None else Clock(self.sr)
        self.source: InstrumentBase | None = None  # None = Internal; set by the manager (GUI) before publishing
        self.live = False
        self.eclock = self.clock  # effective clock: own if Internal, else the source's (set on topology adoption)
        self.buf = np.zeros(CONTROL_BLOCK)  # per-instance scratch for level/mute
        self._cq: queue.SimpleQueue = queue.SimpleQueue()
        self._level, self._muted, self._igain = 1.0, False, 1.0
        self.cmd_errors = 0

    # ---- caller thread -----------------------------------------------------
    def command(self, name: str, arg=None) -> None:
        if self.live:
            self._cq.put((name, arg))
        else:
            self._apply(name, arg)

    def start(self) -> None:
        self.command("start")

    def stop(self) -> None:
        self.command("stop")

    def toggle(self) -> None:
        # reads audio-owned state; start/stop are idempotent so a stale read only drops a click
        self.command("stop" if self.clock.running else "start")

    def set_bpm(self, bpm: float) -> None:
        self.command("bpm", bpm)

    def set_mute(self, muted: bool) -> None:
        self.command("mute", bool(muted))

    def set_level(self, level: float) -> None:
        self.command("level", level)

    # ---- audio thread ------------------------------------------------------
    def _apply(self, name: str, arg) -> None:
        if name == "start":
            self.clock.start()
        elif name == "stop":
            self.clock.stop()
        elif name == "bpm":
            v = float(arg)
            if math.isfinite(v):
                self.clock.bpm = min(max(v, 20.0), 300.0)
        elif name == "mute":
            self._muted = bool(arg)
        elif name == "level":
            v = float(arg)
            self._level = min(max(v, 0.0), 1.0) if math.isfinite(v) else 1.0
        else:
            self._apply_extra(name, arg)

    def _apply_extra(self, name: str, arg) -> None:
        pass

    def drain_commands(self) -> None:
        q = self._cq
        while True:
            try:
                name, arg = q.get_nowait()
            except queue.Empty:
                return
            try:
                self._apply(name, arg)
            except Exception:  # a bad command must never kill the audio callback
                self.cmd_errors += 1

    @property
    def gain_target(self) -> float:
        return 0.0 if self._muted else self._level

    # ---- protocol the Engine drives (overridden) ----------------------------
    def drain(self) -> None:
        self.drain_commands()

    def render_block(self, b: np.ndarray, n: int) -> None:
        raise NotImplementedError

    def poll(self) -> None:
        pass

    def frames_to_event(self) -> int:
        return 1 << 30

    def advance(self, n: int) -> None:
        pass

    def panic(self) -> None:
        self.command("stop")

    def close_now(self) -> None:
        """Audio thread, when a topology that is closing this instance is adopted (after the clock handover)."""
        self._apply("stop", None)
