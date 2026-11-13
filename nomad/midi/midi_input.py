"""MIDI input: pure message parsing, a port wrapper and an engine router.

Channels: omni. Every channel is accepted and reported in ``MidiEvent.channel``
(0-15); the router ignores it. Filtering by channel is deliberately not done.
"""
from __future__ import annotations

from dataclasses import dataclass

import mido


@dataclass
class MidiEvent:
    kind: str  # "note_on" | "note_off" | "cc" | "bend"
    note: int = 0
    velocity: float = 0.0
    cc: int = 0
    value: int = 0
    channel: int = 0
    port: str = ""
    bend: float = 0.0


def parse_message(msg, port: str) -> MidiEvent | None:
    """mido message -> MidiEvent, or None for anything unsupported/malformed."""
    try:
        t = msg.type
        ch = int(getattr(msg, "channel", 0))
        if t in ("note_on", "note_off"):
            vel = min(max(int(msg.velocity), 0), 127)
            if t == "note_off" or vel == 0:
                return MidiEvent("note_off", note=int(msg.note), channel=ch, port=port)
            return MidiEvent("note_on", note=int(msg.note), velocity=vel / 127.0, channel=ch, port=port)
        if t == "control_change":
            return MidiEvent("cc", cc=int(msg.control), value=int(msg.value), channel=ch, port=port)
        if t == "pitchwheel":
            return MidiEvent("bend", bend=min(max(int(msg.pitch) / 8191.0, -1.0), 1.0), channel=ch, port=port)
    except (AttributeError, TypeError, ValueError):
        pass
    return None  # clock, sysex, aftertouch, program change, junk...


class MidiInput:
    """One open mido input port; events go to ``on_event`` on rtmidi's thread."""

    def __init__(self, on_event) -> None:
        self._on_event = on_event
        self._port = None
        self._name = ""
        self.errors = 0  # handler exceptions swallowed (must never kill the rtmidi thread)

    @property
    def is_open(self) -> bool:
        return self._port is not None and not getattr(self._port, "closed", False)

    def list_ports(self) -> list[str]:
        try:
            return list(mido.get_input_names())
        except Exception:  # backend missing / ALSA unavailable
            return []

    def open(self, port_name: str | None = None) -> None:
        self.close()
        names = self.list_ports()
        if port_name is None:
            if not names:
                raise RuntimeError("no MIDI input ports available")
            port_name = names[0]
        elif port_name not in names:
            raise RuntimeError(f"MIDI port {port_name!r} not found; available: {names}")
        name = port_name
        self._name = name
        try:
            self._port = mido.open_input(name, callback=lambda m: self._dispatch(m, name))
        except Exception as exc:
            raise RuntimeError(f"cannot open MIDI port {name!r}: {exc}; available: {names}") from exc

    def _dispatch(self, msg, name: str) -> None:
        try:
            ev = parse_message(msg, name)
            if ev is not None:
                self._on_event(ev)
        except Exception:
            self.errors += 1

    def close(self) -> None:
        port, self._port = self._port, None
        if port is not None:
            try:
                port.close()
            except Exception:
                pass


class MidiRouter:
    """Translates events into engine calls. Not thread-safe: feed from one thread.

    shortcut: CC1/64/120/123 are consumed here and not forwarded to ``on_cc``;
    all other CCs go to ``on_cc(cc, value, channel)``.
    """

    def __init__(self, engine, on_cc=None) -> None:
        self._engine = engine
        self._on_cc = on_cc
        self._sustain = False
        self._held: set[int] = set()     # keys physically down
        self._pending: set[int] = set()  # released while pedal down

    def handle(self, e: MidiEvent) -> None:
        eng = self._engine
        if e.kind == "note_on":
            self._held.add(e.note)
            self._pending.discard(e.note)  # re-tap cancels its pending off
            eng.note_on(e.note, e.velocity)
        elif e.kind == "note_off":
            self._held.discard(e.note)
            if self._sustain:
                self._pending.add(e.note)
            else:
                eng.note_off(e.note)
        elif e.kind == "bend":
            eng.set_control("bend", e.bend)
        elif e.kind == "cc":
            self._cc(e)

    def _cc(self, e: MidiEvent) -> None:
        eng = self._engine
        if e.cc == 1:
            eng.set_control("modwheel", e.value / 127.0)  # unipolar: wheel at rest = 0
        elif e.cc == 64:
            self._sustain = e.value >= 64
            if not self._sustain:
                self._flush_pending()
        elif e.cc == 123:  # all notes off
            self._flush_pending()
            for n in sorted(self._held):
                eng.note_off(n)
            self._held.clear()
        elif e.cc == 120:  # all sound off
            self._held.clear()
            self._pending.clear()
            eng.panic()
        elif self._on_cc is not None:
            self._on_cc(e.cc, e.value, e.channel)

    def _flush_pending(self) -> None:
        for n in sorted(self._pending):
            self._engine.note_off(n)
        self._pending.clear()
