"""Engine (mixer) reference: the golden render and mixer operation scenarios.

Each ("render", n) is one Engine.render call of n frames; it stores 1 in DECIM
samples (all renders of a scenario concatenated), per-render checksums, and the
(pos, bpm, running) state of every instance's own clock after the render.
"""

from __future__ import annotations

import math
from pathlib import Path

import numpy as np

from nomad.engine.engine import Engine
from nomad.engine.manager import InstrumentManager
from tools.refio import BlobWriter

SR = 48000
DECIM = 20
ROOT = Path(__file__).resolve().parent.parent
GOLDEN_SIZES = [512, 512, 300, 700, 1024, 2048, 37, 1000, 1500, 1000]
GOLDEN_RECIPE = ("Engine(48000); drums bpm 120; drums start; note_on(60, 0.8); note_on(67, 0.6); "
                 "render the sizes in order; after the 4th render: note_off(60), note_on(64, 1.0)")

SCENARIOS = {
    "mix/levels": [["start", 2], ["note_on", 60, 0.8], ["level", 1, 0.5], ["render", 4096], ["level", 2, 0.3],
                   ["render", 4096]],
    "mix/mute_ramp": [["start", 2], ["note_on", 60, 0.8], ["render", 2048], ["mute", 1, True], ["render", 2048],
                      ["mute", 1, False], ["render", 2048], ["mute", 2, True], ["render", 1024]],
    "mix/master": [["note_on", 60, 0.8], ["start", 2], ["render", 2048], ["master", 0.2], ["render", 4096],
                   ["master", 1.0], ["render", 2048]],
    "mix/level_nonfinite": [["level", 1, float("nan")], ["level", 2, -3.0], ["note_on", 60, 0.8], ["start", 2],
                            ["render", 3000], ["level", 2, 5.0], ["render", 3000]],
    "mix/bpm_clamp": [["bpm", 2, 500.0], ["render", 512], ["bpm", 2, float("nan")], ["start", 2],
                      ["render", 8192], ["bpm", 2, 5.0], ["param", "filt_cutoff", 300.0], ["note_on", 48, 1.0],
                      ["render", 4096]],
}


def golden() -> np.ndarray:
    return np.load(ROOT / "tests" / "data" / "engine_golden.npy").astype(np.float64)


def run(commands):
    e = Engine(SR)
    m = InstrumentManager(e)
    by_id = {i.id: i for i in m.instances}
    audio, sums, clocks, rets = [], [], [], []
    for c in commands:
        op = c[0]
        if op == "note_on":
            e.note_on(c[1], c[2])
        elif op == "note_off":
            e.note_off(c[1])
        elif op == "start":
            rets.append(m.start(by_id[c[1]]))
        elif op == "stop":
            rets.append(m.stop(by_id[c[1]]))
        elif op == "bpm":
            rets.append(m.set_bpm(by_id[c[1]], c[2]))
        elif op == "mute":
            m.set_mute(by_id[c[1]], c[2])
        elif op == "level":
            m.set_level(by_id[c[1]], c[2])
        elif op == "master":
            e.set_param("master_volume", c[1])
        elif op == "param":
            e.set_param(c[1], c[2])
        else:
            o = np.zeros(c[1])
            e.render(o)
            audio.append(o)
            sums.append([math.fsum(o), math.fsum(o * o)])
            clocks.append([[i.clock.pos, i.clock.bpm, float(i.clock.running)] for i in e.topology.insts])
    return np.concatenate(audio), sums, clocks, rets


def write(out_dir: Path) -> None:
    w = BlobWriter("engine")
    w.add("golden", golden(), recipe=GOLDEN_RECIPE, sizes=GOLDEN_SIZES)
    for label, cmds in SCENARIOS.items():
        y, sums, clocks, rets = run(cmds)
        w.add(label, y[::DECIM], commands=cmds, decim=DECIM, sums=sums, clocks=clocks, returns=rets)
    w.write(out_dir)
