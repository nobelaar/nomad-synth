"""Instrument presets: sound plus pattern, factory (in code) and user (JSON files).

``capture``/``apply`` talk to an instrument through the same caller-thread stores
the GUI uses, so they are safe while audio runs. Tempo, mix level, mute and the
clock source are session state and are not part of a preset. Whether the
sequencer is on (SEQ) is also left alone: loading a sound never starts playback.

User files are untrusted input: ``check`` validates the whole preset before
``apply`` touches the instrument, so a bad file changes nothing.
"""

from __future__ import annotations

import json
import math
import os
import re
from pathlib import Path

import numpy as np

from nomad.engine.clock import STEPS
from nomad.engine.drums import DRUM_REGISTRY, INSTRUMENTS
from nomad.engine.params import REGISTRY
from nomad.engine.sequencer import EXTRA, NONE

__all__ = ["PresetStore", "FACTORY", "capture", "apply", "check", "slug"]

VERSION = 1
MAX_BYTES = 64 * 1024
_SYNTH_SKIP = {"master_volume"}
_DRUM_SKIP = {"bpm"}
_DIVISIONS = (0.25, 0.5, 1.0, 2.0, 4.0)


def _num(x) -> bool:
    return isinstance(x, (int, float)) and not isinstance(x, bool) and math.isfinite(x)


def _ints(x, n, lo, hi) -> bool:
    return isinstance(x, list) and len(x) == n and all(
        isinstance(v, int) and not isinstance(v, bool) and lo <= v <= hi for v in x)


# ---- capture / validate / apply ---------------------------------------------------------------
def capture(inst, name: str = "") -> dict:
    if inst.kind == "synth":
        sq = inst.sequencer
        return {"version": VERSION, "kind": "synth", "name": name,
                "params": {n: float(v) for n, v in inst._values.items() if n not in _SYNTH_SKIP},
                "seq": {"on": sq.on.tolist(), "note": sq.note.tolist(), "vel": sq.vel.tolist(),
                        "extra": sq.extra.tolist(), "gate": sq.gate, "division": sq.grid.division,
                        "length": sq.grid.length, "swing": sq.grid.swing}}
    return {"version": VERSION, "kind": "drums", "name": name,
            "params": {n: float(v) for n, v in inst.values.items() if n not in _DRUM_SKIP},
            "seq": {"steps": inst.steps.tolist()}}


def check(kind: str, data) -> dict:
    """Return ``data`` if it is a well-formed preset of ``kind``; raise ValueError otherwise."""
    if not isinstance(data, dict) or data.get("kind") != kind:
        raise ValueError(f"not a {kind} preset")
    params, seq = data.get("params"), data.get("seq")
    if not isinstance(params, dict) or not isinstance(seq, dict):
        raise ValueError("missing params/seq")
    if not all(isinstance(k, str) and _num(v) for k, v in params.items()):
        raise ValueError("bad param value")
    if kind == "synth":
        ok = (_ints(seq.get("on"), STEPS, 0, 1) and _ints(seq.get("note"), STEPS, 0, 127)
              and _ints(seq.get("vel"), STEPS, 1, 127)
              and isinstance(seq.get("extra"), list) and len(seq["extra"]) == STEPS
              and all(_ints(r, EXTRA, 0, NONE) and all(v == NONE or v <= 127 for v in r) for r in seq["extra"])
              and all(_num(seq.get(k)) for k in ("gate", "division", "swing"))
              and _ints([seq.get("length")], 1, 1, STEPS))
        if not ok:
            raise ValueError("bad sequence")
    else:
        rows = seq.get("steps")
        if not (isinstance(rows, list) and len(rows) == len(INSTRUMENTS)
                and all(_ints(r, STEPS, 0, 2) for r in rows)):
            raise ValueError("bad pattern")
    return data


def apply(inst, data: dict) -> None:
    """Load a preset into ``inst`` (validates first; unknown param names are skipped)."""
    data = check(inst.kind, data)
    seq = data["seq"]
    if inst.kind == "synth":
        for n, v in data["params"].items():
            if n in REGISTRY and n not in _SYNTH_SKIP:
                inst.set_param(n, v)
        sq = inst.sequencer
        sq.gate = seq["gate"]
        g = sq.grid
        g.division = min(_DIVISIONS, key=lambda d: abs(d - seq["division"]))
        g.length, g.swing = seq["length"], min(max(float(seq["swing"]), 0.0), 0.6)
        sq.note[:], sq.vel[:] = seq["note"], seq["vel"]
        sq.extra[:] = seq["extra"]
        sq.on[:] = seq["on"]
    else:
        for n, v in data["params"].items():
            if n in DRUM_REGISTRY and n not in _DRUM_SKIP:
                inst.set_value(n, v)
        inst.steps[:] = np.array(seq["steps"], dtype=np.uint8)


