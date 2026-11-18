"""GUI logic reference: KnobModel formats and gestures, scope trigger/points."""

from __future__ import annotations

import json
from pathlib import Path

import numpy as np

from nomad.engine.drums import DRUM_REGISTRY
from nomad.engine.params import REGISTRY
from nomad.gui.knob import KnobModel
from nomad.gui.scope_view import find_trigger, to_points
from tools.refio import BlobWriter

EDGES = {
    "ms": [99.95, 100.0, 999.9, 1000.0, 9.95, 1.25, 2.5, 1234.5],
    "Hz": [9999.0, 10000.0, 9.995, 99.95, 100.5, 1250.0, 12345.0, 20.5],
    "st": [-0.4, 0.4, -0.5, 0.5, 2.5, -2.5, 0.0],
    "ct": [-0.4, 0.4, -0.5, 0.5, 2.5, -2.5],
    "": [0.005, 0.125, 0.625, -0.005, -0.125, 0.0, 0.335],
    "BPM": [110.0, 119.4, 60.0, 123.456789],
    "voices": [1.0, 4.5, 3.5],
}


def format_cases():
    out = []
    for reg_name, reg in (("synth", REGISTRY), ("drums", DRUM_REGISTRY)):
        for name, spec in reg.items():
            vals = [spec.lo, spec.default, spec.hi, spec.lo + (spec.hi - spec.lo) * 0.37]
            vals += [v for v in EDGES.get(spec.unit, []) if spec.lo <= v <= spec.hi]
            for v in vals:
                m = KnobModel(spec, v)
                out.append({"reg": reg_name, "param": name, "value": v, "norm": m.norm, "text": m.format_value()})
    return out


GESTURES = [
    ("filt_cutoff", [["drag", 40, False], ["drag", -7, True], ["wheel", 3, False], ["wheel", -1, True],
                     ["drag", 1000, False], ["wheel", -200, False], ["reset"]]),
    ("osc1_coarse", [["drag", 13, False], ["wheel", 1, True], ["drag", 0, False], ["reset"]]),
    ("amp_attack", [["wheel", 5, False], ["drag", -3, True], ["reset"]]),
]


def gesture_cases():
    out = []
    for name, ops in GESTURES:
        m = KnobModel(REGISTRY[name], REGISTRY[name].default)
        steps = []
        for op in ops:
            if op[0] == "drag":
                m.set_from_drag(op[1], op[2])
            elif op[0] == "wheel":
                m.set_from_wheel(op[1], op[2])
            else:
                m.reset()
            steps.append({"norm": m.norm, "value": m.value, "text": m.format_value()})
        out.append({"param": name, "ops": ops, "steps": steps})
    return out


def signals():
    t = np.arange(2000) / 48000.0
    rng = np.random.default_rng(9)
    sigs = {
        "sine_440": np.sin(2 * np.pi * 440 * t),
        "sine_55": 0.5 * np.sin(2 * np.pi * 55 * t + 1.0),
        "sine_3k": np.sin(2 * np.pi * 3000 * t),
        "noise": rng.standard_normal(2000) * 0.3,
        "noise_dc": rng.standard_normal(2000) * 0.1 + 0.5,
        "empty": np.zeros(0),
        "constant": np.full(1000, 0.25),
        "neg_zero": np.array([-0.0, 0.0, -0.0, 0.1, -0.1, 0.0]),
        "with_nan": np.where(np.arange(2000) % 97 == 0, np.nan, np.sin(2 * np.pi * 300 * t[:2000])),
        "with_inf": np.where(np.arange(500) == 7, np.inf, np.sin(2 * np.pi * 1000 * t[:500])),
        "short": np.array([0.3]),
        "tiny_saw": (np.arange(300) % 50) / 25.0 - 1.0,
    }
    return {k: np.asarray(v, dtype=np.float32) for k, v in sigs.items()}


def write_scope(out_dir: Path) -> None:
    w = BlobWriter("gui_scope")
    for name, sig in signals().items():
        w.add(f"sig/{name}", sig.astype(np.float64))
        for width, height in ((250, 126), (37, 50)):
            pts = to_points(sig, width, height)
            w.add(f"pts/{name}/{width}x{height}", np.array(pts), signal=f"sig/{name}", width=width,
                  height=height, trigger=find_trigger(sig))
    w.write(out_dir)


def write(out_dir: Path) -> None:
    out_dir = Path(out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    data = {"format": format_cases(), "gestures": gesture_cases()}
    (out_dir / "gui_logic.json").write_text(json.dumps(data, indent=0) + "\n")
    write_scope(out_dir)
