"""On-screen keyboard reference: note_at over a grid of points."""

from __future__ import annotations

import json
from pathlib import Path

from nomad.gui.keyboard import note_at

YS = [None, 0, 20, 41, 42, 60, 83, -1, 84]
HEIGHT = 84


def write(out_dir: Path) -> None:
    out_dir = Path(out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    cases = []
    for octaves in (1, 2, 6):
        for base in (36, 48):
            for width in (350, 1350):
                xs = list(range(0, width, 7)) + [-1, width - 0.5, width, width + 5]
                grid = [[note_at(x, y, width, HEIGHT, octaves, base) for y in YS] for x in xs]
                cases.append({"octaves": octaves, "base": base, "width": width, "height": HEIGHT,
                              "xs": xs, "ys": YS, "notes": grid})
    (out_dir / "keyboard.json").write_text(json.dumps(cases, separators=(",", ":")) + "\n")
