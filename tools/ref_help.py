"""Help-mode texts: ``help_for(key)`` for every parameter and panel element.

The Rust app embeds this file (``include_str!``), so the texts stay those of
``nomad/gui/help.py``; the pytest fails when they drift.
"""

from __future__ import annotations

import json
from pathlib import Path

from nomad.engine.drums import DRUM_REGISTRY
from nomad.engine.params import REGISTRY
from nomad.gui import help as gui_help


def keys() -> list[str]:
    out = list(REGISTRY) + list(DRUM_REGISTRY) + list(gui_help._FIXED) + list(gui_help._ELEMENTS)
    return list(dict.fromkeys(out))


def write(out_dir: Path) -> None:
    texts, missing = {}, []
    for k in keys():
        try:
            texts[k] = list(gui_help.help_for(k))
        except KeyError:
            missing.append(k)
    data = {"texts": texts, "missing": missing}
    (Path(out_dir) / "help.json").write_text(json.dumps(data, indent=1, ensure_ascii=False) + "\n")
