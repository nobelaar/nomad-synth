"""MIDI parse reference: raw bytes -> parse_message(mido.Message.from_bytes(bytes))."""

from __future__ import annotations

import json
from pathlib import Path

import mido

from nomad.midi.midi_input import parse_message


def cases() -> list[list[int]]:
    out = []
    for ch in (0, 7, 15):
        for vel in (0, 1, 64, 127):
            out.append([0x90 | ch, 60, vel])
            out.append([0x80 | ch, 61, vel])
    for cc in (1, 64, 120, 123, 7, 21, 0, 127):
        out.append([0xB0, cc, 100])
    out += [[0xB5, 74, 0], [0xBF, 21, 127]]
    for v in (0x0000, 0x2000, 0x3FFF, 0x1FFF, 0x2001):
        out.append([0xE0, v & 0x7F, v >> 7])
    out.append([0xE9, 0x7F, 0x7F])
    out += [[0xF8], [0xFE], [0xFF], [0xF0, 0x7E, 0xF7], [0xF0], [0xC0, 5], [0xD0, 40], [0xA0, 60, 30],
            [0x90, 60], [0x90, 128, 64], [0x90, 60, 200], [0x90, 60, 64, 1], [], [0x40, 1, 2], [0xB0, 1]]
    return out


def event(b):
    try:
        msg = mido.Message.from_bytes(b)
    except (ValueError, TypeError, IndexError):
        return None
    e = parse_message(msg, "")
    if e is None:
        return None
    return {"kind": e.kind, "note": e.note, "velocity": e.velocity, "cc": e.cc, "value": e.value,
            "channel": e.channel, "bend": e.bend}


def write(out_dir: Path) -> None:
    out_dir = Path(out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    data = [{"bytes": b, "event": event(b)} for b in cases()]
    (out_dir / "midi_parse.json").write_text(json.dumps(data, indent=1) + "\n")
