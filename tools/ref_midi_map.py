"""MidiMap reference: operation scenarios, a file Python saved, and bad files.

Ports are generic test names; nothing from the author's real midi-map.json.
"""

from __future__ import annotations

import json
import tempfile
from pathlib import Path

from nomad.midi.mapping import MidiMap
from nomad.midi.midi_input import MidiEvent

P = "TEST:TEST MIDI 1 24:0"
Q = "OTHER:OTHER MIDI 2 28:0"


def ev(port, ch, cc, value, kind="cc"):
    return ["event", port, ch, cc, value, kind]


SCENARIOS = {
    "learn_binds_next_cc": [["arm", "filt_cutoff"], ev(P, 0, 21, 64), ev(P, 0, 21, 127)],
    "learn_disarms": [["arm", "filt_reso"], ev(P, 0, 22, 10), ev(P, 0, 23, 10)],
    "fallback_other_port": [["arm", "filt_cutoff"], ev(P, 0, 21, 64), ev(Q, 0, 21, 100), ev(Q, 1, 21, 100)],
    "exact_beats_fallback": [["arm", "filt_cutoff"], ev(P, 0, 21, 0), ["arm", "filt_reso"], ev(Q, 0, 21, 0),
                             ev(P, 0, 21, 127), ev(Q, 0, 21, 127), ev("X", 0, 21, 127)],
    "relearn_drops_old": [["arm", "filt_cutoff"], ev(P, 0, 21, 5), ["arm", "filt_reso"], ev(P, 0, 23, 5),
                          ["arm", "filt_cutoff"], ev(P, 0, 22, 5), ev(P, 0, 21, 5),
                          ["arm", "filt_cutoff"], ev(P, 0, 23, 99), ev(P, 0, 23, 99)],
    "discrete_rounded": [["arm", "osc1_wave"], ev(P, 0, 30, 80), ["arm", "polyphony"], ev(P, 0, 31, 127),
                         ["arm", "lfo1_wave"], ev(P, 0, 32, 64), ev(P, 0, 32, 95), ["arm", "mod0_dst"],
                         ev(P, 0, 33, 64)],
    "unknown_cc_none": [ev(P, 0, 50, 64), ["arm", None], ev(P, 0, 50, 64)],
    "non_cc_keeps_armed": [["arm", "filt_reso"], ev(P, 0, 60, 100, "note_on"), ev(P, 0, 40, 64)],
    "unbind": [["arm", "filt_cutoff"], ev(P, 0, 21, 64), ["unbind", "filt_cutoff"], ev(P, 0, 21, 64)],
    "continuous_exp": [["arm", "filt_cutoff"], ev(P, 0, 21, 0), ev(P, 0, 21, 64), ev(P, 0, 21, 127),
                       ["arm", "amp_attack"], ev(P, 0, 22, 32)],
    "save_load_roundtrip": [["arm", "filt_cutoff"], ev(P, 0, 21, 1), ["arm", "glide"], ev(Q, 3, 7, 1),
                            ["save"], ["load"], ev(Q, 3, 7, 127)],
}

BAD = {
    "corrupt.json": '{"version": 1, "bindings": [',
    "wrong_version.json": json.dumps({"version": 2, "bindings": []}),
    "bindings_not_list.json": json.dumps({"version": 1, "bindings": {"a": 1}}),
    "no_version.json": json.dumps({"bindings": []}),
    "bad_entries.json": json.dumps({"version": 1, "bindings": [
        {"port": P, "channel": 0, "cc": 21, "param": "filt_cutoff"},
        {"port": 5, "channel": 0, "cc": 22, "param": "filt_reso"},
        {"port": P, "channel": 16, "cc": 22, "param": "filt_reso"},
        {"port": P, "channel": 0, "cc": 128, "param": "filt_reso"},
        {"port": P, "channel": True, "cc": 22, "param": "filt_reso"},
        {"port": P, "channel": 0, "cc": 22.0, "param": "filt_reso"},
        "not a dict",
        {"port": P, "channel": 0, "cc": 23, "param": "nope"},
        {"port": Q, "channel": 2, "cc": 24, "param": "glide"},
        {"port": P, "channel": 0, "cc": 21, "param": "filt_reso"},
    ]}, indent=2),
    "unknown_param.json": json.dumps({"version": 1, "bindings": [
        {"port": P, "channel": 0, "cc": 21, "param": "master_volume"},
        {"port": P, "channel": 0, "cc": 22, "param": "does_not_exist"}]}, indent=2),
    # Same shape as a real user file (two knobs), generic port name.
    "real_shape.json": json.dumps({"version": 1, "bindings": [
        {"port": P, "channel": 0, "cc": 21, "param": "filt_cutoff"},
        {"port": P, "channel": 0, "cc": 22, "param": "filt_reso"}]}, indent=2),
}


def bindings(m):
    return [[p, ch, cc, v] for (p, ch, cc), v in m.bindings.items()]


def run(ops, path):
    m = MidiMap(path)
    steps = []
    for op in ops:
        r = None
        if op[0] == "arm":
            m.arm(op[1])
        elif op[0] == "unbind":
            m.unbind(op[1])
        elif op[0] == "save":
            m.save()
        elif op[0] == "load":
            m.load()
        else:
            _, port, ch, cc, value, kind = op
            out = m.handle(MidiEvent(kind, cc=cc, value=value, channel=ch, port=port, note=cc))
            r = list(out) if out is not None else None
        steps.append({"result": r, "bindings": bindings(m), "learning": m.learning})
    return steps


def write(out_dir: Path) -> None:
    out_dir = Path(out_dir)
    files = out_dir / "midi_map_files"
    files.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory() as tmp:
        scen = {k: {"ops": ops, "steps": run(ops, Path(tmp) / f"{k}.json")} for k, ops in SCENARIOS.items()}
        saved = Path(tmp) / "save_load_roundtrip.json"
        (files / "saved_after_learn.json").write_bytes(saved.read_bytes())
    for name, text in BAD.items():
        (files / name).write_text(text)
    loads = {}
    for f in sorted(files.iterdir()):
        m = MidiMap(f)
        m.load()
        loads[f.name] = {"bindings": bindings(m), "error": m.error is not None}
    data = {"scenarios": scen, "files": loads}
    (out_dir / "midi_map.json").write_text(json.dumps(data, indent=1) + "\n")
