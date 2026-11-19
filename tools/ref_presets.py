"""Presets reference: FACTORY, ``check`` and ``slug`` results, float formatting and files.

``preset_files/`` holds presets saved by Python's ``PresetStore.save`` (byte-exact
targets for the Rust writer) plus hand-written bad files. Nothing here reads the
author's real preset folder.
"""

from __future__ import annotations

import copy
import json
import struct
import tempfile
from pathlib import Path

from nomad.engine.drums import DrumMachine
from nomad.engine.instrument import SynthInstrument
from nomad.presets import FACTORY, MAX_BYTES, PresetStore, capture, check, slug

SR = 48000.0


def _synth_ok():
    return copy.deepcopy(FACTORY["synth"][0])


def _drums_ok():
    return copy.deepcopy(FACTORY["drums"][0])


def _with(base, path, value):
    d = base()
    node = d
    for k in path[:-1]:
        node = node[k]
    if value is _DEL:
        del node[path[-1]]
    else:
        node[path[-1]] = value
    return d


_DEL = object()

CHECKS = [
    ("synth", "synth_ok", _synth_ok),
    ("drums", "drums_ok", _drums_ok),
    ("drums", "synth_as_drums", _synth_ok),
    ("synth", "drums_as_synth", _drums_ok),
    ("synth", "not_a_dict", lambda: [1, 2]),
    ("synth", "no_params", lambda: _with(_synth_ok, ["params"], _DEL)),
    ("synth", "seq_list", lambda: _with(_synth_ok, ["seq"], [])),
    ("synth", "param_string", lambda: _with(_synth_ok, ["params", "glide"], "x")),
    ("synth", "param_bool", lambda: _with(_synth_ok, ["params", "glide"], True)),
    ("synth", "unknown_param_ok", lambda: _with(_synth_ok, ["params", "nope"], 1.5)),
    ("synth", "on_two", lambda: _with(_synth_ok, ["seq", "on", 3], 2)),
    ("synth", "on_float", lambda: _with(_synth_ok, ["seq", "on", 3], 1.0)),
    ("synth", "on_short", lambda: _with(_synth_ok, ["seq", "on"], [1] * 15)),
    ("synth", "vel_zero", lambda: _with(_synth_ok, ["seq", "vel", 0], 0)),
    ("synth", "note_128", lambda: _with(_synth_ok, ["seq", "note", 0], 128)),
    ("synth", "extra_128", lambda: _with(_synth_ok, ["seq", "extra", 0, 0], 128)),
    ("synth", "extra_short_row", lambda: _with(_synth_ok, ["seq", "extra", 0], [255] * 4)),
    ("synth", "gate_string", lambda: _with(_synth_ok, ["seq", "gate"], "1")),
    ("synth", "gate_int_ok", lambda: _with(_synth_ok, ["seq", "gate"], 1)),
    ("synth", "length_zero", lambda: _with(_synth_ok, ["seq", "length"], 0)),
    ("synth", "length_17", lambda: _with(_synth_ok, ["seq", "length"], 17)),
    ("synth", "length_float", lambda: _with(_synth_ok, ["seq", "length"], 4.0)),
    ("synth", "no_version_ok", lambda: _with(_synth_ok, ["version"], _DEL)),
    ("drums", "drum_level_3", lambda: _with(_drums_ok, ["seq", "steps", 0, 0], 3)),
    ("drums", "drum_five_rows", lambda: _with(_drums_ok, ["seq", "steps"], [[0] * 16] * 5)),
    ("drums", "drum_param_list", lambda: _with(_drums_ok, ["params", "kick_tune"], [0])),
]

SLUGS = ["Pad", "  Mi preset  ", "Ácido!", "a/b\\c", "", "!!!", "x" * 50, "a" * 39 + " b",
         "Bajo-2_v", "tab\tname", "ñandú 3", "  .json  "]

# json.dumps of these floats (given as bits so parsing cannot hide a formatter bug).
FLOATS = [1e-05, 0.0001, 1.5e-07, 1e16, 9999999999999998.0, 1.2345678901234568e+17, -0.0, 0.1,
          2200.0, 5e-324, 0.15, 1e22, 123456789.125, -3.5e-05, 0.45, 1e-4 * 0.999]


def _bits(x: float) -> str:
    return format(struct.unpack("<Q", struct.pack("<d", x))[0], "016x")


def _synth_instrument():
    s = SynthInstrument(SR)
    for name, v in [("filt_cutoff", 2200), ("filt_reso", 0.37), ("mod0_amt", 1e-05), ("mod0_src", 1),
                    ("mod0_dst", 5), ("glide", 60), ("polyphony", 6), ("osc1_fine", -9.5)]:
        s.set_param(name, v)
    sq = s.sequencer
    sq.set_step(0, True, 48, 90)
    sq.add_note(0, 52)
    sq.add_note(0, 55)
    sq.set_step(3, True, 41, 127)
    sq.add_note(3, 45)
    sq.set_step(7, True, 60, 1)
    sq.gate = 0.8
    sq.grid.division = 0.5
    sq.grid.length = 12
    sq.grid.swing = 0.15
    return s


def _drum_instrument():
    d = DrumMachine(SR)
    d.set_value("kick_tune", 0.3)
    d.set_value("snare_decay", 0.25)
    d.set_value("swing", 0.2)
    d.set_step("tom", 5, 2)
    d.set_step("ohat", 15, 1)
    return d


BAD_FILES = {
    "corrupt.json": '{"version": 1, "kind": "synth", "name": "Roto", "params": {',
    "wrong_kind.json": json.dumps({**FACTORY["drums"][0], "name": "Otro"}, indent=1),
    "bad_shape.json": json.dumps(_with(_synth_ok, ["seq", "on"], [1] * 3) | {"name": "Forma"}, indent=1),
    "no_name.json": json.dumps(_with(_synth_ok, ["name"], 5), indent=1),
    "oversize.json": json.dumps({**_synth_ok(), "name": "Grande", "pad": "x" * MAX_BYTES}, indent=1),
    "notes.txt": "not a preset",
}


def write(out_dir: Path) -> None:
    out_dir = Path(out_dir)
    files = out_dir / "preset_files"
    (files / "synth").mkdir(parents=True, exist_ok=True)
    (files / "drums").mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory() as tmp:
        store = PresetStore(Path(tmp))
        saved = {
            "synth": store.save("synth", "Acordes de prueba", capture(_synth_instrument(), "x")),
            "drums": store.save("drums", "Ritmo 2", capture(_drum_instrument(), "x")),
        }
        for kind, name in saved.items():
            src = Path(tmp) / kind / f"{name}.json"
            (files / kind / src.name).write_bytes(src.read_bytes())
    for name, text in BAD_FILES.items():
        (files / "synth" / name).write_text(text)
    listed = {k: PresetStore(files).list(k) for k in ("synth", "drums")}
    checks = []
    for kind, label, make in CHECKS:
        data = make()
        try:
            check(kind, data)
            err = None
        except ValueError as e:
            err = str(e)
        checks.append({"label": label, "kind": kind, "data": data, "error": err})
    out = {
        "factory": FACTORY,
        "checks": checks,
        "slugs": [[s, slug(s)] for s in SLUGS],
        "floats": [[_bits(x), json.dumps(x)] for x in FLOATS],
        "saved": saved,
        "listed": listed,
    }
    (out_dir / "presets.json").write_text(json.dumps(out, indent=1) + "\n")
