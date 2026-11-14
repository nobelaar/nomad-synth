"""Modulation matrix: eight slots, each routing one source to one destination.

Slot indices match the registry's ``mod{i}_src`` / ``mod{i}_dst`` /
``mod{i}_amt`` parameters (src 0..5, dst 0..7, amt -1..1). Per R2 the engine
resolves per voice: it passes that voice's ``velocity``/``env2`` together with
the global ``lfo1``/``lfo2``/``modwheel`` values. Sources missing from the
input dict count as 0.0.
"""

from __future__ import annotations

import math

__all__ = ["ModMatrix", "SOURCES", "DESTINATIONS", "NUM_SLOTS"]

SOURCES = ["off", "lfo1", "lfo2", "modwheel", "velocity", "env2"]
DESTINATIONS = ["off", "pitch", "pitch1", "pitch2", "amp", "cutoff", "pw1", "pw2"]
NUM_SLOTS = 8


class ModMatrix:
    def __init__(self) -> None:
        self._src = [0] * NUM_SLOTS
        self._dst = [0] * NUM_SLOTS
        self._amt = [0.0] * NUM_SLOTS
        self._out = {name: 0.0 for name in DESTINATIONS[1:]}

    def set_slot(self, i: int, src: int, dst: int, amount: float) -> None:
        """Configure slot ``i``; bad indices raise ValueError, bad amount -> 0."""
        if not 0 <= i < NUM_SLOTS:
            raise ValueError(f"slot {i} out of range")
        if not 0 <= src < len(SOURCES):
            raise ValueError(f"src {src} out of range")
        if not 0 <= dst < len(DESTINATIONS):
            raise ValueError(f"dst {dst} out of range")
        amount = float(amount)
        amount = max(-1.0, min(1.0, amount)) if math.isfinite(amount) else 0.0
        self._src[i], self._dst[i], self._amt[i] = int(src), int(dst), amount

    def resolve(self, sources: dict[str, float]) -> dict[str, float]:
        """Sum amount * source per destination.

        The returned dict is owned by the matrix and reused (and overwritten)
        on every call, so no allocation happens per voice per block; copy it
        if it must outlive the next ``resolve``.
        """
        out = self._out
        for k in out:
            out[k] = 0.0
        for i in range(NUM_SLOTS):
            s, d = self._src[i], self._dst[i]
            if s and d:
                out[DESTINATIONS[d]] += self._amt[i] * sources.get(SOURCES[s], 0.0)
        return out
