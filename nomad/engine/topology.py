"""Immutable snapshot of the instrument graph, adopted by the audio thread as one unit."""

from __future__ import annotations

__all__ = ["Topology"]


class Topology:
    """``insts`` in mix order; ``roots`` = clocks of Internal instances (advanced once per chunk)."""

    def __init__(self, insts) -> None:
        self.insts = tuple(insts)
        self.roots = tuple(i.clock for i in self.insts if i.source is None)
        # effective clock per instance, frozen here so the audio thread never reads the GUI-written ``source``
        self.eff = tuple((i.source.clock if i.source is not None else i.clock) for i in self.insts)
