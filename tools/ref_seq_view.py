"""SeqView logic reference: note names, step labels/styles, DIV/LEN, REC, clicks.

Labels and styles follow `SeqView.refresh` line by line (the widget itself needs
Tk); clicks run `toggle_step`'s rule on a real `StepSequencer`.
"""

from __future__ import annotations

import json
from pathlib import Path

from nomad.engine.clock import STEPS
from nomad.engine.sequencer import NONE, StepSequencer
from nomad.gui.seq_view import ACCENT_VEL, DIVS, NORMAL_VEL, note_name


def label(on, note, extra):
    return [note_name(note) if on else "·", f"+{extra}" if on and extra else " "]


def style(on, vel, step, length, pos):
    live = step < length
    fill = ("accent" if vel == ACCENT_VEL else "on") if on and live else "off"
    return {"fill": fill, "dimmed": not live, "playhead": step == pos}


PATTERNS = [
    # (on, note, vel, extras) per case; length and playhead vary below
    (0, 60, 100, 0), (1, 60, 100, 0), (0, 48, 100, 2), (1, 48, 100, 2), (1, 61, 127, 1),
    (1, 72, 100, 5), (1, 0, 100, 3), (1, 127, 127, 0), (0, 59, 127, 4),
]


def step_cases():
    out = []
    for on, note, vel, extra in PATTERNS:
        for step, length, pos in ((0, 16, -1), (3, 16, 3), (10, 8, 10), (15, 16, 2), (5, 5, -1)):
            out.append({"on": on, "note": note, "vel": vel, "extra": extra, "step": step, "length": length,
                        "pos": pos, "label": label(on, note, extra), "style": style(on, vel, step, length, pos)})
    return out


def next_division(cur):
    i = min(range(len(DIVS)), key=lambda j: abs(DIVS[j][0] - cur))
    return DIVS[(i + 1) % len(DIVS)][0]


def click_cases():
    out = []
    for on, vel in ((0, 100), (1, 100), (1, 127), (0, 127), (0, 64), (1, 64)):
        for accent in (False, True):
            seq = StepSequencer(None, 48000)
            seq.set_step(2, bool(on), 60, vel)
            cur_on, cur_vel = bool(seq.on[2]), int(seq.vel[2])
            if accent:
                seq.set_step(2, not (cur_on and cur_vel == ACCENT_VEL), vel=ACCENT_VEL)
            else:
                seq.set_step(2, not cur_on, vel=NORMAL_VEL if not cur_on else None)
            out.append({"on": on, "vel": vel, "accent": accent, "result": [int(seq.on[2]), int(seq.vel[2])]})
    return out


def write(out_dir: Path) -> None:
    out_dir = Path(out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    data = {
        "note_name": {str(n): note_name(n) for n in list(range(-13, 140))},
        "steps": step_cases(),
        "divs": [[d, n] for d, n in DIVS],
        "next_division": [[x, next_division(x)] for x in (0.25, 0.5, 1.0, 2.0, 4.0, 0.3, 3.0, 9.0, 0.1)],
        "next_length": [[n, n % STEPS + 1] for n in range(1, 17)],
        "rec": [[rec, playing, ("REC·LIVE" if playing else "REC·STEP") if rec else "REC", rec and not playing]
                for rec in (False, True) for playing in (False, True)],
        "clicks": click_cases(),
        "none": NONE,
    }
    (out_dir / "seq_view.json").write_text(json.dumps(data, indent=1, ensure_ascii=False) + "\n")
