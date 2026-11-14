"""Registry of instrument types the manager can create.

Engine side only: a type is a kind id, a label for the tab/menu and a factory.
The GUI keeps its own kind -> panel map, so adding a Sampler later means one
entry here plus one panel factory there.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Callable

from nomad.engine.drums import DrumMachine
from nomad.engine.instance import InstrumentBase
from nomad.engine.instrument import SynthInstrument

__all__ = ["InstrumentType", "INSTRUMENT_TYPES"]


@dataclass(frozen=True)
class InstrumentType:
    kind: str
    label: str
    create: Callable[[float, int, str], InstrumentBase]  # (sample_rate, id, name)


INSTRUMENT_TYPES: dict[str, InstrumentType] = {
    "synth": InstrumentType("synth", "Synth", lambda sr, iid, name: SynthInstrument(sr, 8, name, iid=iid)),
    "drums": InstrumentType("drums", "Machine Drum", lambda sr, iid, name: DrumMachine(sr, name=name, iid=iid)),
}
