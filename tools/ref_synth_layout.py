"""Synth panel layout reference: sections, widget kinds, selector labels, short names."""

from __future__ import annotations

import json
from pathlib import Path

from nomad.engine.params import REGISTRY
from nomad.gui.synth_panel import HIDDEN, LAYOUT, _labels, _short, widget_kind


def sections():
    out = []
    for key, names in LAYOUT.items():
        names = [n for n in names if n not in HIDDEN]
        title = key.upper().replace("_", " ")
        if key == "global":  # the VOICE section of the right column: master lives in the mixer
            names, title = [n for n in names if n != "master_volume"], "VOICE"
        out.append({"key": key, "title": title, "params": names})
    return out


def write(out_dir: Path) -> None:
    out_dir = Path(out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    visible = [n for s in sections() for n in s["params"]]
    data = {
        "sections": sections(),
        "kinds": {n: widget_kind(n) for n in REGISTRY},
        "labels": {n: _labels(n) for n in REGISTRY if widget_kind(n) == "selector"},
        "offsets": {n: 1 if n == "polyphony" else 0 for n in REGISTRY if widget_kind(n) == "selector"},
        "short": {n: _short(n) for n in visible + ["master_volume"]},
    }
    (out_dir / "synth_layout.json").write_text(json.dumps(data, indent=1) + "\n")
