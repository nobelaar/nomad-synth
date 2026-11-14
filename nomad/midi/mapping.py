"""MIDI learn: CC -> parameter bindings, persisted as JSON.

Rules: one CC key (port, channel, cc) maps to one param; a param has at most
one CC (learning it again drops its old CC). Exact (port, channel, cc) wins;
otherwise the same (channel, cc) on any other port matches.

Threading: ``handle`` runs on the rtmidi thread, ``arm``/``unbind``/``load``
on the GUI thread. Shared state is only ever replaced by a single attribute
assignment (``_armed``; ``bindings`` is copy-on-write), never mutated in
place, so readers see a consistent old or new value under the GIL. shortcut:
an arm() racing the very CC that consumes the previous arm may be lost; the
user just presses LEARN again.
"""
from __future__ import annotations

import json
import os
from pathlib import Path

from nomad.engine.params import REGISTRY, from_norm
from nomad.midi.midi_input import MidiEvent

VERSION = 1
_DISCRETE_SUFFIXES = ("_wave", "_mode", "_sync", "_src", "_dst")


def _is_discrete(name: str) -> bool:
    return name == "polyphony" or name.endswith(_DISCRETE_SUFFIXES)


def _is_int(x, lo: int, hi: int) -> bool:
    return isinstance(x, int) and not isinstance(x, bool) and lo <= x <= hi


class MidiMap:
    def __init__(self, path: Path | None = None) -> None:
        self.path = Path(path) if path is not None else None
        self.bindings: dict[tuple[str, int, int], str] = {}
        self.error: str | None = None  # set by load() when the file was unusable
        self._armed: str | None = None

    @property
    def learning(self) -> str | None:
        return self._armed

    def arm(self, param: str | None) -> None:
        """Bind the next CC to ``param``; None cancels. Unknown param -> KeyError."""
        if param is not None and param not in REGISTRY:
            raise KeyError(param)
        self._armed = param

    def unbind(self, param: str) -> None:
        self.bindings = {k: v for k, v in self.bindings.items() if v != param}

    def handle(self, event: MidiEvent) -> tuple[str, float] | None:
        if event.kind != "cc":
            return None
        key = (event.port, event.channel, event.cc)
        armed = self._armed
        if armed is not None:
            self._armed = None
            b = {k: v for k, v in self.bindings.items() if v != armed}
            b[key] = armed
            self.bindings = b
            param = armed
        else:
            b = self.bindings
            param = b.get(key)
            if param is None:
                param = next((v for (_, ch, cc), v in b.items()
                              if (ch, cc) == (event.channel, event.cc)), None)
            if param is None:
                return None
        value = from_norm(param, event.value / 127.0)
        if _is_discrete(param):
            spec = REGISTRY[param]
            value = min(max(float(round(value)), spec.lo), spec.hi)
        return param, value

    def save(self) -> None:
        if self.path is None:
            return
        data = {"version": VERSION, "bindings": [
            {"port": p, "channel": ch, "cc": cc, "param": v}
            for (p, ch, cc), v in self.bindings.items()]}
        self.path.parent.mkdir(parents=True, exist_ok=True)
        tmp = self.path.with_name(self.path.name + ".tmp")
        try:
            tmp.write_text(json.dumps(data, indent=2), encoding="utf-8")
            os.replace(tmp, self.path)
        finally:
            tmp.unlink(missing_ok=True)

    def load(self) -> None:
        """Replace bindings from disk. Never raises; bad files give an empty map + ``error``."""
        if self.path is None:
            return
        self.error = None
        self.bindings = {}
        if not self.path.exists():
            return
        try:
            data = json.loads(self.path.read_text(encoding="utf-8"))
            if data["version"] != VERSION:
                raise ValueError(f"unsupported map version {data['version']!r}")
            entries = data["bindings"]
            if not isinstance(entries, list):
                raise TypeError("bindings is not a list")
        except (OSError, ValueError, KeyError, TypeError) as exc:
            self.error = f"{self.path}: {exc}"
            return
        b: dict[tuple[str, int, int], str] = {}
        for e in entries:
            if (isinstance(e, dict) and isinstance(e.get("port"), str)
                    and _is_int(e.get("channel"), 0, 15) and _is_int(e.get("cc"), 0, 127)
                    and isinstance(e.get("param"), str) and e["param"] in REGISTRY):
                b[(e["port"], e["channel"], e["cc"])] = e["param"]
        self.bindings = b
