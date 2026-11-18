"""Clock, StepGrid and StepTracker reference data for the Rust tests."""

from __future__ import annotations

from pathlib import Path

import numpy as np

from nomad.engine.clock import EPS, Clock, StepGrid, StepTracker
from tools.refio import BlobWriter

SR = 48000.0


def positions(g: StepGrid) -> np.ndarray:
    pos = [g.step_start(k) for k in range(25)]
    for k in (1, 2, 3, 8, 15):
        b = g.step_start(k)
        pos += [b - 1e-10, b + 1e-10, b - 1e-12, b + 1e-12]
    pos += [-EPS, 0.0, EPS, -2 * EPS, 2 * EPS]
    rng = np.random.default_rng(3)
    pos += list(rng.uniform(-0.1, 8.0, 200 - len(pos)))
    return np.array(pos)


def grid_entries(w: BlobWriter) -> None:
    for swing in (0.0, 0.3, 0.6):
        for div in (0.25, 1.0):
            g = StepGrid(div, 16, swing)
            pos = positions(g)
            label = f"grid/{swing}/{div}"
            w.add(label + "/pos", pos)
            w.add(label, np.array([[g.step_at(p), g.next_boundary(p)] for p in pos]).ravel(),
                  swing=swing, division=div)
            w.add(label + "/k", np.array([[g.step_beats(k), g.step_start(k)] for k in range(33)]).ravel())


def _on_boundary():
    # c2 runs ahead so that, when the tracker switches, c2 sits exactly on a step start.
    return [["bpm", 1, 120.0], ["bpm", 2, 120.0], ["start", 1], ["poll"], ["adv", 3000], ["poll"],
            ["start", 2], ["adv", 6000], ["poll"], ["clock", 2], ["poll"]] + [["adv", 3000], ["poll"]] * 8


TRACK = {
    "track/basic": [["bpm", 1, 120.0], ["start", 1]] + [["poll"], ["adv", 3000]] * 40,
    "track/engine_loop": [["bpm", 1, 97.0], ["swing", 0.35], ["start", 1]] + [["poll"], ["adv_fte"]] * 40,
    "track/restart": [["bpm", 1, 120.0], ["start", 1]] + [["poll"], ["adv", 2500]] * 10
    + [["stop", 1], ["poll"], ["start", 1]] + [["poll"], ["adv", 2500]] * 10,
    "track/bpm_change": [["bpm", 1, 120.0], ["start", 1]] + [["poll"], ["adv", 2000]] * 10
    + [["bpm", 1, 173.0]] + [["poll"], ["adv", 2000]] * 15,
    "track/swing_drag": [["bpm", 1, 120.0], ["start", 1]]
    + [c for i in range(10) for c in (["swing", 0.05 * (i + 1)], ["poll"], ["adv", 1700], ["poll"])],
    "track/div_change": [["bpm", 1, 120.0], ["start", 1]] + [["poll"], ["adv", 2000]] * 10
    + [["div", 1.0]] + [["poll"], ["adv", 2000]] * 10 + [["div", 0.5]] + [["poll"], ["adv", 2000]] * 10,
    "track/join_midrun": [["bpm", 1, 120.0], ["bpm", 2, 120.0], ["start", 1], ["start", 2],
                          ["adv", 1300]] + [["poll"], ["adv", 2000]] * 5 + [["clock", 2]]
    + [["poll"], ["adv", 2000]] * 12,
    "track/handover_on_boundary": _on_boundary(),
    "track/len5": [["bpm", 1, 120.0], ["len", 5], ["start", 1]] + [["poll"], ["adv", 6000]] * 14,
    "track/stopped": [["poll"], ["adv", 5000]] * 5 + [["start", 1], ["poll"], ["stop", 1]]
    + [["poll"], ["adv", 5000]] * 3,
}


def run_track(commands):
    clocks = {1: Clock(SR), 2: Clock(SR)}
    grid = StepGrid(0.25, 16, 0.0)
    tr = StepTracker(grid)
    cur = clocks[1]
    out = []
    for cmd in commands:
        op = cmd[0]
        if op == "start":
            clocks[cmd[1]].start()
        elif op == "stop":
            clocks[cmd[1]].stop()
        elif op == "bpm":
            clocks[cmd[1]].bpm = cmd[2]
        elif op == "adv":
            for c in clocks.values():
                c.advance(cmd[1])
        elif op == "adv_fte":
            n = tr.frames_to_event(cur)
            for c in clocks.values():
                c.advance(n)
        elif op == "swing":
            grid.swing = cmd[1]
        elif op == "div":
            grid.division = cmd[1]
        elif op == "len":
            grid.length = cmd[1]
        elif op == "clock":
            cur = clocks[cmd[1]]
        else:
            s = tr.poll(cur)
            out.append([-1 if s is None else s, tr.frames_to_event(cur)])
    return np.array(out, dtype=np.float64).ravel()


def write(out_dir: Path) -> None:
    w = BlobWriter("clock")
    grid_entries(w)
    for label, cmds in TRACK.items():
        w.add(label, run_track(cmds), commands=cmds)
    w.write(out_dir)