# ---- factory presets (complete param sets: nothing leaks from the previous sound) ---------------
def _synth(name, over, notes, *, on=None, vel=None, division=0.25, length=STEPS, gate=0.5, extra=None):
    params = {n: s.default for n, s in REGISTRY.items() if n not in _SYNTH_SKIP}
    params.update(over)
    n = len(notes)
    pad = lambda row, fill: list(row) + [fill] * (STEPS - len(row))
    return {"version": VERSION, "kind": "synth", "name": name, "params": params,
            "seq": {"on": pad(on if on is not None else [1] * n, 0), "note": pad(notes, 60),
                    "vel": pad(vel if vel is not None else [100] * n, 100),
                    "extra": pad(extra or [], [NONE] * EXTRA) if extra else [[NONE] * EXTRA] * STEPS,
                    "gate": gate, "division": division, "length": length, "swing": 0.0}}


def _chord(*notes):
    return list(notes) + [NONE] * (EXTRA - len(notes))


def _drums(name, tracks, over=None):
    params = {n: s.default for n, s in DRUM_REGISTRY.items() if n not in _DRUM_SKIP}
    params.update(over or {})
    steps = [[0] * STEPS for _ in INSTRUMENTS]
    for track, hits in tracks.items():
        for step, level in hits.items():
            steps[INSTRUMENTS.index(track)][step] = level
    return {"version": VERSION, "kind": "drums", "name": name, "params": params, "seq": {"steps": steps}}


# wave: 0 sin 1 tri 2 saw 3 sqr; modN_src: 1 lfo1 2 lfo2 3 mw 4 vel 5 env2; modN_dst: 1 pitch 2 p1 3 p2 4 amp 5 cut
FACTORY: dict[str, list[dict]] = {
    "synth": [
        _synth("Pad de acordes", {
            "osc1_wave": 2, "osc2_wave": 2, "osc1_fine": -9, "osc2_fine": 9, "osc1_level": 0.45, "osc2_level": 0.45,
            "filt_cutoff": 2200, "filt_reso": 0.2, "filt_env_amt": 0.25,
            "amp_attack": 900, "amp_decay": 800, "amp_sustain": 0.85, "amp_release": 2500,
            "filt_attack": 1200, "filt_decay": 1500, "filt_sustain": 0.5, "filt_release": 2000,
            "lfo1_wave": 1, "lfo1_rate": 0.25, "lfo1_depth": 1.0, "mod0_src": 1, "mod0_dst": 5, "mod0_amt": 0.15,
            "polyphony": 6},
            [45, 41, 48, 43], vel=[90] * 4, division=4.0, length=4, gate=1.0,
            extra=[_chord(48, 52), _chord(45, 48), _chord(52, 55), _chord(47, 50)]),
        _synth("Bajo", {
            "osc1_wave": 2, "osc2_wave": 3, "osc2_coarse": -12, "osc1_level": 0.6, "osc2_level": 0.4,
            "filt_cutoff": 400, "filt_reso": 0.45, "filt_env_amt": 0.6,
            "amp_attack": 2, "amp_decay": 220, "amp_sustain": 0.5, "amp_release": 120,
            "filt_attack": 1, "filt_decay": 200, "filt_sustain": 0.1, "filt_release": 100, "polyphony": 1},
            [36, 36, 39, 36, 34, 41], gate=0.45,
            on=[1, 0, 0, 1, 0, 0, 1, 0, 1, 0, 0, 1, 0, 0, 1, 0]),
        _synth("Lead", {
            "osc1_wave": 3, "osc1_pw": 0.4, "osc2_wave": 2, "osc2_fine": 7, "osc1_level": 0.5, "osc2_level": 0.35,
            "filt_cutoff": 3500, "filt_reso": 0.25, "filt_env_amt": 0.3,
            "amp_attack": 8, "amp_decay": 250, "amp_sustain": 0.7, "amp_release": 300,
            "lfo1_wave": 1, "lfo1_rate": 5.5, "lfo1_depth": 1.0, "mod0_src": 1, "mod0_dst": 1, "mod0_amt": 0.04,
            "mod1_src": 3, "mod1_dst": 5, "mod1_amt": 0.3, "polyphony": 1},
            [69, 72, 76, 72, 69, 67, 64, 67], length=8, gate=0.8),
        _synth("Pluck", {
            "osc1_wave": 2, "osc2_wave": 1, "osc2_coarse": 12, "osc1_level": 0.5, "osc2_level": 0.3,
            "filt_cutoff": 600, "filt_reso": 0.35, "filt_env_amt": 0.8,
            "amp_attack": 1, "amp_decay": 350, "amp_sustain": 0.0, "amp_release": 250,
            "filt_attack": 1, "filt_decay": 300, "filt_sustain": 0.0, "polyphony": 6},
            [57, 60, 64, 69] * 4, division=0.5, gate=0.5),
    ],
    "drums": [
        _drums("4x4", {"kick": {0: 2, 4: 1, 8: 1, 12: 1}, "clap": {4: 1, 12: 1},
                       "chat": {2: 1, 6: 1, 10: 1, 14: 1}}),
        _drums("Breakbeat", {"kick": {0: 2, 6: 1, 10: 1}, "snare": {4: 2, 12: 2, 7: 1},
                             "chat": {i: 1 for i in range(0, STEPS, 2)}, "ohat": {14: 1}}, {"snare_decay": 0.4}),
        _drums("Minimal", {"kick": {0: 2, 8: 1}, "clap": {12: 1}, "tom": {11: 1},
                           "chat": {2: 1, 6: 1, 10: 1, 14: 1}}, {"swing": 0.3, "kick_decay": 0.6}),
    ],
}


