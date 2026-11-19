"""MidiRouter reference: event lists -> trace of calls on a fake engine."""

from __future__ import annotations

import json
from pathlib import Path

from nomad.midi.midi_input import MidiEvent, MidiRouter


class Fake:
    def __init__(self, trace):
        self.t = trace

    def note_on(self, n, v):
        self.t.append(["note_on", n, v])

    def note_off(self, n):
        self.t.append(["note_off", n])

    def set_control(self, name, v):
        self.t.append(["control", name, v])

    def panic(self):
        self.t.append(["panic"])


def on(n, v=100):
    return {"kind": "note_on", "note": n, "velocity": v / 127.0}


def off(n):
    return {"kind": "note_off", "note": n}


def cc(c, v, ch=0):
    return {"kind": "cc", "cc": c, "value": v, "channel": ch}


SCENARIOS = {
    "basic_chord": [on(60), on(64), on(67), off(64), off(60), off(67)],
    "vel0_is_off": [on(60), {"kind": "note_off", "note": 60}],
    "sustain_hold_release": [on(60), on(64), cc(64, 127), off(60), off(64), on(67), off(67), cc(64, 0)],
    "retap_cancels_pending": [cc(64, 100), on(60), off(60), on(60), cc(64, 10), off(60)],
    "cc123_all_notes_off": [on(48), on(52), cc(64, 127), off(48), cc(123, 0), on(55)],
    "cc120_panic": [on(60), cc(64, 127), off(60), cc(120, 0), cc(64, 0)],
    "modwheel_bend": [cc(1, 0), cc(1, 64), cc(1, 127), {"kind": "bend", "bend": -1.0},
                      {"kind": "bend", "bend": 0.5}],
    "other_cc_forwarded": [cc(21, 5, 3), cc(7, 127, 15), cc(74, 0)],
    "reset_releases_everything": [on(62), on(60), cc(64, 127), off(62), on(71), "reset", on(50), off(50)],
    "sustain_threshold": [on(60), cc(64, 63), off(60), on(61), cc(64, 64), off(61), cc(64, 63)],
}


def full(e):
    base = {"kind": "", "note": 0, "velocity": 0.0, "cc": 0, "value": 0, "channel": 0, "bend": 0.0}
    base.update(e)
    return base


def run(events):
    trace = []
    r = MidiRouter(Fake(trace), on_cc=lambda c, v, ch: trace.append(["cc", c, v, ch]))
    for e in events:
        if e == "reset":
            r.reset()
        else:
            f = full(e)
            r.handle(MidiEvent(f["kind"], note=f["note"], velocity=f["velocity"], cc=f["cc"], value=f["value"],
                               channel=f["channel"], bend=f["bend"]))
    return trace


def write(out_dir: Path) -> None:
    out_dir = Path(out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    data = {label: {"events": [e if e == "reset" else full(e) for e in evs], "trace": run(evs)}
            for label, evs in SCENARIOS.items()}
    (out_dir / "midi_router.json").write_text(json.dumps(data, indent=1) + "\n")