# ---- storage ------------------------------------------------------------------------------------
def slug(name: str) -> str:
    return re.sub(r"[^A-Za-z0-9 _-]", "", name).strip()[:40]


class PresetStore:
    """Factory presets plus ``<dir>/<kind>/<slug>.json``; ``dir=None`` means factory only."""

    def __init__(self, directory: Path | None = None) -> None:
        self.dir = Path(directory) if directory is not None else None

    @property
    def can_save(self) -> bool:
        return self.dir is not None

    def _user_files(self, kind: str):
        d = self.dir / kind if self.dir is not None else None
        return sorted(d.glob("*.json")) if d is not None and d.is_dir() else []

    def _load(self, path: Path, kind: str) -> dict | None:
        try:
            if path.stat().st_size > MAX_BYTES:
                return None
            return check(kind, json.loads(path.read_text(encoding="utf-8")))
        except (OSError, ValueError):  # JSONDecodeError is a ValueError; a bad file is just not listed
            return None

    def list(self, kind: str) -> list[tuple[str, str]]:
        names = [(p["name"], "factory") for p in FACTORY[kind]]
        for f in self._user_files(kind):
            d = self._load(f, kind)
            if d is not None and isinstance(d.get("name"), str):
                names.append((d["name"], "user"))
        return names

    def get(self, kind: str, name: str) -> dict:
        for p in FACTORY[kind]:
            if p["name"] == name:
                return p
        for f in self._user_files(kind):
            d = self._load(f, kind)
            if d is not None and d.get("name") == name:
                return d
        raise KeyError(name)

    def save(self, kind: str, name: str, data: dict) -> str:
        """Write a user preset; returns the stored name. ValueError for a bad or reserved name."""
        clean = slug(name)
        if not clean:
            raise ValueError("nombre vacío")
        if clean.lower() in {p["name"].lower() for p in FACTORY[kind]}:
            raise ValueError("ese nombre es de un preset de fábrica")
        if self.dir is None:
            raise ValueError("no hay carpeta de presets")
        data = check(kind, {**data, "name": clean})
        path = self.dir / kind / f"{clean}.json"
        path.parent.mkdir(parents=True, exist_ok=True)
        tmp = path.with_name(path.name + ".tmp")
        try:
            tmp.write_text(json.dumps(data, indent=1), encoding="utf-8")
            os.replace(tmp, path)
        finally:
            tmp.unlink(missing_ok=True)
        return clean
